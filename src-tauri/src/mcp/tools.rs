//! MCP に公開するツールの定義と実装。
//!
//! 原則: どのツールも生のキー値を返さない。キーは .env への書き出しか、
//! 子プロセスの環境変数への注入でのみ使われる。

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::runner;
use crate::error::{Error, Result};
use crate::vault::core::{AssignResult, InjectResult, Vault};

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
                    "project_path": { "type": "string", "description": "Absolute path of the project directory. Can only be set when the project has no path yet; changing it requires the KeyNest app." }
                },
                "required": ["service", "project"]
            }
        },
        {
            "name": "inject_env",
            "description": "Write all keys assigned to a project into <stored project dir>/.env (existing lines are kept, only the target variables are updated). Refused inside a git repository unless .env is git-ignored. Returns the variable names written, never the values. Do not read the .env file afterwards.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string" }
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
        "assign_key" => to_value(assign_key(vault, args)?),
        "inject_env" => to_value(inject_env(vault, args)?),
        "run_with_keys" => to_value(run_with_keys(vault, args)?),
        other => Err(CallError::Unknown(other.to_string())),
    }
}

/// 保存済みのプロジェクトパスは inject_env / run_with_keys の書き込み・実行先になり、
/// GUI からの書き出しにも使われる。MCP 経由では新規設定のみ許し、変更は GUI に限る。
fn assign_key(vault: &mut Vault, args: &Map<String, Value>) -> Result<AssignResult> {
    let service = req_str(args, "service")?;
    let name = opt_str(args, "name")?;
    let project = req_str(args, "project")?;
    let project_path = opt_str(args, "project_path")?;
    if let Some(new_path) = project_path {
        match vault.find_project(project) {
            Ok((_, Some(stored))) if stored != new_path => {
                return Err(Error::InvalidInput(format!(
                    "プロジェクト {project} のパスは設定済みです({stored})。\
                     パスの変更は KeyNest アプリで行ってください"
                )));
            }
            Ok(_) | Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    vault.assign_key(service, name, project, project_path)
}

fn inject_env(vault: &mut Vault, args: &Map<String, Value>) -> Result<InjectResult> {
    let project = req_str(args, "project")?;
    if args.get("path").is_some_and(|v| !v.is_null()) {
        return Err(Error::InvalidInput(
            "MCP からは書き出し先を指定できません。プロジェクトのパスは KeyNest アプリで設定してください"
                .into(),
        ));
    }
    if let (_, Some(dir)) = vault.find_project(project)? {
        ensure_env_not_committable(Path::new(&dir))?;
    }
    vault.inject(project, None)
}

/// git 管理下に .env を書き出すと、AI がそのままコミット・push してしまう事故が起きうる。
/// 上位ディレクトリに .git があれば、.env が ignore されていることを git で確認できた場合だけ許す
/// (git が使えない・判定に失敗した場合も拒否する)。
fn ensure_env_not_committable(dir: &Path) -> Result<()> {
    if !dir.ancestors().any(|d| d.join(".git").exists()) {
        return Ok(());
    }
    let ignored = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "-q", ".env"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ignored {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "{} は git リポジトリ内ですが、.env が .gitignore で除外されていない(または追跡済み)ため書き出しません。\
             .gitignore に .env を追加してから再実行してください",
            dir.display()
        )))
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
