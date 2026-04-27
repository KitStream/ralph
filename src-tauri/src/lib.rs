mod commands;
mod path_env;
mod settings;

use ralph_core::session::manager::SessionManager;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItemBuilder, SubmenuBuilder};
use tauri::Emitter;

const MENU_ID_CHECK_UPDATES: &str = "check-updates";
/// Event the Rust menu fires to ask the React frontend's `useAppUpdate` hook
/// to invoke its existing `checkForUpdate()` flow.
const EVENT_REQUEST_CHECK_UPDATES: &str = "request-check-for-updates";

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
        .setup(|app| {
            let check_updates =
                MenuItemBuilder::with_id(MENU_ID_CHECK_UPDATES, "Check for Updates…").build(app)?;
            let help = SubmenuBuilder::new(app, "Help")
                .item(&check_updates)
                .build()?;
            let menu = Menu::default(app.handle())?;
            menu.append(&help)?;
            app.set_menu(menu)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id() == MENU_ID_CHECK_UPDATES {
                let _ = app.emit(EVENT_REQUEST_CHECK_UPDATES, ());
            }
        })
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
