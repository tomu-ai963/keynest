use crate::vault::presets::{Preset, PRESETS};

#[tauri::command]
pub fn list_presets() -> &'static [Preset] {
    PRESETS
}
