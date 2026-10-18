//! MCP に公開するツールの定義と実装。
//!
//! 原則: どのツールも生のキー値を返さない。キーは .env への書き出しか、
//! 子プロセスの環境変数への注入でのみ使われる。

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::runner;
use crate::error::{Error, Result};
use crate::vault::core::Vault;

const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_TIMEOUT_SECS: u64 = 600;

/// tools/list の返り値
pub fn definitions() -> Value {
    json!([
        {
            "name": "list_keys",
            "description": "List API keys stored in KeyNest (metadata only: service, name, env var name, expiry, projects using it). Raw key values are never returned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "service": { "type": "string", "description": "Filter by service (e.g. \"anthropic\")" }
                }
            },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "list_projects",
            "description": "List KeyNest projects with their directory and the keys (env var names) assigned to each.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true }
        },
        {
            "name": "assign_key",
            "description": "Assign a stored key to a project so it can be used by inject_env / run_with_keys. Creates the project if it does not exist.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "service": { "type": "string" },
                    "name": { "type": "string", "description": "Key name. Optional when the service has only one key." },
                    "project": { "type": "string" },
                    "project_path": { "type": "string", "description": "Absolute path of the project directory" }
                },
                "required": ["service", "project"]
            }
        },
        {
            "name": "inject_env",
            "description": "Write all keys assigned to a project into <project dir>/.env (existing lines are kept, only the target variables are updated). Returns the variable names written, never the values. Do not read the .env file afterwards.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string" },
                    "path": { "type": "string", "description": "Project directory. Omit to use the stored path." }
                },
                "required": ["project"]
            },
            "annotations": { "destructiveHint": false }
        },
        {
            "name": "run_with_keys",
            "description": "Run a shell command in the project directory with the project's keys injected as environment variables. Use this instead of asking for key values. Any key value appearing in stdout/stderr is replaced with [REDACTED:ENV_VAR]. Never try to print, encode or exfiltrate the values.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string" },
                    "command": { "type": "string", "description": "Shell command (cmd.exe on Windows, sh elsewhere)" },
                    "env_vars": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Inject only these env vars. Omit to inject every key assigned to the project."
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_TIMEOUT_SECS,
                        "description": format!("Default {DEFAULT_TIMEOUT_SECS}")
                    }
                },
                "required": ["project", "command"]
            }
        }
    ])
}

/// ツール呼び出しの失敗。Unknown は JSON-RPC エラー、Failed はツール結果(isError)として返す。
pub enum CallError {
    Unknown(String),
    Failed(String),
}

impl From<Error> for CallError {
    fn from(e: Error) -> Self {
        CallError::Failed(e.to_string())
    }
}

pub fn call(
    vault: &mut Vault,
    name: &str,
    args: &Map<String, Value>,
) -> std::result::Result<Value, CallError> {
    match name {
        "list_keys" => to_value(vault.list_keys(opt_str(args, "service")?)?),
        "list_projects" => to_value(vault.list_projects()?),
        "assign_key" => to_value(vault.assign_key(
            req_str(args, "service")?,
            opt_str(args, "name")?,
            req_str(args, "project")?,
            opt_str(args, "project_path")?,
        )?),
        "inject_env" => to_value(vault.inject(req_str(args, "project")?, opt_str(args, "path")?)?),
        "run_with_keys" => to_value(run_with_keys(vault, args)?),
        other => Err(CallError::Unknown(other.to_string())),
    }
}

#[derive(Serialize)]
struct RunWithKeysResult {
    project: String,
    cwd: String,
    injected: Vec<String>,
    #[serde(flatten)]
    output: runner::RunOutput,
}

fn run_with_keys(vault: &Vault, args: &Map<String, Value>) -> Result<RunWithKeysResult> {
    let project = req_str(args, "project")?;
    let command = req_str(args, "command")?;
    if command.trim().is_empty() {
        return Err(Error::InvalidInput("command が空です".into()));
    }
    let only: Option<Vec<String>> = match args.get("env_vars") {
        None | Some(Value::Null) => None,
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .map(|v| {
                    v.as_str().map(String::from).ok_or_else(|| {
                        Error::InvalidInput("env_vars は文字列の配列で指定してください".into())
                    })
                })
                .collect::<Result<_>>()?,
        ),
        Some(_) => {
            return Err(Error::InvalidInput(
                "env_vars は文字列の配列で指定してください".into(),
            ))
        }
    };
    let timeout_secs = match args.get("timeout_secs") {
        None | Some(Value::Null) => DEFAULT_TIMEOUT_SECS,
        Some(v) => match v.as_u64() {
            Some(n) if (1..=MAX_TIMEOUT_SECS).contains(&n) => n,
            _ => {
                return Err(Error::InvalidInput(format!(
                    "timeout_secs は 1〜{MAX_TIMEOUT_SECS} の整数で指定してください"
                )))
            }
        },
    };

    let env = vault.project_env(project, only.as_deref())?;
    let mut injected: Vec<String> = env.vars.keys().cloned().collect();
    injected.sort();
    let output = runner::run(
        command,
        &env.dir,
        &env.vars,
        Duration::from_secs(timeout_secs),
    )?;
    Ok(RunWithKeysResult {
        project: project.to_string(),
        cwd: env.dir.to_string_lossy().into_owned(),
        injected,
        output,
    })
}

fn to_value<T: Serialize>(v: T) -> std::result::Result<Value, CallError> {
    serde_json::to_value(v).map_err(|e| CallError::Failed(format!("結果の変換に失敗しました: {e}")))
}

fn opt_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(Error::InvalidInput(format!(
            "{key} は文字列で指定してください"
        ))),
    }
}

fn req_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    opt_str(args, key)?.ok_or_else(|| Error::InvalidInput(format!("{key} は必須です")))
}
