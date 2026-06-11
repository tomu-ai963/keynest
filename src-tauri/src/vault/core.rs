//! vault の高レベル API。Tauri コマンド層はこのモジュールだけを呼ぶ。

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::secrets::SecretStore;
use super::{db, envfile};
use crate::error::{Error, Result};

pub struct Vault {
    pub(super) conn: Connection,
    pub(super) store: Box<dyn SecretStore>,
}

#[derive(Debug, Serialize)]
pub struct SetKeyResult {
    pub action: &'static str, // "created" | "updated"
    pub key_id: i64,
    pub service: String,
    pub name: String,
    pub env_var_name: String,
    pub value_masked: String,
}

#[derive(Debug, Serialize)]
pub struct KeyInfo {
    pub key_id: i64,
    pub service: String,
    pub name: String,
    pub env_var_name: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub last_validated_at: Option<String>,
    pub note: Option<String>,
    pub used_by: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ProjectKey {
    pub key_id: i64,
    pub service: String,
    pub name: String,
    pub env_var_name: String,
}

#[derive(Debug, Serialize)]
pub struct ProjectInfo {
    pub project_id: i64,
    pub name: String,
    pub path: Option<String>,
    pub keys: Vec<ProjectKey>,
}

#[derive(Debug, Serialize)]
pub struct AssignResult {
    pub key: String,
    pub env_var_name: String,
    pub project: String,
}

#[derive(Debug, Serialize)]
pub struct InjectResult {
    pub project: String,
    pub env_path: String,
    pub written: Vec<String>,
}

struct FoundKey {
    id: i64,
    service: String,
    name: String,
    env_var_name: String,
}

impl FoundKey {
    fn label(&self) -> String {
        format!("{}/{}", self.service, self.name)
    }
}

/// 値のマスク表示。12文字以上なら先頭4+末尾4、未満なら全て伏せる。
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() >= 12 {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}…{tail}")
    } else {
        "*".repeat(chars.len())
    }
}

fn normalize_service(service: &str) -> String {
    service.trim().to_lowercase()
}

impl Vault {
    pub fn open(db_path: &Path, store: Box<dyn SecretStore>) -> Result<Self> {
        Ok(Self {
            conn: db::open(db_path)?,
            store,
        })
    }

    fn get_or_create_service(&self, name: &str) -> Result<i64> {
        let name = normalize_service(name);
        let existing: Option<i64> = self
            .conn
            .query_row("SELECT id FROM services WHERE name = ?1", [&name], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(id) = existing {
            return Ok(id);
        }
        self.conn
            .execute("INSERT INTO services (name) VALUES (?1)", [&name])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// service(+ name)でキーを 1 件特定する。
    /// name 省略時はそのサービスのキーが 1 件だけなら返し、複数なら Ambiguous。
    fn find_key(&self, service: &str, name: Option<&str>) -> Result<FoundKey> {
        let service_norm = normalize_service(service);
        let mut stmt = self.conn.prepare(
            "SELECT k.id, s.name, k.name, k.env_var_name
             FROM keys k JOIN services s ON s.id = k.service_id
             WHERE s.name = ?1",
        )?;
        let rows: Vec<FoundKey> = stmt
            .query_map([&service_norm], |row| {
                Ok(FoundKey {
                    id: row.get(0)?,
                    service: row.get(1)?,
                    name: row.get(2)?,
                    env_var_name: row.get(3)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;

        let not_found =
            || Error::NotFound(format!("{}/{}", service_norm, name.unwrap_or("(省略)")));
        match name {
            Some(n) => rows.into_iter().find(|k| k.name == n).ok_or_else(not_found),
            None => match rows.len() {
                1 => Ok(rows.into_iter().next().unwrap()),
                0 => Err(not_found()),
                _ => {
                    let names: Vec<String> = rows.into_iter().map(|k| k.name).collect();
                    Err(Error::Ambiguous(format!(
                        "サービス {} にはキーが複数あります。name を指定してください: {}",
                        service_norm,
                        names.join(", ")
                    )))
                }
            },
        }
    }

    /// キーを登録または更新する(同じ service + name は上書き)。
    pub fn set_key(
        &mut self,
        service: &str,
        name: &str,
        value: &str,
        env_var_name: &str,
        expires_at: Option<&str>,
        note: Option<&str>,
    ) -> Result<SetKeyResult> {
        if value.is_empty() {
            return Err(Error::InvalidInput("value が空です".into()));
        }
        if name.trim().is_empty() {
            return Err(Error::InvalidInput("name が空です".into()));
        }
        if env_var_name.trim().is_empty() {
            return Err(Error::InvalidInput("env_var_name が空です".into()));
        }
        let service_id = self.get_or_create_service(service)?;

        let Self { conn, store } = self;
        let tx = conn.transaction()?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT id FROM keys WHERE service_id = ?1 AND name = ?2",
                params![service_id, name],
                |row| row.get(0),
            )
            .optional()?;
        let (key_id, action) = match existing {
            Some(key_id) => {
                tx.execute(
                    "UPDATE keys SET env_var_name = ?1, expires_at = ?2, note = ?3,
                                     last_validated_at = NULL
                     WHERE id = ?4",
                    params![env_var_name, expires_at, note, key_id],
                )?;
                (key_id, "updated")
            }
            None => {
                tx.execute(
                    "INSERT INTO keys (service_id, name, env_var_name, expires_at, note)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![service_id, name, env_var_name, expires_at, note],
                )?;
                (tx.last_insert_rowid(), "created")
            }
        };
        store.set(key_id, value)?;
        tx.commit()?;

        Ok(SetKeyResult {
            action,
            key_id,
            service: normalize_service(service),
            name: name.to_string(),
            env_var_name: env_var_name.to_string(),
            value_masked: mask(value),
        })
    }

    /// キー一覧を返す。値そのものは一切読み込まない(メタデータのみ)。
    pub fn list_keys(&self, service: Option<&str>) -> Result<Vec<KeyInfo>> {
        let mut sql = String::from(
            "SELECT k.id, s.name, k.name, k.env_var_name,
                    k.created_at, k.expires_at, k.last_validated_at, k.note
             FROM keys k JOIN services s ON s.id = k.service_id",
        );
        let service_norm = service.map(normalize_service);
        if service_norm.is_some() {
            sql.push_str(" WHERE s.name = ?1");
        }
        sql.push_str(" ORDER BY s.name, k.name");

        let mut stmt = self.conn.prepare(&sql)?;
        let map_row = |row: &rusqlite::Row| -> rusqlite::Result<KeyInfo> {
            Ok(KeyInfo {
                key_id: row.get(0)?,
                service: row.get(1)?,
                name: row.get(2)?,
                env_var_name: row.get(3)?,
                created_at: row.get(4)?,
                expires_at: row.get(5)?,
                last_validated_at: row.get(6)?,
                note: row.get(7)?,
                used_by: Vec::new(),
            })
        };
        let mut keys: Vec<KeyInfo> = match &service_norm {
            Some(s) => stmt
                .query_map([s], map_row)?
                .collect::<rusqlite::Result<_>>()?,
            None => stmt
                .query_map([], map_row)?
                .collect::<rusqlite::Result<_>>()?,
        };

        let mut usage_stmt = self.conn.prepare(
            "SELECT p.name FROM key_usages ku JOIN projects p ON p.id = ku.project_id
             WHERE ku.key_id = ?1 ORDER BY p.name",
        )?;
        for key in &mut keys {
            key.used_by = usage_stmt
                .query_map([key.key_id], |row| row.get(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(keys)
    }

    /// キーをメタデータ・セキュアストレージの両方から削除する。
    /// 返り値は削除したキーのラベル("service/name")。
    pub fn delete_key(&mut self, service: &str, name: Option<&str>) -> Result<String> {
        let key = self.find_key(service, name)?;
        self.store.delete(key.id)?;
        self.conn
            .execute("DELETE FROM keys WHERE id = ?1", [key.id])?;
        Ok(key.label())
    }

    fn get_or_create_project(&self, name: &str, path: Option<&str>) -> Result<i64> {
        let existing: Option<(i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT id, path FROM projects WHERE name = ?1",
                [name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        match existing {
            Some((id, _)) => {
                if path.is_some() {
                    self.conn.execute(
                        "UPDATE projects SET path = ?1 WHERE id = ?2",
                        params![path, id],
                    )?;
                }
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO projects (name, path) VALUES (?1, ?2)",
                    params![name, path],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    /// キーをプロジェクトに紐づける。プロジェクトが未登録なら作成する。
    pub fn assign_key(
        &mut self,
        service: &str,
        name: Option<&str>,
        project: &str,
        project_path: Option<&str>,
    ) -> Result<AssignResult> {
        if project.trim().is_empty() {
            return Err(Error::InvalidInput("project が空です".into()));
        }
        let key = self.find_key(service, name)?;
        let project_id = self.get_or_create_project(project, project_path)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO key_usages (key_id, project_id) VALUES (?1, ?2)",
            params![key.id, project_id],
        )?;
        Ok(AssignResult {
            key: key.label(),
            env_var_name: key.env_var_name,
            project: project.to_string(),
        })
    }

    /// キーとプロジェクトの紐づけを解除する。
    pub fn unassign_key(
        &mut self,
        service: &str,
        name: Option<&str>,
        project: &str,
    ) -> Result<String> {
        let key = self.find_key(service, name)?;
        let changed = self.conn.execute(
            "DELETE FROM key_usages WHERE key_id = ?1
             AND project_id = (SELECT id FROM projects WHERE name = ?2)",
            params![key.id, project],
        )?;
        if changed == 0 {
            return Err(Error::InvalidInput(format!(
                "{} は {} に紐づいていません",
                key.label(),
                project
            )));
        }
        Ok(key.label())
    }

    /// プロジェクト一覧(紐づくキーのメタデータ付き)を返す。
    pub fn list_projects(&self) -> Result<Vec<ProjectInfo>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, path FROM projects ORDER BY name")?;
        let mut projects: Vec<ProjectInfo> = stmt
            .query_map([], |row| {
                Ok(ProjectInfo {
                    project_id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    keys: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut key_stmt = self.conn.prepare(
            "SELECT k.id, s.name, k.name, k.env_var_name
             FROM key_usages ku
             JOIN keys k ON k.id = ku.key_id
             JOIN services s ON s.id = k.service_id
             WHERE ku.project_id = ?1
             ORDER BY s.name, k.name",
        )?;
        for project in &mut projects {
            project.keys = key_stmt
                .query_map([project.project_id], |row| {
                    Ok(ProjectKey {
                        key_id: row.get(0)?,
                        service: row.get(1)?,
                        name: row.get(2)?,
                        env_var_name: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(projects)
    }

    /// プロジェクトを削除する(キー本体は残る。紐づけのみ CASCADE で消える)。
    pub fn delete_project(&mut self, project: &str) -> Result<()> {
        let changed = self
            .conn
            .execute("DELETE FROM projects WHERE name = ?1", [project])?;
        if changed == 0 {
            return Err(Error::NotFound(format!("プロジェクト: {project}")));
        }
        Ok(())
    }

    /// プロジェクトに紐づくキー一式を、プロジェクトパス直下の .env に書き出す。
    /// 返り値に生の値は含めない(書き出した変数名のみ)。
    pub fn inject(&mut self, project: &str, path: Option<&str>) -> Result<InjectResult> {
        let row: Option<(i64, Option<String>)> = self
            .conn
            .query_row(
                "SELECT id, path FROM projects WHERE name = ?1",
                [project],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((project_id, stored_path)) = row else {
            return Err(Error::NotFound(format!("プロジェクト: {project}")));
        };
        if let Some(p) = path {
            self.conn.execute(
                "UPDATE projects SET path = ?1 WHERE id = ?2",
                params![p, project_id],
            )?;
        }
        let Some(target) = path.map(String::from).or(stored_path) else {
            return Err(Error::InvalidInput(format!(
                "プロジェクト {project} のパスが未設定です。path を指定してください"
            )));
        };
        let target_dir = Path::new(&target);
        if !target_dir.is_dir() {
            return Err(Error::InvalidInput(format!(
                "ディレクトリが存在しません: {target}"
            )));
        }

        let mut stmt = self.conn.prepare(
            "SELECT k.id, k.env_var_name, s.name, k.name
             FROM key_usages ku
             JOIN keys k ON k.id = ku.key_id
             JOIN services s ON s.id = k.service_id
             WHERE ku.project_id = ?1
             ORDER BY s.name, k.name",
        )?;
        let keys: Vec<(i64, String, String, String)> = stmt
            .query_map([project_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        if keys.is_empty() {
            return Err(Error::InvalidInput(format!(
                "プロジェクト {project} に紐づくキーがありません"
            )));
        }

        let mut updates: HashMap<String, String> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();
        for (key_id, env_var_name, service, name) in keys {
            match self.store.get(key_id)? {
                Some(value) => {
                    updates.insert(env_var_name, value);
                }
                None => missing.push(format!("{service}/{name}")),
            }
        }
        if !missing.is_empty() {
            return Err(Error::InvalidInput(format!(
                "セキュアストレージに値が見つからないキーがあります: {}",
                missing.join(", ")
            )));
        }

        let env_path = target_dir.join(".env");
        envfile::merge(&env_path, &updates)?;
        let mut written: Vec<String> = updates.into_keys().collect();
        written.sort();
        Ok(InjectResult {
            project: project.to_string(),
            env_path: env_path.to_string_lossy().into_owned(),
            written,
        })
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

    #[test]
    fn mask_long_and_short_values() {
        assert_eq!(mask("sk-ant-api03-abcdef3f2x"), "sk-a…3f2x");
        assert_eq!(mask("short"), "*****");
        assert_eq!(mask(""), "");
    }

    #[test]
    fn set_key_creates_and_stores_secret() {
        let (mut vault, store, _dir) = test_vault();
        let result = vault
            .set_key(
                " Anthropic ",
                "default",
                "sk-ant-api03-secret-value",
                "ANTHROPIC_API_KEY",
                None,
                Some("メインキー"),
            )
            .unwrap();
        assert_eq!(result.action, "created");
        assert_eq!(result.service, "anthropic");
        assert!(!result.value_masked.contains("secret"));
        assert_eq!(
            store.get(result.key_id).unwrap().as_deref(),
            Some("sk-ant-api03-secret-value")
        );

        let keys = vault.list_keys(None).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].service, "anthropic");
        assert_eq!(keys[0].env_var_name, "ANTHROPIC_API_KEY");
        assert_eq!(keys[0].note.as_deref(), Some("メインキー"));
    }

    #[test]
    fn set_key_overwrites_same_service_and_name() {
        let (mut vault, store, _dir) = test_vault();
        let first = vault
            .set_key(
                "openai",
                "default",
                "sk-proj-original-value",
                "OPENAI_API_KEY",
                None,
                None,
            )
            .unwrap();
        let second = vault
            .set_key(
                "openai",
                "default",
                "sk-proj-rotated-value!",
                "OPENAI_KEY",
                None,
                None,
            )
            .unwrap();
        assert_eq!(second.action, "updated");
        assert_eq!(second.key_id, first.key_id);
        assert_eq!(
            store.get(first.key_id).unwrap().as_deref(),
            Some("sk-proj-rotated-value!")
        );

        let keys = vault.list_keys(Some("openai")).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].env_var_name, "OPENAI_KEY");
    }

    #[test]
    fn set_key_rejects_empty_value() {
        let (mut vault, _store, _dir) = test_vault();
        let err = vault
            .set_key("openai", "default", "", "OPENAI_API_KEY", None, None)
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn list_keys_filters_by_service() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "anthropic",
                "default",
                "sk-ant-aaaaaaaa",
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_bbbbbbbb",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        assert_eq!(vault.list_keys(None).unwrap().len(), 2);
        let filtered = vault.list_keys(Some("stripe")).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].service, "stripe");
    }

    #[test]
    fn delete_key_removes_metadata_and_secret() {
        let (mut vault, store, _dir) = test_vault();
        let result = vault
            .set_key(
                "resend",
                "default",
                "re_123456789012",
                "RESEND_API_KEY",
                None,
                None,
            )
            .unwrap();
        let deleted = vault.delete_key("resend", None).unwrap();
        assert_eq!(deleted, "resend/default");
        assert!(vault.list_keys(None).unwrap().is_empty());
        assert_eq!(store.get(result.key_id).unwrap(), None);
    }

    #[test]
    fn assign_and_unassign_key_to_project() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "anthropic",
                "default",
                "sk-ant-aaaaaaaa",
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        let result = vault
            .assign_key("anthropic", None, "my-app", Some("C:\\projects\\my-app"))
            .unwrap();
        assert_eq!(result.key, "anthropic/default");
        assert_eq!(result.env_var_name, "ANTHROPIC_API_KEY");

        let projects = vault.list_projects().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "my-app");
        assert_eq!(projects[0].path.as_deref(), Some("C:\\projects\\my-app"));
        assert_eq!(projects[0].keys.len(), 1);

        let keys = vault.list_keys(None).unwrap();
        assert_eq!(keys[0].used_by, vec!["my-app"]);

        vault.unassign_key("anthropic", None, "my-app").unwrap();
        assert!(vault.list_projects().unwrap()[0].keys.is_empty());
        let err = vault.unassign_key("anthropic", None, "my-app").unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
    }

    #[test]
    fn delete_project_keeps_keys() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_aaaaaaaa",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        vault.assign_key("stripe", None, "shop", None).unwrap();
        vault.delete_project("shop").unwrap();
        assert!(vault.list_projects().unwrap().is_empty());
        assert_eq!(vault.list_keys(None).unwrap().len(), 1);
        assert!(matches!(
            vault.delete_project("shop").unwrap_err(),
            Error::NotFound(_)
        ));
    }

    #[test]
    fn inject_writes_env_preserving_existing_content() {
        let (mut vault, _store, _dir) = test_vault();
        let project_dir = TempDir::new().unwrap();
        let env_path = project_dir.path().join(".env");
        std::fs::write(
            &env_path,
            "# app config\nPORT=3000\nANTHROPIC_API_KEY=stale\n",
        )
        .unwrap();

        vault
            .set_key(
                "anthropic",
                "default",
                "sk-ant-fresh-value",
                "ANTHROPIC_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault
            .set_key(
                "resend",
                "default",
                "re_123456789012",
                "RESEND_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault
            .assign_key("anthropic", None, "my-app", project_dir.path().to_str())
            .unwrap();
        vault.assign_key("resend", None, "my-app", None).unwrap();

        let result = vault.inject("my-app", None).unwrap();
        assert_eq!(result.written, vec!["ANTHROPIC_API_KEY", "RESEND_API_KEY"]);
        let content = std::fs::read_to_string(&env_path).unwrap();
        assert_eq!(
            content,
            "# app config\nPORT=3000\nANTHROPIC_API_KEY=sk-ant-fresh-value\nRESEND_API_KEY=re_123456789012\n"
        );
    }

    #[test]
    fn inject_fails_without_path_or_keys() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "openai",
                "default",
                "sk-proj-aaaaaaaa",
                "OPENAI_API_KEY",
                None,
                None,
            )
            .unwrap();
        vault.assign_key("openai", None, "no-path", None).unwrap();
        assert!(matches!(
            vault.inject("no-path", None).unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            vault.inject("unknown-project", None).unwrap_err(),
            Error::NotFound(_)
        ));
    }

    #[test]
    fn delete_without_name_is_ambiguous_when_multiple_keys() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "stripe",
                "live",
                "sk_live_aaaaaaaa",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        vault
            .set_key(
                "stripe",
                "test",
                "sk_test_bbbbbbbb",
                "STRIPE_TEST_KEY",
                None,
                None,
            )
            .unwrap();
        let err = vault.delete_key("stripe", None).unwrap_err();
        assert!(matches!(err, Error::Ambiguous(_)));
        let err = vault.delete_key("stripe", Some("missing")).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }
}
