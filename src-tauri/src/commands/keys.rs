use std::sync::{Mutex, MutexGuard};

use tauri::State;

use crate::error::Error;
use crate::vault::core::{
    KeyInfo, RotateResult, RotationInfo, SetKeyResult, ValidateResult, Vault,
};

pub struct VaultState(pub Mutex<Vault>);

impl VaultState {
    /// vault のロックを取る。直前のコマンドが panic してロックが毒化していた場合、
    /// unwrap で巻き添え panic せず、再起動を促すエラーをフロントへ返す。
    pub fn vault(&self) -> Result<MutexGuard<'_, Vault>, Error> {
        self.0.lock().map_err(|_| {
            Error::Internal(
                "内部エラー: 直前の操作が異常終了したため vault を利用できません。\
                 アプリを再起動してください。 / Internal error: the vault is unavailable \
                 because a previous operation crashed. Please restart the app."
                    .into(),
            )
        })
    }
}

#[tauri::command]
pub fn set_key(
    state: State<'_, VaultState>,
    service: String,
    name: String,
    value: String,
    env_var_name: String,
    expires_at: Option<String>,
    note: Option<String>,
) -> Result<SetKeyResult, Error> {
    state.vault()?.set_key(
        &service,
        &name,
        &value,
        &env_var_name,
        expires_at.as_deref(),
        note.as_deref(),
    )
}

#[tauri::command]
pub fn list_keys(
    state: State<'_, VaultState>,
    service: Option<String>,
) -> Result<Vec<KeyInfo>, Error> {
    state.vault()?.list_keys(service.as_deref())
}

#[tauri::command]
pub fn delete_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
) -> Result<String, Error> {
    state.vault()?.delete_key(&service, name.as_deref())
}

#[tauri::command]
pub fn validate_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
) -> Result<ValidateResult, Error> {
    // プロバイダ API はタイムアウトまで最大15秒かかるため、ネットワーク検証中は
    // ロックを保持しない: begin(ロック内) → run(ロック外) → finish(ロック内)。
    let (pending, value) = state.vault()?.begin_validation(&service, name.as_deref())?;
    let outcome = pending.run(&value)?;
    state.vault()?.finish_validation(&pending, outcome)
}

#[tauri::command]
pub fn rotate_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
    new_value: String,
) -> Result<RotateResult, Error> {
    state
        .vault()?
        .rotate_key(&service, name.as_deref(), &new_value)
}

#[tauri::command]
pub fn list_rotations(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
) -> Result<Vec<RotationInfo>, Error> {
    state.vault()?.list_rotations(&service, name.as_deref())
}
