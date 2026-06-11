mod commands;
mod error;
mod vault;

use std::sync::Mutex;

use tauri::Manager;

use commands::keys::VaultState;
use vault::core::Vault;
use vault::secrets::KeyringStore;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 環境変数 KEYNEST_DB で DB パスを上書き可能(開発・テスト用)
            let db_path = match std::env::var("KEYNEST_DB") {
                Ok(custom) => std::path::PathBuf::from(custom),
                Err(_) => app.path().app_data_dir()?.join("keynest.db"),
            };
            let vault = Vault::open(&db_path, Box::new(KeyringStore))?;
            app.manage(VaultState(Mutex::new(vault)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::keys::set_key,
            commands::keys::list_keys,
            commands::keys::delete_key,
            commands::keys::validate_key,
            commands::keys::rotate_key,
            commands::keys::list_rotations,
            commands::presets::list_presets,
            commands::projects::assign_key,
            commands::projects::unassign_key,
            commands::projects::list_projects,
            commands::projects::delete_project,
            commands::projects::inject,
            commands::scan::scan_preview,
            commands::scan::scan_apply,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
