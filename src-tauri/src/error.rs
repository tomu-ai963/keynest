use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("データベースエラー: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{0}")]
    Secret(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("キーが見つかりません: {0}")]
    NotFound(String),
    #[error("{0}")]
    Ambiguous(String),
    #[error("{0}")]
    Internal(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// keyring クレートのエラーを、原因が分かる日本語+英語メッセージに変換する。
/// 特に Linux で Secret Service(gnome-keyring / KWallet 等)が動いていない場合、
/// 生の DBus エラーではなく必要な前提を案内する。
impl From<keyring::Error> for Error {
    fn from(e: keyring::Error) -> Self {
        let msg = match &e {
            keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => format!(
                "OSのセキュアストレージにアクセスできませんでした。\
                 Linux では gnome-keyring / KWallet などの Secret Service が起動している必要があります。 / \
                 Could not access the OS secure store. On Linux a Secret Service provider \
                 (gnome-keyring, KWallet, etc.) must be running. (詳細/detail: {e})"
            ),
            keyring::Error::TooLong(_, limit) => format!(
                "キーが長すぎてこのOSのセキュアストレージに保存できません(上限 {limit} バイト)。 / \
                 The key is too long for this OS secure store (limit {limit} bytes). (詳細/detail: {e})"
            ),
            _ => format!("資格情報ストアエラー / secret store error: {e}"),
        };
        Error::Secret(msg)
    }
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
