mod commands;
mod error;
pub mod mcp;
mod vault;

use std::path::Path;
use std::sync::Mutex;

use tauri::Manager;

use commands::keys::VaultState;
use vault::core::Vault;
use vault::secrets::KeyringStore;

/// v0.1.0 までの identifier "com.keynest.app" 配下にある既存 DB を、
/// 新 identifier のディレクトリへ 1 回だけコピー移行する。
/// 新パスに DB が既にあれば何もしない(冪等)。
/// シークレット本体は identifier 非依存の keyring 名前空間に残るため、移すのは DB だけでよい。
fn migrate_legacy_db(new_dir: &Path, new_path: &Path) {
    if new_path.exists() {
        return;
    }
    let Some(parent) = new_dir.parent() else {
        return;
    };
    let legacy_path = parent.join("com.keynest.app").join("keynest.db");
    if !legacy_path.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(new_dir) {
        eprintln!("旧DBの移行に失敗しました(ディレクトリ作成) / legacy DB migration failed: {e}");
        return;
    }
    if let Err(e) = std::fs::copy(&legacy_path, new_path) {
        eprintln!("旧DBの移行に失敗しました(コピー) / legacy DB migration failed: {e}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 環境変数 KEYNEST_DB で DB パスを上書き可能(開発・テスト用)
            let db_path = match std::env::var("KEYNEST_DB") {
                Ok(custom) => std::path::PathBuf::from(custom),
                Err(_) => {
                    let dir = app.path().app_data_dir()?;
                    let path = dir.join("keynest.db");
                    migrate_legacy_db(&dir, &path);
                    path
                }
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
