mod commands;
mod path_env;
mod settings;

use ralph_core::session::manager::SessionManager;
use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    path_env::augment_path_for_gui_launch();
    settings::apply_tool_path_overrides(&settings::load_settings());
    let manager = Arc::new(SessionManager::new());
    tauri::Builder::default()
        .manage(manager)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::discover_modes,
            commands::create_session,
            commands::start_session,
            commands::stop_session,
            commands::cancel_stop_session,
            commands::abort_session,
            commands::remove_session,
            commands::list_sessions,
            commands::list_log_iterations,
            commands::read_log_iteration,
            commands::read_log_iteration_view,
            commands::get_available_tools,
            commands::detect_tool_paths,
            commands::list_backend_models,
            commands::resume_session,
            commands::send_recovery_action,
            settings::get_settings,
            settings::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
