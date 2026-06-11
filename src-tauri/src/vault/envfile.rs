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

    std::fs::write(env_path, out.join("\n") + "\n")?;
    Ok(())
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
}
