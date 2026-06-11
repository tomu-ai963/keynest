//! SQLite メタデータ層。キー本体の値はここには保存しない(secrets.rs が担当)。

use std::path::Path;

use rusqlite::Connection;

use crate::error::Result;

/// 番号順に適用するマイグレーション。適用済みの位置は PRAGMA user_version で管理する。
/// 既存リリースのマイグレーションは変更せず、末尾に追加すること。
const MIGRATIONS: &[&str] = &[include_str!("../../migrations/001_init.sql")];

/// DB を開き、外部キー制約とスキーマを保証したコネクションを返す。
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        conn.execute_batch(&format!("BEGIN;\n{sql}\nCOMMIT;"))?;
        conn.pragma_update(None, "user_version", (i + 1) as i64)?;
    }
    Ok(())
}
