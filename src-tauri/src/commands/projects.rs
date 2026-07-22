use tauri::State;

use super::keys::VaultState;
use crate::error::Error;
use crate::vault::core::{AssignResult, InjectResult, ProjectInfo};

#[tauri::command]
pub fn assign_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
    project: String,
    project_path: Option<String>,
) -> Result<AssignResult, Error> {
    state
        .vault()?
        .assign_key(&service, name.as_deref(), &project, project_path.as_deref())
}

#[tauri::command]
pub fn unassign_key(
    state: State<'_, VaultState>,
    service: String,
    name: Option<String>,
    project: String,
) -> Result<String, Error> {
    state
        .vault()?
        .unassign_key(&service, name.as_deref(), &project)
}

#[tauri::command]
pub fn list_projects(state: State<'_, VaultState>) -> Result<Vec<ProjectInfo>, Error> {
    state.vault()?.list_projects()
}

#[tauri::command]
pub fn delete_project(state: State<'_, VaultState>, project: String) -> Result<(), Error> {
    state.vault()?.delete_project(&project)
}

#[tauri::command]
pub fn inject(
    state: State<'_, VaultState>,
    project: String,
    path: Option<String>,
) -> Result<InjectResult, Error> {
    state.vault()?.inject(&project, path.as_deref())
}
