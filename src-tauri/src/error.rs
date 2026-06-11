use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("データベースエラー: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("資格情報ストアエラー: {0}")]
    Secret(#[from] keyring::Error),
    #[error("{0}")]
    InvalidInput(String),
    #[error("キーが見つかりません: {0}")]
    NotFound(String),
    #[error("{0}")]
    Ambiguous(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// Tauri コマンドの Err はフロントへ文字列として返す
impl Serialize for Error {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
