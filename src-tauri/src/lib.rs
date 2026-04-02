mod commands;
mod settings;

use ralph_core::session::manager::SessionManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(SessionManager::new())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::discover_modes,
            commands::create_session,
            commands::start_session,
            commands::stop_session,
            commands::abort_session,
            commands::remove_session,
            commands::list_sessions,
            commands::get_available_tools,
            commands::send_recovery_action,
            settings::get_settings,
            settings::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
