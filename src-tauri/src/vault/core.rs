//! vault の高レベル API。Tauri コマンド層はこのモジュールだけを呼ぶ。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::secrets::SecretStore;
use super::{db, envfile, validate};
use crate::error::{Error, Result};

pub struct Vault {
    pub(super) conn: Connection,
    pub(super) store: Box<dyn SecretStore>,
    /// scan_preview で検出した .env の絶対パス集合。
    /// scan_apply はこの集合に含まれるパスしか受理しない(任意パス読み取りの防止)。
    pub(super) scanned_paths: HashSet<String>,
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
    /// このサービスの検証(validate_key)に対応しているか
    pub validatable: bool,
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

#[derive(Debug, Serialize)]
pub struct ValidateResult {
    pub key: String,
    pub ok: bool,
    pub detail: String,
    pub last_validated_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RotateResult {
    pub key: String,
    pub env_var_name: String,
    pub value_masked: String,
    pub old_value_masked: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RotationInfo {
    pub rotated_at: String,
    pub old_value_masked: String,
}

/// Vault::project_env の返り値。生の値を含むため Serialize を実装せず、crate 外にも出さない。
pub(crate) struct ProjectEnv {
    pub(crate) dir: PathBuf,
    pub(crate) vars: HashMap<String, String>,
}

/// キー検証の途中状態。begin_validation で作り、run でネットワーク検証を行い、
/// finish_validation で結果を記録する。run は Vault のロックを持たずに呼べる。
/// フィールドは非公開: begin_validation を経由せずに finish_validation へ渡せる値を
/// 作れないようにし、生のキー値もコマンド層へ露出させない。
pub struct PendingValidation {
    key_id: i64,
    label: String,
    service: String,
    value: String,
}

impl PendingValidation {
    /// プロバイダ API を叩いてキーの生死を確認する(タイムアウトまで最大15秒)。
    pub fn run(&self) -> Result<validate::Validation> {
        validate::validate(&self.service, &self.value)
    }
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

/// 値のマスク表示。
/// - 20文字以上のときだけ末尾4文字を見せる(それ未満は全マスク)。
/// - 先頭に露出してよいのは公知のプレフィックス(sk-ant- 等)だけ。
///   任意のキーの先頭数文字は決して露出しない。
///
/// このマスク文字列はローテーション履歴として SQLite に永続化されるため、
/// 露出範囲は「公開情報のプレフィックス + 末尾4文字」に厳しく限定する。
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 20 {
        return "*".repeat(chars.len());
    }
    let tail: String = chars[chars.len() - 4..].iter().collect();
    match super::presets::known_prefix(value) {
        Some(prefix) => format!("{prefix}…{tail}"),
        None => format!("…{tail}"),
    }
}

fn normalize_service(service: &str) -> String {
    service.trim().to_lowercase()
}

/// expires_at の形式検証。DB は TEXT のため、ここで弾かないと不正値がそのまま入る。
/// 受理する形式:
/// - "YYYY-MM-DD"(暦として妥当な日付)
/// - 上記 + "T" または " " 区切りの "HH:MM[:SS[.fff]]"、末尾に "Z" か "±HH:MM" も可
fn validate_expires_at(value: &str) -> Result<()> {
    fn err(value: &str) -> Error {
        Error::InvalidInput(format!(
            "expires_at は YYYY-MM-DD(または ISO 8601 日時)形式で指定してください: {value:?}"
        ))
    }
    let digits = |s: &str| -> Option<u32> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };

    // 日付部 "YYYY-MM-DD"
    let (Some(date), rest) = (value.get(..10), value.get(10..).unwrap_or("")) else {
        return Err(err(value));
    };
    let mut parts = date.split('-');
    let (Some(year), Some(month), Some(day), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(err(value));
    };
    let (Some(year), Some(month), Some(day)) = (
        (year.len() == 4).then(|| digits(year)).flatten(),
        (month.len() == 2).then(|| digits(month)).flatten(),
        (day.len() == 2).then(|| digits(day)).flatten(),
    ) else {
        return Err(err(value));
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(err(value)),
    };
    if day == 0 || day > days_in_month {
        return Err(err(value));
    }

    // 時刻部(任意)
    if rest.is_empty() {
        return Ok(());
    }
    let Some(time) = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' ')) else {
        return Err(err(value));
    };
    // タイムゾーン部を切り離す("Z" / "+HH:MM" / "-HH:MM")
    let time = if let Some(t) = time.strip_suffix('Z') {
        t
    } else if let Some(pos) = time.rfind(['+', '-']) {
        let (t, tz) = time.split_at(pos);
        let (Some(h), Some(m)) = (
            tz.get(1..3).and_then(digits),
            (tz.len() == 6 && tz.as_bytes()[3] == b':')
                .then(|| tz.get(4..6).and_then(digits))
                .flatten(),
        ) else {
            return Err(err(value));
        };
        if h > 23 || m > 59 {
            return Err(err(value));
        }
        t
    } else {
        time
    };
    // "HH:MM[:SS[.fff]]"
    let mut parts = time.split(':');
    let (Some(hour), Some(minute), sec, None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(err(value));
    };
    let (Some(hour), Some(minute)) = (
        (hour.len() == 2).then(|| digits(hour)).flatten(),
        (minute.len() == 2).then(|| digits(minute)).flatten(),
    ) else {
        return Err(err(value));
    };
    if hour > 23 || minute > 59 {
        return Err(err(value));
    }
    if let Some(sec) = sec {
        let (sec, frac) = sec.split_once('.').unwrap_or((sec, "0"));
        let (Some(sec), Some(_)) = (
            (sec.len() == 2).then(|| digits(sec)).flatten(),
            digits(frac),
        ) else {
            return Err(err(value));
        };
        if sec > 59 {
            return Err(err(value));
        }
    }
    Ok(())
}

impl Vault {
    pub fn open(db_path: &Path, store: Box<dyn SecretStore>) -> Result<Self> {
        Ok(Self {
            conn: db::open(db_path)?,
            store,
            scanned_paths: HashSet::new(),
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
        if let Some(expires) = expires_at {
            validate_expires_at(expires)?;
        }
        let service_id = self.get_or_create_service(service)?;

        let Self { conn, store, .. } = self;
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
                // 上書きで旧値が失われる前に、マスクをローテーション履歴へ残す
                // (rotate_key と同じ扱い。値が変わらない場合は記録しない)。
                if let Some(old_value) = store.get(key_id)? {
                    if old_value != value {
                        tx.execute(
                            "INSERT INTO key_rotations (key_id, old_value_masked) VALUES (?1, ?2)",
                            params![key_id, mask(&old_value)],
                        )?;
                    }
                }
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
            let service: String = row.get(1)?;
            Ok(KeyInfo {
                key_id: row.get(0)?,
                validatable: validate::is_supported(&service),
                service,
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

    /// 検証対象キーの特定と生値の取り出しだけを行う(ネットワークには触れない)。
    /// プロバイダ API の呼び出しは最大15秒かかるため、コマンド層はこの後ロックを
    /// 手放してから PendingValidation::run を実行し、finish_validation で結果を記録する。
    pub fn begin_validation(&self, service: &str, name: Option<&str>) -> Result<PendingValidation> {
        let key = self.find_key(service, name)?;
        let Some(value) = self.store.get(key.id)? else {
            return Err(Error::NotFound(format!(
                "セキュアストレージに値がありません: {}",
                key.label()
            )));
        };
        Ok(PendingValidation {
            key_id: key.id,
            label: key.label(),
            service: key.service,
            value,
        })
    }

    /// 検証結果を記録して ValidateResult を組み立てる。
    /// ロック外での検証中にキーが削除されていた場合は NotFound を返す。
    pub fn finish_validation(
        &mut self,
        pending: &PendingValidation,
        outcome: validate::Validation,
    ) -> Result<ValidateResult> {
        // 検証中に rotate_key / set_key で値が差し替わっていた場合、検証したのは
        // 旧値なので last_validated_at は更新しない(新値は未検証のまま残す)。
        let unchanged = self.store.get(pending.key_id)?.as_deref() == Some(&pending.value);
        if outcome.ok && unchanged {
            self.conn.execute(
                "UPDATE keys SET last_validated_at = datetime('now') WHERE id = ?1",
                [pending.key_id],
            )?;
        }
        let last_validated_at: Option<String> = self
            .conn
            .query_row(
                "SELECT last_validated_at FROM keys WHERE id = ?1",
                [pending.key_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(pending.label.clone()))?;
        Ok(ValidateResult {
            key: pending.label.clone(),
            ok: outcome.ok,
            detail: outcome.detail,
            last_validated_at,
        })
    }

    /// キーの値を新しい値に差し替え、旧値のマスクを key_rotations に記録する。
    pub fn rotate_key(
        &mut self,
        service: &str,
        name: Option<&str>,
        new_value: &str,
    ) -> Result<RotateResult> {
        if new_value.is_empty() {
            return Err(Error::InvalidInput("new_value が空です".into()));
        }
        let key = self.find_key(service, name)?;
        let Self { conn, store, .. } = self;
        let old_value = store.get(key.id)?;
        if old_value.as_deref() == Some(new_value) {
            return Err(Error::InvalidInput("新しい値が現在の値と同じです".into()));
        }
        let old_masked = old_value.as_deref().map(mask);
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO key_rotations (key_id, old_value_masked) VALUES (?1, ?2)",
            params![key.id, old_masked.clone().unwrap_or_default()],
        )?;
        tx.execute(
            "UPDATE keys SET last_validated_at = NULL WHERE id = ?1",
            [key.id],
        )?;
        store.set(key.id, new_value)?;
        tx.commit()?;
        Ok(RotateResult {
            key: key.label(),
            env_var_name: key.env_var_name,
            value_masked: mask(new_value),
            old_value_masked: old_masked,
        })
    }

    /// キーのローテーション履歴(新しい順)を返す。
    pub fn list_rotations(&self, service: &str, name: Option<&str>) -> Result<Vec<RotationInfo>> {
        let key = self.find_key(service, name)?;
        let mut stmt = self.conn.prepare(
            "SELECT rotated_at, old_value_masked FROM key_rotations
             WHERE key_id = ?1 ORDER BY rotated_at DESC, id DESC",
        )?;
        let rotations = stmt
            .query_map([key.id], |row| {
                Ok(RotationInfo {
                    rotated_at: row.get(0)?,
                    old_value_masked: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rotations)
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

    fn find_project(&self, project: &str) -> Result<(i64, Option<String>)> {
        self.conn
            .query_row(
                "SELECT id, path FROM projects WHERE name = ?1",
                [project],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("プロジェクト: {project}")))
    }

    /// プロジェクトに紐づくキーの「環境変数名 → 生の値」を読み出す。
    /// 紐づけが空、またはセキュアストレージに値がないキーがあればエラー。
    fn project_values(&self, project_id: i64, project: &str) -> Result<HashMap<String, String>> {
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

        let mut values: HashMap<String, String> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();
        for (key_id, env_var_name, service, name) in keys {
            match self.store.get(key_id)? {
                Some(value) => {
                    values.insert(env_var_name, value);
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
        Ok(values)
    }

    /// MCP の run_with_keys 用: プロジェクトの作業ディレクトリと、
    /// 子プロセスへ注入する環境変数(生の値)を返す。
    /// `only` を渡すとその環境変数名だけに絞る(紐づいていない名前はエラー)。
    /// 生の値を含むため crate 外には公開しない。
    pub(crate) fn project_env(&self, project: &str, only: Option<&[String]>) -> Result<ProjectEnv> {
        let (project_id, stored_path) = self.find_project(project)?;
        let Some(dir) = stored_path else {
            return Err(Error::InvalidInput(format!(
                "プロジェクト {project} のパスが未設定です。KeyNest でパスを設定してください"
            )));
        };
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(Error::InvalidInput(format!(
                "ディレクトリが存在しません: {}",
                dir.display()
            )));
        }
        let mut vars = self.project_values(project_id, project)?;
        if let Some(only) = only {
            let unknown: Vec<&str> = only
                .iter()
                .filter(|v| !vars.contains_key(v.as_str()))
                .map(String::as_str)
                .collect();
            if !unknown.is_empty() {
                return Err(Error::InvalidInput(format!(
                    "プロジェクト {project} に紐づいていない環境変数です: {}",
                    unknown.join(", ")
                )));
            }
            vars.retain(|k, _| only.contains(k));
        }
        Ok(ProjectEnv { dir, vars })
    }

    /// プロジェクトに紐づくキー一式を、プロジェクトパス直下の .env に書き出す。
    /// 返り値に生の値は含めない(書き出した変数名のみ)。
    pub fn inject(&mut self, project: &str, path: Option<&str>) -> Result<InjectResult> {
        let (project_id, stored_path) = self.find_project(project)?;
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

        let updates = self.project_values(project_id, project)?;

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
        // 20文字以上: 公知プレフィックス + 末尾4文字だけ露出
        assert_eq!(mask("sk-ant-api03-abcdef3f2x"), "sk-ant-…3f2x");
        // 公知プレフィックスがない長いキーは末尾4文字のみ
        assert_eq!(mask("abcdefghij0123456789"), "…6789");
        // 20文字未満は全マスク(先頭は一切出さない)
        assert_eq!(mask("re_123456789012"), "***************");
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

        // 上書きで旧値のマスクがローテーション履歴に残る
        let history = vault.list_rotations("openai", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].old_value_masked, mask("sk-proj-original-value"));
    }

    #[test]
    fn set_key_overwrite_with_same_value_records_no_history() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "openai",
                "default",
                "sk-proj-unchanged-value",
                "OPENAI_API_KEY",
                None,
                None,
            )
            .unwrap();
        // 同じ値で保存し直してもメタデータ更新のみ。履歴は増やさない
        vault
            .set_key(
                "openai",
                "default",
                "sk-proj-unchanged-value",
                "OPENAI_API_KEY",
                None,
                Some("メモだけ変更"),
            )
            .unwrap();
        assert!(vault.list_rotations("openai", None).unwrap().is_empty());
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
    fn rotate_key_records_masked_history_and_updates_secret() {
        let (mut vault, store, _dir) = test_vault();
        let result = vault
            .set_key(
                "stripe",
                "default",
                "sk_live_original-value",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        // 検証済みの状態を作る(ネットワークには触れない)
        vault
            .conn
            .execute(
                "UPDATE keys SET last_validated_at = datetime('now') WHERE id = ?1",
                [result.key_id],
            )
            .unwrap();

        let rotated = vault
            .rotate_key("stripe", None, "sk_live_rotated-value!")
            .unwrap();
        assert_eq!(rotated.key, "stripe/default");
        assert!(!rotated.value_masked.contains("rotated-value"));
        assert_eq!(rotated.old_value_masked.as_deref(), Some("sk_live_…alue"));
        assert_eq!(
            store.get(result.key_id).unwrap().as_deref(),
            Some("sk_live_rotated-value!")
        );

        // 履歴に旧値のマスクだけが残り、ローテーション後は未検証に戻る
        let history = vault.list_rotations("stripe", None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].old_value_masked, "sk_live_…alue");
        let keys = vault.list_keys(Some("stripe")).unwrap();
        assert_eq!(keys[0].last_validated_at, None);

        vault
            .rotate_key("stripe", None, "sk_live_third-value!!")
            .unwrap();
        assert_eq!(vault.list_rotations("stripe", None).unwrap().len(), 2);
    }

    #[test]
    fn rotate_key_rejects_empty_or_unchanged_value() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_same-value!!",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        assert!(matches!(
            vault.rotate_key("stripe", None, "").unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(matches!(
            vault
                .rotate_key("stripe", None, "sk_live_same-value!!")
                .unwrap_err(),
            Error::InvalidInput(_)
        ));
        assert!(vault.list_rotations("stripe", None).unwrap().is_empty());
    }

    #[test]
    fn validate_key_rejects_unsupported_service() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "myapp",
                "default",
                "value-1234567890",
                "MYAPP_KEY",
                None,
                None,
            )
            .unwrap();
        // 未対応サービスはネットワークに触れる前(run)に拒否される
        let pending = vault.begin_validation("myapp", None).unwrap();
        assert!(matches!(pending.run().unwrap_err(), Error::InvalidInput(_)));
        // validatable フラグはバリデータ対応表と一致する
        let keys = vault.list_keys(None).unwrap();
        assert!(!keys[0].validatable);
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
        let keys = vault.list_keys(Some("anthropic")).unwrap();
        assert!(keys[0].validatable);
    }

    fn ok_validation() -> validate::Validation {
        validate::Validation {
            ok: true,
            detail: "HTTP 200".into(),
        }
    }

    #[test]
    fn finish_validation_records_timestamp_when_value_unchanged() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_same-value!!",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        let pending = vault.begin_validation("stripe", None).unwrap();
        let result = vault.finish_validation(&pending, ok_validation()).unwrap();
        assert!(result.ok);
        assert_eq!(result.key, "stripe/default");
        assert!(result.last_validated_at.is_some());
    }

    #[test]
    fn finish_validation_skips_timestamp_when_rotated_mid_flight() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_original-value",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        let pending = vault.begin_validation("stripe", None).unwrap();
        // ロック外での検証中にローテーションが走ったケース。
        // 検証したのは旧値なので、新値が検証済み扱いになってはいけない
        vault
            .rotate_key("stripe", None, "sk_live_rotated-value!")
            .unwrap();
        let result = vault.finish_validation(&pending, ok_validation()).unwrap();
        assert!(result.ok);
        assert_eq!(result.last_validated_at, None);
        let keys = vault.list_keys(Some("stripe")).unwrap();
        assert_eq!(keys[0].last_validated_at, None);
    }

    #[test]
    fn finish_validation_reports_not_found_when_deleted_mid_flight() {
        let (mut vault, _store, _dir) = test_vault();
        vault
            .set_key(
                "stripe",
                "default",
                "sk_live_same-value!!",
                "STRIPE_SECRET_KEY",
                None,
                None,
            )
            .unwrap();
        let pending = vault.begin_validation("stripe", None).unwrap();
        vault.delete_key("stripe", None).unwrap();
        let err = vault
            .finish_validation(&pending, ok_validation())
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn validate_expires_at_accepts_dates_and_iso8601_datetimes() {
        for ok in [
            "2026-12-31",
            "2028-02-29", // うるう年
            "2000-02-29", // 400年例外(うるう年)
            "2026-07-22T10:30",
            "2026-07-22 10:30:59",
            "2026-07-22T10:30:00.123",
            "2026-07-22T10:30:00Z",
            "2026-07-22T10:30:00+09:00",
            "2026-07-22T10:30:00.5-05:00",
        ] {
            assert!(validate_expires_at(ok).is_ok(), "{ok} は受理されるべき");
        }
        for bad in [
            "",
            "明日",
            "2026/07/22",
            "2026-13-01",
            "2026-00-10",
            "2026-02-30",
            "2027-02-29", // 平年
            "2100-02-29", // 100年例外(平年)
            "26-07-22",
            "2026-7-2",
            "2026-07-22x",
            "2026-07-22T24:00",
            "2026-07-22T10:60",
            "2026-07-22T10:30:61",
            "2026-07-22T10:30:00+9:00",
            "2026-07-22T10",
        ] {
            assert!(
                matches!(validate_expires_at(bad), Err(Error::InvalidInput(_))),
                "{bad:?} は拒否されるべき"
            );
        }
    }

    #[test]
    fn set_key_rejects_malformed_expires_at() {
        let (mut vault, _store, _dir) = test_vault();
        let err = vault
            .set_key(
                "openai",
                "default",
                "sk-proj-aaaaaaaa",
                "OPENAI_API_KEY",
                Some("来月まで"),
                None,
            )
            .unwrap_err();
        assert!(matches!(err, Error::InvalidInput(_)));
        assert!(err.to_string().contains("expires_at"));
        // 不正値では登録自体が起きない
        assert!(vault.list_keys(None).unwrap().is_empty());

        vault
            .set_key(
                "openai",
                "default",
                "sk-proj-aaaaaaaa",
                "OPENAI_API_KEY",
                Some("2026-12-31"),
                None,
            )
            .unwrap();
        let keys = vault.list_keys(None).unwrap();
        assert_eq!(keys[0].expires_at.as_deref(), Some("2026-12-31"));
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
