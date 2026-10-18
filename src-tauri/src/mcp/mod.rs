//! mcp: Claude Code などの MCP クライアントから KeyNest のキーを使うための stdio サーバー。
//!
//! - プロトコルは JSON-RPC 2.0 を改行区切りで stdin/stdout に流す(MCP stdio transport)。
//! - stdout はプロトコル専用。ログは stderr に出す。
//! - 生のキー値はどのレスポンスにも含めない(tools.rs 参照)。
//! - このモジュールは Tauri に依存しない。エントリポイントは src/bin/keynest-mcp.rs。

mod runner;
mod tools;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::{json, Map, Value};

use crate::vault::core::Vault;
use crate::vault::secrets::KeyringStore;

/// tauri.conf.json の identifier。GUI と同じ DB を開くため一致させること。
const APP_IDENTIFIER: &str = "com.tomuai.keynest";

/// 対応する MCP プロトコルバージョン(新しい順)。
/// クライアントの要求がこの中にあればそれを、なければ先頭を返す。
const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "KeyNest stores the user's API keys in the OS secure store. \
Key values are never returned to you. To use a key, call run_with_keys (injects keys as env vars \
into a command; values in the output are redacted) or inject_env (writes the project's .env). \
Never try to read, print or encode key values.";

/// GUI と同じ DB パス: 環境変数 KEYNEST_DB、なければ <データディレクトリ>/<identifier>/keynest.db。
fn db_path() -> Option<PathBuf> {
    match std::env::var_os("KEYNEST_DB") {
        Some(custom) => Some(PathBuf::from(custom)),
        None => Some(dirs::data_dir()?.join(APP_IDENTIFIER).join("keynest.db")),
    }
}

/// keynest-mcp バイナリのエントリポイント。
pub fn main() -> ExitCode {
    let Some(path) = db_path() else {
        eprintln!(
            "keynest-mcp: データディレクトリを特定できません / cannot determine data directory"
        );
        return ExitCode::FAILURE;
    };
    // DB を新規作成しない: 空の DB を先に作ると、GUI 側の旧パスからの移行
    // (lib.rs の migrate_legacy_db)が行われなくなるため。
    if !path.exists() {
        eprintln!(
            "keynest-mcp: DB が見つかりません: {}\n\
             KeyNest アプリを一度起動してからやり直してください。 / \
             Database not found. Launch the KeyNest app once first.",
            path.display()
        );
        return ExitCode::FAILURE;
    }
    let mut vault = match Vault::open(&path, Box::new(KeyringStore)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("keynest-mcp: DB を開けませんでした / failed to open database: {e}");
            return ExitCode::FAILURE;
        }
    };
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    match serve(&mut vault, stdin, stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("keynest-mcp: 入出力エラー / I/O error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// 入力が閉じられるまで、1 行 1 メッセージで要求を処理する。
pub fn serve<R: BufRead, W: Write>(
    vault: &mut Vault,
    input: R,
    mut output: W,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(vault, &msg),
            Err(e) => Some(error_response(
                Value::Null,
                -32700,
                &format!("Parse error: {e}"),
            )),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}

/// 1 メッセージを処理する。通知(id なし)とクライアントからの応答には何も返さない。
fn handle(vault: &mut Vault, msg: &Value) -> Option<Value> {
    let Some(obj) = msg.as_object() else {
        return Some(error_response(Value::Null, -32600, "Invalid Request"));
    };
    let id = obj.get("id").cloned();
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        // サーバーからは要求を送らないので、method のないメッセージ(応答)は無視する
        return None;
    };
    let id = id?;
    let empty = Map::new();
    let params = obj
        .get("params")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let result = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let version = requested
                .filter(|v| PROTOCOL_VERSIONS.contains(v))
                .unwrap_or(PROTOCOL_VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "keynest", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools::definitions() }),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error_response(id, -32602, "params.name is required"));
            };
            let args = params
                .get("arguments")
                .and_then(Value::as_object)
                .unwrap_or(&empty);
            match tools::call(vault, name, args) {
                Ok(value) => tool_result(
                    serde_json::to_string_pretty(&value).unwrap_or_default(),
                    false,
                ),
                Err(tools::CallError::Failed(message)) => tool_result(message, true),
                Err(tools::CallError::Unknown(name)) => {
                    return Some(error_response(id, -32602, &format!("Unknown tool: {name}")))
                }
            }
        }
        other => {
            return Some(error_response(
                id,
                -32601,
                &format!("Method not found: {other}"),
            ))
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn tool_result(text: String, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::secrets::memory::MemoryStore;
    use tempfile::TempDir;

    const SECRET: &str = "sk-ant-api03-mcp-test-value-0123456789";

    fn test_vault() -> (Vault, TempDir) {
        let dir = TempDir::new().unwrap();
        let vault = Vault::open(
            &dir.path().join("vault.db"),
            Box::new(MemoryStore::default()),
        )
        .unwrap();
        (vault, dir)
    }

    /// 要求を順に流し、応答を 1 行ずつパースして返す
    fn roundtrip(vault: &mut Vault, requests: &[Value]) -> (String, Vec<Value>) {
        let input: String = requests.iter().map(|r| format!("{r}\n")).collect();
        let mut output = Vec::new();
        serve(vault, input.as_bytes(), &mut output).unwrap();
        let raw = String::from_utf8(output).unwrap();
        let parsed = raw
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        (raw, parsed)
    }

    fn call(id: i64, name: &str, arguments: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": name, "arguments": arguments } })
    }

    fn tool_text(resp: &Value) -> (&str, bool) {
        let result = &resp["result"];
        (
            result["content"][0]["text"].as_str().unwrap(),
            result["isError"].as_bool().unwrap(),
        )
    }

    #[test]
    fn initialize_and_list_tools() {
        let (mut vault, _dir) = test_vault();
        let (_, resp) = roundtrip(
            &mut vault,
            &[
                json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": { "protocolVersion": "2025-03-26", "capabilities": {},
                                    "clientInfo": { "name": "test", "version": "0" } } }),
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            ],
        );
        // 通知には応答しない
        assert_eq!(resp.len(), 2);
        assert_eq!(resp[0]["result"]["protocolVersion"], "2025-03-26");
        let names: Vec<&str> = resp[1]["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "list_keys",
                "list_projects",
                "assign_key",
                "inject_env",
                "run_with_keys"
            ]
        );
    }

    #[test]
    fn unknown_protocol_version_falls_back_to_latest() {
        let (mut vault, _dir) = test_vault();
        let (_, resp) = roundtrip(
            &mut vault,
            &[json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                      "params": { "protocolVersion": "1999-01-01" } })],
        );
        assert_eq!(resp[0]["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[test]
    fn errors_for_bad_input() {
        let (mut vault, _dir) = test_vault();
        let input = "not json\n{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"nope\"}\n";
        let mut output = Vec::new();
        serve(&mut vault, input.as_bytes(), &mut output).unwrap();
        let resp: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(resp[0]["error"]["code"], -32700);
        assert_eq!(resp[1]["error"]["code"], -32601);

        let (_, resp) = roundtrip(&mut vault, &[call(2, "get_key", json!({}))]);
        assert_eq!(resp[0]["error"]["code"], -32602);

        let (_, resp) = roundtrip(
            &mut vault,
            &[call(3, "assign_key", json!({ "service": 1 }))],
        );
        let (text, is_error) = tool_text(&resp[0]);
        assert!(is_error);
        assert!(text.contains("service"), "{text}");
    }

    #[test]
    fn raw_value_never_appears_in_any_response() {
        let (mut vault, dir) = test_vault();
        let project_dir = dir.path().join("app");
        std::fs::create_dir(&project_dir).unwrap();
        vault
            .set_key(
                "anthropic",
                "default",
                SECRET,
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        let echo = if cfg!(windows) {
            "echo %ANTHROPIC_API_KEY%"
        } else {
            "echo $ANTHROPIC_API_KEY"
        };
        let (raw, resp) = roundtrip(
            &mut vault,
            &[
                call(
                    1,
                    "assign_key",
                    json!({ "service": "anthropic", "project": "app",
                                              "project_path": project_dir.to_str().unwrap() }),
                ),
                call(2, "list_keys", json!({})),
                call(3, "list_projects", json!({})),
                call(4, "inject_env", json!({ "project": "app" })),
                call(
                    5,
                    "run_with_keys",
                    json!({ "project": "app", "command": echo }),
                ),
            ],
        );
        assert!(
            !raw.contains(SECRET),
            "生の値がレスポンスに含まれている: {raw}"
        );
        assert!(
            !raw.contains("0123456789"),
            "値の一部がレスポンスに含まれている: {raw}"
        );
        for r in &resp {
            assert!(!tool_text(r).1, "{r}");
        }

        // inject_env は .env に書き出す(返り値は変数名のみ)
        let env = std::fs::read_to_string(project_dir.join(".env")).unwrap();
        assert!(env.contains(&format!("ANTHROPIC_API_KEY={SECRET}")));

        // run_with_keys は注入され、出力は伏せ字になる
        let run: Value = serde_json::from_str(tool_text(&resp[4]).0).unwrap();
        assert_eq!(run["exit_code"], 0);
        assert_eq!(run["injected"], json!(["ANTHROPIC_API_KEY"]));
        assert_eq!(run["redacted"], json!(["ANTHROPIC_API_KEY"]));
        assert!(run["stdout"]
            .as_str()
            .unwrap()
            .contains("[REDACTED:ANTHROPIC_API_KEY]"));
    }

    #[test]
    fn run_with_keys_rejects_unassigned_env_var() {
        let (mut vault, dir) = test_vault();
        vault
            .set_key(
                "anthropic",
                "default",
                SECRET,
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault
            .assign_key("anthropic", None, "app", Some(dir.path().to_str().unwrap()))
            .unwrap();
        let (_, resp) = roundtrip(
            &mut vault,
            &[call(
                1,
                "run_with_keys",
                json!({ "project": "app", "command": "echo hi",
                                               "env_vars": ["OPENAI_API_KEY"] }),
            )],
        );
        let (text, is_error) = tool_text(&resp[0]);
        assert!(is_error);
        assert!(text.contains("OPENAI_API_KEY"), "{text}");
    }

    #[test]
    fn run_with_keys_requires_project_path() {
        let (mut vault, _dir) = test_vault();
        vault
            .set_key(
                "anthropic",
                "default",
                SECRET,
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault.assign_key("anthropic", None, "nopath", None).unwrap();
        let (_, resp) = roundtrip(
            &mut vault,
            &[call(
                1,
                "run_with_keys",
                json!({ "project": "nopath", "command": "echo hi" }),
            )],
        );
        let (text, is_error) = tool_text(&resp[0]);
        assert!(is_error);
        assert!(text.contains("パスが未設定"), "{text}");
    }
}
