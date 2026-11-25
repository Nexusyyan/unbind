mod commands;
mod elevation;

use commands::AppState;
use tauri::{Manager, RunEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState::new();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .setup(|app| {
            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                // Enable Windows 11 Mica backdrop (falls back gracefully on Windows 10).
                let _ = window_vibrancy::apply_mica(&window, None)
                    .or_else(|_| window_vibrancy::apply_acrylic(&window, Some((18, 18, 22, 180))));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_system_status,
            commands::get_settings,
            commands::save_settings,
            commands::list_apps,
            commands::get_app_icons,
            commands::elevate_session,
            commands::run_uninstall,
            commands::scan_app_leftovers,
            commands::scan_force_target,
            commands::clean_leftovers,
            commands::check_file_locks,
            commands::terminate_locking_processes,
            commands::list_startup_items,
            commands::set_startup_enabled,
            commands::delete_startup_item,
            commands::start_install_track,
            commands::finish_install_track,
            commands::cancel_install_track,
            commands::list_active_track_sessions,
            commands::list_install_traces,
            commands::get_trace_leftovers,
            commands::delete_install_trace,
            commands::list_registry_backups,
            commands::list_operation_logs,
            commands::create_restore_point,
            commands::open_backups_folder,
            commands::open_path_in_explorer,
            commands::open_key_in_regedit,
            commands::open_external_url,
            commands::export_apps,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Unbind");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<AppState>() {
                state.elevation.shutdown();
            }
        }
    });
}
