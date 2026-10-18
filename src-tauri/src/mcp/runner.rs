//! run_with_keys の実体: キーを環境変数に注入して子プロセスを実行し、
//! 出力に含まれるキーの値を伏せ字に置き換えてから返す。
//!
//! 伏せ字化は「うっかり値が会話に載る」事故を防ぐためのもので、
//! エンコードして出力する等の意図的な持ち出しまでは防げない(README 参照)。

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};

/// 1 ストリームあたり返す出力の上限(末尾を残す)
const OUTPUT_CAP: usize = 32 * 1024;
/// 子プロセス終了後、孫プロセスがパイプを握っている場合に読み取りを待つ上限
const DRAIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize)]
pub struct RunOutput {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: String,
    pub stderr: String,
    /// 出力が上限を超え、先頭が切り捨てられたか
    pub truncated: bool,
    /// 出力中に値が見つかり伏せ字にした環境変数名
    pub redacted: Vec<String>,
}

/// 上限付きの出力バッファ。上限の 2 倍を超えたら先頭を捨てて末尾 OUTPUT_CAP を残す。
#[derive(Default)]
struct Capture {
    buf: Vec<u8>,
    dropped: bool,
}

fn spawn_reader<R: Read + Send + 'static>(mut src: R, cap: Arc<Mutex<Capture>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match src.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut c = cap.lock().unwrap_or_else(|e| e.into_inner());
                    c.buf.extend_from_slice(&chunk[..n]);
                    if c.buf.len() > OUTPUT_CAP * 2 {
                        let excess = c.buf.len() - OUTPUT_CAP;
                        c.buf.drain(..excess);
                        c.dropped = true;
                    }
                }
            }
        }
    })
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut cmd = Command::new("cmd");
        // /S: 先頭と末尾の引用符だけを外し、中身はそのまま cmd に渡す
        cmd.args(["/D", "/S", "/C"])
            .raw_arg(format!("\"{command}\""))
            .creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        // タイムアウト時にプロセスグループごと止められるよう、新しいグループで起動する
        cmd.process_group(0);
        cmd
    }
}

/// タイムアウト時は孫プロセスも含めて止める(npm → node のような多段起動に備える)。
fn kill_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &pid, "/T", "/F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(not(windows))]
    let _ = Command::new("kill")
        .args(["-KILL", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
}

/// command をシェル経由で dir 上で実行する。vars は継承した環境変数に上書き追加される。
pub fn run(
    command: &str,
    dir: &Path,
    vars: &HashMap<String, String>,
    timeout: Duration,
) -> Result<RunOutput> {
    let mut child = shell_command(command)
        .current_dir(dir)
        .envs(vars)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Error::Internal(format!("コマンドを起動できませんでした: {e}")))?;

    let out_cap = Arc::new(Mutex::new(Capture::default()));
    let err_cap = Arc::new(Mutex::new(Capture::default()));
    let readers = [
        spawn_reader(
            child.stdout.take().expect("stdout は piped"),
            out_cap.clone(),
        ),
        spawn_reader(
            child.stderr.take().expect("stderr は piped"),
            err_cap.clone(),
        ),
    ];

    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            kill_tree(&mut child);
            break child.wait()?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    // 孫プロセスがパイプを開いたままだと読み取りが終わらないため、待つのは DRAIN_GRACE まで
    let drain_deadline = Instant::now() + DRAIN_GRACE;
    while readers.iter().any(|r| !r.is_finished()) && Instant::now() < drain_deadline {
        std::thread::sleep(Duration::from_millis(20));
    }

    let secrets: Vec<(&str, &str)> = vars.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let take = |cap: &Arc<Mutex<Capture>>, reader: &JoinHandle<()>| {
        // 読み取りが終わっていなければ、末尾は書き込みの途中で切れている可能性がある
        let incomplete = !reader.is_finished();
        let c = cap.lock().unwrap_or_else(|e| e.into_inner());
        let (text, hits) = redact(&c.buf, c.dropped, incomplete, &secrets);
        (text, hits, c.dropped || incomplete)
    };
    let (stdout, mut redacted, out_dropped) = take(&out_cap, &readers[0]);
    let (stderr, err_hits, err_dropped) = take(&err_cap, &readers[1]);
    redacted.extend(err_hits);
    redacted.sort();
    redacted.dedup();

    Ok(RunOutput {
        exit_code: status.code(),
        timed_out,
        stdout,
        stderr,
        truncated: out_dropped || err_dropped,
        redacted,
    })
}

/// 出力から secrets の値をすべて `[REDACTED:環境変数名]` に置き換える。
/// 返り値は (置換後のテキスト, 値が見つかった環境変数名)。
///
/// 値そのものに加え、ログに現れやすい表現も対象にする:
/// UTF-16LE/BE(Windows の一部コマンドの出力)、URL エンコード、JSON の "\/" エスケープ。
///
/// dropped(先頭を切り捨て済み)のときは、切れ目に値の後半だけが残っている可能性があるため、
/// 最長のパターンの長さぶん先頭をさらに捨ててから置換する。
/// incomplete(読み取り途中で打ち切った)のときは、末尾に値の前半だけが残っている可能性があるため、
/// いずれかのパターンの前半と一致する末尾を捨てる。
fn redact(
    raw: &[u8],
    dropped: bool,
    incomplete: bool,
    secrets: &[(&str, &str)],
) -> (String, Vec<String>) {
    let mut patterns: Vec<(&str, Vec<u8>)> = secrets
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .flat_map(|(name, value)| encodings(value).into_iter().map(move |p| (*name, p)))
        .collect();
    // 長いパターンから置換する(ある値が別の値を含む場合に、短い方で先に崩さない)
    patterns.sort_by_key(|(_, p)| std::cmp::Reverse(p.len()));
    let longest = patterns.first().map_or(0, |(_, p)| p.len());

    let mut data: Vec<u8> = if dropped {
        raw[longest.min(raw.len())..].to_vec()
    } else {
        raw.to_vec()
    };

    let mut hits = Vec::new();
    for (name, needle) in &patterns {
        let marker = format!("[REDACTED:{name}]");
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        let mut found = false;
        while i < data.len() {
            if data[i..].starts_with(needle) {
                out.extend_from_slice(marker.as_bytes());
                i += needle.len();
                found = true;
            } else {
                out.push(data[i]);
                i += 1;
            }
        }
        if found && !hits.iter().any(|h| h == name) {
            hits.push(name.to_string());
        }
        data = out;
    }

    if incomplete {
        let partial = patterns
            .iter()
            .flat_map(|(_, p)| (1..p.len()).filter(|&k| data.ends_with(&p[..k])))
            .max()
            .unwrap_or(0);
        data.truncate(data.len() - partial);
    }
    (String::from_utf8_lossy(&data).into_owned(), hits)
}

/// 値がログに現れうる表現の一覧(重複は除く)。
fn encodings(value: &str) -> Vec<Vec<u8>> {
    let mut out = vec![
        value.as_bytes().to_vec(),
        value.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        value.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        value.replace('/', "\\/").into_bytes(),
    ];
    let mut url = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    out.push(url.into_bytes());
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_replaces_all_occurrences() {
        let (text, hits) = redact(
            b"key=sk-secret-123 again sk-secret-123",
            false,
            false,
            &[("API_KEY", "sk-secret-123")],
        );
        assert_eq!(text, "key=[REDACTED:API_KEY] again [REDACTED:API_KEY]");
        assert_eq!(hits, vec!["API_KEY"]);
    }

    #[test]
    fn redact_prefers_longer_value_when_nested() {
        let (text, mut hits) = redact(
            b"abcdef-long and abcdef",
            false,
            false,
            &[("SHORT", "abcdef"), ("LONG", "abcdef-long")],
        );
        hits.sort();
        assert_eq!(text, "[REDACTED:LONG] and [REDACTED:SHORT]");
        assert_eq!(hits, vec!["LONG", "SHORT"]);
    }

    #[test]
    fn redact_drops_possible_partial_secret_after_truncation() {
        // 先頭が切り捨てられ、値の後半 "cret-123" だけが残っているケース
        // (捨てる幅は最長パターン = UTF-16 表現の 26 バイトなので、それより長い入力にする)
        let (text, hits) = redact(
            b"cret-123 ................................ tail output",
            true,
            false,
            &[("K", "sk-secret-123")],
        );
        assert!(!text.contains("cret-123"), "{text}");
        assert!(text.ends_with("output"));
        assert!(hits.is_empty());
    }

    #[test]
    fn redact_trims_partial_secret_at_end_when_incomplete() {
        // 読み取り途中で打ち切り、値の前半 "sk-secr" だけが末尾に残っているケース
        let (text, _) = redact(b"log line sk-secr", false, true, &[("K", "sk-secret-123")]);
        assert!(!text.contains("sk-secr"), "{text}");
        assert!(text.starts_with("log line"), "{text}");
    }

    #[test]
    fn redact_handles_utf16_output() {
        // Windows の一部コマンドは UTF-16LE で出力する
        let raw: Vec<u8> = "value=sk-secret-123 tail"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let (text, hits) = redact(&raw, false, false, &[("K", "sk-secret-123")]);
        let visible: String = text.chars().filter(|c| *c != '\0').collect();
        assert!(!visible.contains("sk-secret-123"), "{visible:?}");
        assert!(!visible.contains("secret"), "{visible:?}");
        assert_eq!(hits, vec!["K"]);
    }

    #[test]
    fn redact_handles_url_and_json_escaped_forms() {
        let secret = "abc+def/ghi=jkl";
        let (text, hits) = redact(
            br#"GET /v1?key=abc%2Bdef%2Fghi%3Djkl {"k":"abc+def\/ghi=jkl"}"#,
            false,
            false,
            &[("K", secret)],
        );
        assert!(!text.contains("def"), "{text}");
        assert_eq!(hits, vec!["K"]);
    }

    #[test]
    fn redact_without_secrets_is_identity() {
        let (text, hits) = redact(b"hello", false, false, &[]);
        assert_eq!(text, "hello");
        assert!(hits.is_empty());
    }

    fn print_env_command(var: &str) -> String {
        if cfg!(windows) {
            format!("echo value=%{var}%")
        } else {
            format!("echo value=${var}")
        }
    }

    #[test]
    fn run_injects_env_and_redacts_output() {
        let dir = tempfile::TempDir::new().unwrap();
        let vars = HashMap::from([(
            "KEYNEST_TEST_SECRET".to_string(),
            "s3cr3t-value-xyz".to_string(),
        )]);
        let out = run(
            &print_env_command("KEYNEST_TEST_SECRET"),
            dir.path(),
            &vars,
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(out.exit_code, Some(0));
        assert!(!out.timed_out);
        assert!(!out.stdout.contains("s3cr3t-value-xyz"), "{}", out.stdout);
        assert!(
            out.stdout.contains("value=[REDACTED:KEYNEST_TEST_SECRET]"),
            "{}",
            out.stdout
        );
        assert_eq!(out.redacted, vec!["KEYNEST_TEST_SECRET"]);
    }

    #[test]
    fn run_reports_nonzero_exit_code() {
        let dir = tempfile::TempDir::new().unwrap();
        let out = run(
            "exit 3",
            dir.path(),
            &HashMap::new(),
            Duration::from_secs(30),
        )
        .unwrap();
        assert_eq!(out.exit_code, Some(3));
    }

    #[test]
    fn run_times_out() {
        let dir = tempfile::TempDir::new().unwrap();
        let command = if cfg!(windows) {
            "ping -n 30 127.0.0.1 > NUL"
        } else {
            "sleep 30"
        };
        let started = Instant::now();
        let out = run(
            command,
            dir.path(),
            &HashMap::new(),
            Duration::from_millis(500),
        )
        .unwrap();
        assert!(out.timed_out);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
