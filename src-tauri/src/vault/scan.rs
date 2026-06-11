//! .env スキャン取り込み(scan.py の移植)。
//!
//! 流れ:
//!   1. scan_preview: 指定ディレクトリ以下から .env / .env.* を探索し、
//!      シークレットらしい変数を判定・サービス推定・既存キーと照合して候補を返す。
//!   2. UI 側で取り込む変数を選択する。
//!   3. scan_apply: 選択された変数だけ .env を読み直して登録し、
//!      .env のあるディレクトリをプロジェクトとして紐づける。
//!
//! 設計原則は core.rs と同じ: 生のキー値は返り値(= IPC)に含めない。
//! プレビューと適用の間に .env が編集されても、適用時に読み直すためズレは起きない。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::core::{mask, Vault};
use super::presets::PRESETS;
use super::secrets::SecretStore;
use crate::error::{Error, Result};

/// 探索しないディレクトリ
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "target",
    ".tox",
    ".mypy_cache",
];

/// 取り込まない .env 系ファイルの末尾(テンプレート類)
const SKIP_SUFFIXES: &[&str] = &["example", "sample", "template", "dist"];

/// シークレットらしい変数名に含まれる語(大文字で比較)
const SECRET_NAME_HINTS: &[&str] = &[
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "AUTH",
    "DSN",
    "PRIVATE",
    "ACCOUNT_ID",
];

#[derive(Debug, Serialize)]
pub struct ScanEntry {
    pub var: String,
    /// "new" | "exists" | "conflict" | "skipped"
    pub status: &'static str,
    pub service: Option<String>,
    pub value_masked: Option<String>,
    /// exists / conflict のとき、該当する既存キーのラベル("service/name")
    pub existing_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScanFile {
    pub file: String,
    /// .env のあるディレクトリ名(適用時のプロジェクト名になる)
    pub project: String,
    pub entries: Vec<ScanEntry>,
}

#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub files_scanned: usize,
    pub summary: HashMap<&'static str, usize>,
    pub files: Vec<ScanFile>,
}

/// 適用対象の選択。file は scan_preview が返したパス、vars は取り込む変数名。
#[derive(Debug, Deserialize)]
pub struct ScanSelection {
    pub file: String,
    pub vars: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ScanApplyEntry {
    pub file: String,
    pub var: String,
    /// "imported" | "linked" | "conflict" | "missing"
    pub status: &'static str,
    pub key: Option<String>,
    pub project: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScanApplyReport {
    pub summary: HashMap<&'static str, usize>,
    pub results: Vec<ScanApplyEntry>,
}

fn find_env_files(root: &Path, max_depth: u32) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, 0, max_depth, &mut found);
    found
}

fn walk(dir: &Path, depth: u32, max_depth: u32, found: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if depth < max_depth {
                walk(&path, depth + 1, max_depth, found);
            }
        } else if name == ".env" || name.starts_with(".env.") {
            let suffix = name.rsplit('.').next().unwrap_or("").to_lowercase();
            if SKIP_SUFFIXES.contains(&suffix.as_str()) {
                continue;
            }
            found.push(path);
        }
    }
}

fn is_valid_var_name(var: &str) -> bool {
    let mut chars = var.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// KEY=VALUE 行を抽出する。コメント・空行は無視、引用符と export は除去。
fn parse_env(text: &str) -> Vec<(String, String)> {
    let mut entries = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((var, value)) = line.split_once('=') else {
            continue;
        };
        let var = var.trim();
        let mut value = value.trim().to_string();
        if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
            if value.len() >= 2 && value.ends_with(quote) {
                value = value[1..value.len() - 1].to_string();
            }
        } else if let Some((head, _)) = value.split_once(" #") {
            // 行末コメントを除去(引用符なしの場合のみ)
            value = head.trim().to_string();
        }
        if is_valid_var_name(var) && !value.is_empty() {
            entries.push((var.to_string(), value));
        }
    }
    entries
}

fn preset_for_env_var(var: &str) -> Option<&'static str> {
    PRESETS
        .iter()
        .find(|p| p.env_var == var || p.env_var_aliases.contains(&var))
        .map(|p| p.id)
}

fn preset_for_value(value: &str) -> Option<&'static str> {
    PRESETS
        .iter()
        .find(|p| p.value_prefixes.iter().any(|pre| value.starts_with(pre)))
        .map(|p| p.id)
}

fn looks_like_secret(var: &str, value: &str) -> bool {
    if preset_for_env_var(var).is_some() || preset_for_value(value).is_some() {
        return true;
    }
    let upper = var.to_uppercase();
    SECRET_NAME_HINTS.iter().any(|hint| upper.contains(hint))
}

fn infer_service(var: &str, value: &str) -> String {
    if let Some(id) = preset_for_env_var(var) {
        return id.to_string();
    }
    if let Some(id) = preset_for_value(value) {
        return id.to_string();
    }
    // フォールバック: 変数名の先頭語をサービス名とみなす(MYAPP_API_KEY → myapp)
    let head = var.split('_').next().unwrap_or("").to_lowercase();
    if head.len() >= 2 {
        head
    } else {
        "unknown".to_string()
    }
}

/// .env のパスからプロジェクト名(親ディレクトリ名)とパスを得る。
fn project_of(env_file: &Path) -> (String, String) {
    let dir = env_file.parent().unwrap_or_else(|| Path::new("."));
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_string_lossy().into_owned());
    (name, dir.to_string_lossy().into_owned())
}

/// 既存キーとの関係。
enum MatchStatus {
    /// 同じ値のキーが既にある(登録不要、紐づけのみ)
    Same { label: String, key_id: i64 },
    /// 同じ service + env_var_name で値が異なるキーがある(手動確認が必要)
    Conflict { label: String },
    /// vault に存在しない
    New,
}

fn match_existing(
    conn: &Connection,
    store: &dyn SecretStore,
    service: &str,
    var: &str,
    value: &str,
) -> Result<MatchStatus> {
    let mut stmt = conn.prepare(
        "SELECT k.id, s.name, k.name, k.env_var_name
         FROM keys k JOIN services s ON s.id = k.service_id
         WHERE s.name = ?1 OR k.env_var_name = ?2",
    )?;
    let rows: Vec<(i64, String, String, String)> = stmt
        .query_map(params![service, var], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut conflict = None;
    for (key_id, row_service, name, env_var) in rows {
        let label = format!("{row_service}/{name}");
        if store.get(key_id)?.as_deref() == Some(value) {
            return Ok(MatchStatus::Same { label, key_id });
        }
        if env_var == var && row_service == service {
            conflict = Some(label);
        }
    }
    Ok(match conflict {
        Some(label) => MatchStatus::Conflict { label },
        None => MatchStatus::New,
    })
}

fn get_or_create_service(conn: &Connection, name: &str) -> Result<i64> {
    let existing: Option<i64> = conn
        .query_row("SELECT id FROM services WHERE name = ?1", [name], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    conn.execute("INSERT INTO services (name) VALUES (?1)", [name])?;
    Ok(conn.last_insert_rowid())
}

/// service 内で重複しないキー名を返す(base, base-2, base-3, ...)。
fn unique_key_name(conn: &Connection, service_id: i64, base: &str) -> Result<String> {
    let mut name = base.to_string();
    let mut n = 2;
    loop {
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM keys WHERE service_id = ?1 AND name = ?2",
                params![service_id, &name],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Ok(name);
        }
        name = format!("{base}-{n}");
        n += 1;
    }
}

fn assign_project(conn: &Connection, key_id: i64, project: &str, path: &str) -> Result<()> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM projects WHERE name = ?1",
            [project],
            |row| row.get(0),
        )
        .optional()?;
    let project_id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE projects SET path = ?1 WHERE id = ?2",
                params![path, id],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO projects (name, path) VALUES (?1, ?2)",
                params![project, path],
            )?;
            conn.last_insert_rowid()
        }
    };
    conn.execute(
        "INSERT OR IGNORE INTO key_usages (key_id, project_id) VALUES (?1, ?2)",
        params![key_id, project_id],
    )?;
    Ok(())
}

impl Vault {
    /// path 以下の .env をスキャンし、取り込み候補を返す(値はマスクのみ)。
    pub fn scan_preview(&self, path: &str, max_depth: u32) -> Result<ScanReport> {
        let root = Path::new(path);
        if !root.is_dir() {
            return Err(Error::InvalidInput(format!(
                "ディレクトリが存在しません: {path}"
            )));
        }
        let env_files = find_env_files(root, max_depth);
        let mut summary: HashMap<&'static str, usize> = HashMap::new();
        let mut files = Vec::new();
        for env_file in &env_files {
            let (project, _) = project_of(env_file);
            let text = std::fs::read_to_string(env_file).unwrap_or_default();
            let mut entries = Vec::new();
            for (var, value) in parse_env(&text) {
                let entry = if !looks_like_secret(&var, &value) {
                    ScanEntry {
                        var,
                        status: "skipped",
                        service: None,
                        value_masked: None,
                        existing_key: None,
                    }
                } else {
                    let service = infer_service(&var, &value);
                    let (status, existing_key) = match match_existing(
                        &self.conn,
                        self.store.as_ref(),
                        &service,
                        &var,
                        &value,
                    )? {
                        MatchStatus::Same { label, .. } => ("exists", Some(label)),
                        MatchStatus::Conflict { label } => ("conflict", Some(label)),
                        MatchStatus::New => ("new", None),
                    };
                    ScanEntry {
                        var,
                        status,
                        service: Some(service),
                        value_masked: Some(mask(&value)),
                        existing_key,
                    }
                };
                *summary.entry(entry.status).or_default() += 1;
                entries.push(entry);
            }
            files.push(ScanFile {
                file: env_file.to_string_lossy().into_owned(),
                project,
                entries,
            });
        }
        Ok(ScanReport {
            files_scanned: env_files.len(),
            summary,
            files,
        })
    }

    /// 選択された変数を .env から読み直して取り込み、プロジェクトに紐づける。
    pub fn scan_apply(&mut self, selections: &[ScanSelection]) -> Result<ScanApplyReport> {
        let Self { conn, store } = self;
        let tx = conn.transaction()?;
        let mut summary: HashMap<&'static str, usize> = HashMap::new();
        let mut results = Vec::new();
        for sel in selections {
            let env_file = Path::new(&sel.file);
            let (project, project_path) = project_of(env_file);
            let text = std::fs::read_to_string(env_file).unwrap_or_default();
            let values: HashMap<String, String> = parse_env(&text).into_iter().collect();
            for var in &sel.vars {
                let entry = match values.get(var) {
                    None => ScanApplyEntry {
                        file: sel.file.clone(),
                        var: var.clone(),
                        status: "missing",
                        key: None,
                        project: None,
                    },
                    Some(value) => {
                        let service = infer_service(var, value);
                        match match_existing(&tx, store.as_ref(), &service, var, value)? {
                            MatchStatus::Same { label, key_id } => {
                                assign_project(&tx, key_id, &project, &project_path)?;
                                ScanApplyEntry {
                                    file: sel.file.clone(),
                                    var: var.clone(),
                                    status: "linked",
                                    key: Some(label),
                                    project: Some(project.clone()),
                                }
                            }
                            MatchStatus::Conflict { label } => ScanApplyEntry {
                                file: sel.file.clone(),
                                var: var.clone(),
                                status: "conflict",
                                key: Some(label),
                                project: None,
                            },
                            MatchStatus::New => {
                                let service_id = get_or_create_service(&tx, &service)?;
                                let key_name = unique_key_name(&tx, service_id, &project)?;
                                tx.execute(
                                    "INSERT INTO keys (service_id, name, env_var_name, note)
                                     VALUES (?1, ?2, ?3, ?4)",
                                    params![
                                        service_id,
                                        key_name,
                                        var,
                                        format!("スキャンで {} から取り込み", sel.file)
                                    ],
                                )?;
                                let key_id = tx.last_insert_rowid();
                                store.set(key_id, value)?;
                                assign_project(&tx, key_id, &project, &project_path)?;
                                ScanApplyEntry {
                                    file: sel.file.clone(),
                                    var: var.clone(),
                                    status: "imported",
                                    key: Some(format!("{service}/{key_name}")),
                                    project: Some(project.clone()),
                                }
                            }
                        }
                    }
                };
                *summary.entry(entry.status).or_default() += 1;
                results.push(entry);
            }
        }
        tx.commit()?;
        Ok(ScanApplyReport { summary, results })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::secrets::memory::MemoryStore;
    use tempfile::TempDir;

    fn test_vault() -> (Vault, MemoryStore, TempDir) {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::default();
        let vault = Vault::open(&dir.path().join("vault.db"), Box::new(store.clone())).unwrap();
        (vault, store, dir)
    }

    fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn parse_env_handles_quotes_exports_and_comments() {
        let text = "\
# comment
export ANTHROPIC_API_KEY=sk-ant-abc
QUOTED=\"quoted value\"
SINGLE='single'
TRAILING=value # comment
EMPTY=
1BAD=value
NOEQUAL
";
        let entries = parse_env(text);
        assert_eq!(
            entries,
            vec![
                ("ANTHROPIC_API_KEY".into(), "sk-ant-abc".into()),
                ("QUOTED".into(), "quoted value".into()),
                ("SINGLE".into(), "single".into()),
                ("TRAILING".into(), "value".into()),
            ]
        );
    }

    #[test]
    fn find_env_files_skips_templates_and_dirs() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), ".env", "A=1\n");
        write(dir.path(), ".env.local", "B=2\n");
        write(dir.path(), ".env.example", "C=3\n");
        write(dir.path(), "app/.env", "D=4\n");
        write(dir.path(), "node_modules/pkg/.env", "E=5\n");
        let found = find_env_files(dir.path(), 3);
        let names: Vec<String> = found
            .iter()
            .map(|p| {
                p.strip_prefix(dir.path())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(names, vec![".env", ".env.local", "app/.env"]);
    }

    #[test]
    fn find_env_files_respects_max_depth() {
        let dir = TempDir::new().unwrap();
        write(dir.path(), "a/.env", "A=1\n");
        write(dir.path(), "a/b/.env", "B=2\n");
        assert_eq!(find_env_files(dir.path(), 1).len(), 1);
        assert_eq!(find_env_files(dir.path(), 2).len(), 2);
    }

    #[test]
    fn infer_service_uses_presets_then_fallback() {
        assert_eq!(infer_service("CLAUDE_API_KEY", "xxx"), "anthropic");
        assert_eq!(infer_service("MY_VAR", "sk_live_xxx"), "stripe");
        assert_eq!(infer_service("MYAPP_API_KEY", "xxx"), "myapp");
        assert_eq!(infer_service("X_TOKEN", "xxx"), "unknown");
    }

    #[test]
    fn preview_reports_new_exists_conflict_and_skipped() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "anthropic",
                "default",
                "sk-ant-same-value",
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_old-value",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();

        let project = TempDir::new().unwrap();
        write(
            project.path(),
            "my-app/.env",
            "ANTHROPIC_API_KEY=sk-ant-same-value\n\
             STRIPE_SECRET_KEY=sk_live_new-value\n\
             RESEND_API_KEY=re_123456789012\n\
             PORT=3000\n",
        );

        let report = vault
            .scan_preview(project.path().to_str().unwrap(), 3)
            .unwrap();
        assert_eq!(report.files_scanned, 1);
        let entries = &report.files[0].entries;
        assert_eq!(report.files[0].project, "my-app");

        let by_var = |v: &str| entries.iter().find(|e| e.var == v).unwrap();
        assert_eq!(by_var("ANTHROPIC_API_KEY").status, "exists");
        assert_eq!(
            by_var("ANTHROPIC_API_KEY").existing_key.as_deref(),
            Some("anthropic/default")
        );
        assert_eq!(by_var("STRIPE_SECRET_KEY").status, "conflict");
        assert_eq!(by_var("RESEND_API_KEY").status, "new");
        assert_eq!(by_var("PORT").status, "skipped");
        // 生の値が漏れていないこと
        let json = serde_json::to_string(&report).unwrap();
        assert!(!json.contains("sk_live_new-value"));
        assert!(!json.contains("sk-ant-same-value"));
    }

    #[test]
    fn apply_imports_links_and_reports_missing() {
        let (mut vault, store, _dir) = test_vault();
        vault
            .set_key(
                "anthropic",
                "default",
                "sk-ant-same-value",
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();

        let project = TempDir::new().unwrap();
        let env_file = write(
            project.path(),
            "my-app/.env",
            "ANTHROPIC_API_KEY=sk-ant-same-value\nRESEND_API_KEY=re_123456789012\n",
        );

        let report = vault
            .scan_apply(&[ScanSelection {
                file: env_file.to_string_lossy().into_owned(),
                vars: vec![
                    "ANTHROPIC_API_KEY".into(),
                    "RESEND_API_KEY".into(),
                    "GONE_KEY".into(),
                ],
            }])
            .unwrap();

        let by_var = |v: &str| report.results.iter().find(|e| e.var == v).unwrap();
        assert_eq!(by_var("ANTHROPIC_API_KEY").status, "linked");
        assert_eq!(by_var("RESEND_API_KEY").status, "imported");
        assert_eq!(
            by_var("RESEND_API_KEY").key.as_deref(),
            Some("resend/my-app")
        );
        assert_eq!(by_var("GONE_KEY").status, "missing");

        // 取り込んだキーがセキュアストレージとプロジェクトに反映されていること
        let keys = vault.list_keys(Some("resend")).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].env_var_name, "RESEND_API_KEY");
        assert_eq!(
            store.get(keys[0].key_id).unwrap().as_deref(),
            Some("re_123456789012")
        );
        let projects = vault.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-app");
        assert_eq!(projects[0].keys.len(), 2);
    }

    #[test]
    fn apply_generates_unique_key_names() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "resend",
                "my-app",
                "re_aaaaaaaaaaaa",
                "RESEND_KEY_OLD",
                None,
                None,
            )
            .unwrap();

        let project = TempDir::new().unwrap();
        let env_file = write(
            project.path(),
            "my-app/.env",
            "RESEND_API_KEY=re_123456789012\n",
        );
        let report = vault
            .scan_apply(&[ScanSelection {
                file: env_file.to_string_lossy().into_owned(),
                vars: vec!["RESEND_API_KEY".into()],
            }])
            .unwrap();
        assert_eq!(report.results[0].key.as_deref(), Some("resend/my-app-2"));
    }

    #[test]
    fn preview_rejects_missing_directory() {
        let (vault, _store, _dir) = test_vault();
        assert!(matches!(
            vault.scan_preview("C:\\no\\such\\dir-keynest-test", 3),
            Err(Error::InvalidInput(_))
        ));
    }
}
