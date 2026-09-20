mod commands;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let proxy_root = app
                .path()
                .app_cache_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("cullflow-proxies");
            app.manage(AppState::new(proxy_root));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::scan_folder,
            commands::analyze_project,
            commands::set_tolerance,
            commands::export_xml,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
