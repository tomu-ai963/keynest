//! シークレット本体の保存層。
//! keyring クレート経由で OS のセキュアストレージに保存する(暗号化は OS に委譲)。
//! Windows: 資格情報マネージャー / macOS: Keychain / Linux: secret-service。
//! 資格情報の識別子は SQLite 側の keys.id を使う。

use crate::error::Result;

const NAMESPACE: &str = "keynest";

pub trait SecretStore: Send {
    fn set(&self, key_id: i64, value: &str) -> Result<()>;
    fn get(&self, key_id: i64) -> Result<Option<String>>;
    fn delete(&self, key_id: i64) -> Result<()>;
}

pub struct KeyringStore;

impl KeyringStore {
    fn entry(key_id: i64) -> Result<keyring::Entry> {
        Ok(keyring::Entry::new(NAMESPACE, &key_id.to_string())?)
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, key_id: i64, value: &str) -> Result<()> {
        Self::entry(key_id)?.set_password(value)?;
        Ok(())
    }

    fn get(&self, key_id: i64) -> Result<Option<String>> {
        match Self::entry(key_id)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, key_id: i64) -> Result<()> {
        match Self::entry(key_id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// テスト用のインメモリ実装。OS のセキュアストレージには触れない。
#[cfg(test)]
pub mod memory {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::SecretStore;
    use crate::error::Result;

    #[derive(Clone, Default)]
    pub struct MemoryStore(pub Arc<Mutex<HashMap<i64, String>>>);

    impl SecretStore for MemoryStore {
        fn set(&self, key_id: i64, value: &str) -> Result<()> {
            self.0.lock().unwrap().insert(key_id, value.to_string());
            Ok(())
        }

        fn get(&self, key_id: i64) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(&key_id).cloned())
        }

        fn delete(&self, key_id: i64) -> Result<()> {
            self.0.lock().unwrap().remove(&key_id);
            Ok(())
        }
    }
}
