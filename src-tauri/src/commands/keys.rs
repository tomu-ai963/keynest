use std::sync::Mutex;

use tauri::State;

use crate::error::Error;
use crate::vault::core::{
    KeyInfo, RotateResult, RotationInfo, SetKeyResult, ValidateResult, Vault,
};

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

#[tauri::command]
pub fn validate_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
) -> Result<ValidateResult, Error> {
    state
        .0
        .lock()
        .unwrap()
        .validate_key(&service, name.as_deref())
}

#[tauri::command]
pub fn rotate_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
    new_value: String,
) -> Result<RotateResult, Error> {
    state
        .0
        .lock()
        .unwrap()
        .rotate_key(&service, name.as_deref(), &new_value)
}

#[tauri::command]
pub fn list_rotations(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
) -> Result<Vec<RotationInfo>, Error> {
    state
        .0
        .lock()
        .unwrap()
        .list_rotations(&service, name.as_deref())
}
