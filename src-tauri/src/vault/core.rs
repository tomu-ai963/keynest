//! vault の高レベル API。Tauri コマンド層はこのモジュールだけを呼ぶ。

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::db;
use super::secrets::SecretStore;
use crate::error::{Error, Result};

pub struct Vault {
    conn: Connection,
    store: Box<dyn SecretStore>,
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

struct FoundKey {
    id: i64,
    service: String,
    name: String,
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
            "SELECT k.id, s.name, k.name
             FROM keys k JOIN services s ON s.id = k.service_id
             WHERE s.name = ?1",
        )?;
        let rows: Vec<FoundKey> = stmt
            .query_map([&service_norm], |row| {
                Ok(FoundKey {
                    id: row.get(0)?,
                    service: row.get(1)?,
                    name: row.get(2)?,
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
