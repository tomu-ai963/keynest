//! vault: KeyNest のバックエンドロジック。
//!
//! 設計原則(tomu-agent の vault/ パッケージから移植):
//! - SQLite にはメタデータのみ。キー本体の値は OS のセキュアストレージに保存する。
//! - 生のキー値は「登録時の入力」と「.env への書き出し」でのみ扱い、
//!   返り値には含めない(マスク表示のみ)。
//! - このモジュールは Tauri に依存しない。コマンド層(commands/)だけが Tauri を知る。

pub mod core;
pub mod db;
pub mod envfile;
pub mod presets;
pub mod scan;
pub mod secrets;
pub mod validate;
