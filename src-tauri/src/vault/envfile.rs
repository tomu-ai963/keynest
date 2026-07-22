//! .env ファイルの読み書きヘルパー。

use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;

/// 既存の .env を保持しつつ、対象の変数だけ更新・追記する。
/// コメント行・空行・対象外の変数はそのまま残す。
pub fn merge(env_path: &Path, updates: &HashMap<String, String>) -> Result<()> {
    let lines: Vec<String> = if env_path.exists() {
        std::fs::read_to_string(env_path)?
            .lines()
            .map(String::from)
            .collect()
    } else {
        Vec::new()
    };

    let mut remaining = updates.clone();
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + remaining.len());
    for line in lines {
        let stripped = line.trim();
        if !stripped.is_empty() && !stripped.starts_with('#') && stripped.contains('=') {
            let var = stripped.split('=').next().unwrap_or("").trim();
            if let Some(value) = remaining.remove(var) {
                out.push(format!("{var}={value}"));
                continue;
            }
        }
        out.push(line);
    }
    // 追記分は順序を安定させる
    let mut appended: Vec<(String, String)> = remaining.into_iter().collect();
    appended.sort();
    for (var, value) in appended {
        out.push(format!("{var}={value}"));
    }

    write_env(env_path, &(out.join("\n") + "\n"))?;
    Ok(())
}

/// .env を書き出す。シークレットを含むため、Unix では umask に依存せず
/// パーミッションを所有者のみ読み書き可(600)に固定する。
#[cfg(unix)]
fn write_env(env_path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(env_path)?;
    // mode は新規作成時にしか効かないため、既存ファイルにも明示的に適用する
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(content.as_bytes())
}

#[cfg(not(unix))]
fn write_env(env_path: &Path, content: &str) -> std::io::Result<()> {
    std::fs::write(env_path, content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn updates(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn merge_creates_new_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        merge(&path, &updates(&[("A_KEY", "value-a")])).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "A_KEY=value-a\n");
    }

    #[test]
    fn merge_preserves_comments_and_unrelated_vars() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");
        std::fs::write(&path, "# comment\nOTHER=keep\nA_KEY=old\n").unwrap();
        merge(&path, &updates(&[("A_KEY", "new"), ("B_KEY", "added")])).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "# comment\nOTHER=keep\nA_KEY=new\nB_KEY=added\n");
    }

    #[cfg(unix)]
    #[test]
    fn merge_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".env");

        // 新規作成時: umask に関係なく 600
        merge(&path, &updates(&[("A_KEY", "value-a")])).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // 既存ファイルが緩い権限でも 600 に絞られる
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        merge(&path, &updates(&[("B_KEY", "value-b")])).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
