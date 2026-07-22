use tauri::State;

use super::keys::VaultState;
use crate::error::Error;
use crate::vault::scan::{ScanApplyReport, ScanReport, ScanSelection};

#[tauri::command]
pub fn scan_preview(
    state: State<'_, VaultState>,
    path: String,
    max_depth: Option<u32>,
) -> Result<ScanReport, Error> {
    state.vault()?.scan_preview(&path, max_depth.unwrap_or(3))
}

#[tauri::command]
pub fn scan_apply(
    state: State<'_, VaultState>,
    selections: Vec<ScanSelection>,
) -> Result<ScanApplyReport, Error> {
    state.vault()?.scan_apply(&selections)
}
