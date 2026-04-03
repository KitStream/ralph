mod commands;
mod settings;

use std::sync::Arc;
use ralph_core::session::manager::SessionManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let manager = Arc::new(SessionManager::new());
    tauri::Builder::default()
        .manage(manager)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::discover_modes,
            commands::create_session,
            commands::start_session,
            commands::stop_session,
            commands::cancel_stop_session,
            commands::abort_session,
            commands::remove_session,
            commands::list_sessions,
            commands::get_available_tools,
            commands::resume_session,
            commands::send_recovery_action,
            settings::get_settings,
            settings::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
