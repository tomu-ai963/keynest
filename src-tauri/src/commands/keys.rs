use std::sync::Mutex;

use tauri::State;

use crate::error::Error;
use crate::vault::core::{KeyInfo, SetKeyResult, Vault};

pub struct VaultState(pub Mutex<Vault>);

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
    state.0.lock().unwrap().set_key(
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
    state.0.lock().unwrap().list_keys(service.as_deref())
}

#[tauri::command]
pub fn delete_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
) -> Result<String, Error> {
    state
        .0
        .lock()
        .unwrap()
        .delete_key(&service, name.as_deref())
}
