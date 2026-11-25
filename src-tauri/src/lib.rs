mod commands;
mod elevation;

use commands::AppState;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, PhysicalPosition, PhysicalSize, RunEvent, WindowEvent};
use unbind_core::models::WindowBounds;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliAction {
    pub action: String,
    pub path: String,
}

/// Parse Explorer context menu CLI invocations (`--force-remove <path>` / `--track-install <path>`).
pub fn parse_cli_action(args: &[String]) -> Option<CliAction> {
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--force-remove" if i + 1 < args.len() => {
                let p = args[i + 1].trim().to_string();
                if !p.is_empty() {
                    return Some(CliAction {
                        action: "forceRemove".to_string(),
                        path: p,
                    });
                }
                i += 2;
            }
            "--track-install" if i + 1 < args.len() => {
                let p = args[i + 1].trim().to_string();
                if !p.is_empty() {
                    return Some(CliAction {
                        action: "trackInstall".to_string(),
                        path: p,
                    });
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    None
}

fn restore_window_bounds(window: &tauri::WebviewWindow, bounds: &WindowBounds) {
    if bounds.width < 800 || bounds.height < 540 {
        return;
    }

    // Verify saved position intersects at least one active monitor so unplugged displays
    // never leave the window stranded off-screen.
    let mut pos_valid = false;
    if let Ok(monitors) = window.available_monitors() {
        for m in monitors {
            let mp = m.position();
            let ms = m.size();
            let right = mp.x + ms.width as i32;
            let bottom = mp.y + ms.height as i32;
            if bounds.x >= mp.x - 100
                && bounds.x < right - 120
                && bounds.y >= mp.y - 40
                && bounds.y < bottom - 120
            {
                pos_valid = true;
                break;
            }
        }
    }

    let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
    if pos_valid {
        let _ = window.set_position(PhysicalPosition::new(bounds.x, bounds.y));
    }
    if bounds.maximized {
        let _ = window.maximize();
    }
}

fn persist_window_bounds(window: &tauri::Window, state: &AppState) {
    let maximized = window.is_maximized().unwrap_or(false);
    let storage = state.current_storage();
    let mut settings = storage.load_settings();

    if maximized {
        if let Some(prev) = settings.window_bounds.as_mut() {
            prev.maximized = true;
        } else {
            settings.window_bounds = Some(WindowBounds {
                width: 1220,
                height: 780,
                x: 100,
                y: 100,
                maximized: true,
            });
        }
    } else if let (Ok(size), Ok(pos)) = (window.outer_size(), window.outer_position()) {
        if size.width >= 800 && size.height >= 540 {
            settings.window_bounds = Some(WindowBounds {
                width: size.width,
                height: size.height,
                x: pos.x,
                y: pos.y,
                maximized: false,
            });
        }
    }

    let _ = storage.save_settings(&settings);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(initial_cli_action: Option<CliAction>) {
    let app_state = AppState::new(initial_cli_action);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            if let Some(action) = parse_cli_action(&argv) {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut guard) = state.pending_cli_action.lock() {
                        *guard = Some(action.clone());
                    }
                }
                let _ = app.emit("cli://action", action);
            }
        }))
        .manage(app_state)
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(target_os = "windows")]
                {
                    let _ = window_vibrancy::apply_mica(&window, None)
                        .or_else(|_| window_vibrancy::apply_acrylic(&window, Some((18, 18, 22, 180))));
                }
                if let Some(state) = app.try_state::<AppState>() {
                    let settings = state.current_storage().load_settings();
                    if let Some(bounds) = &settings.window_bounds {
                        restore_window_bounds(&window, bounds);
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                if let Some(state) = window.try_state::<AppState>() {
                    persist_window_bounds(window, &state);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_system_status,
            commands::get_settings,
            commands::save_settings,
            commands::list_apps,
            commands::resolve_app_sizes,
            commands::get_app_icons,
            commands::elevate_session,
            commands::run_uninstall,
            commands::scan_app_leftovers,
            commands::scan_force_target,
            commands::clean_leftovers,
            commands::find_running_app_processes,
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
            commands::export_install_trace,
            commands::import_install_trace,
            commands::list_registry_backups,
            commands::preview_registry_backup,
            commands::restore_registry_backup,
            commands::delete_registry_backup,
            commands::list_operation_logs,
            commands::clear_operation_logs,
            commands::create_restore_point,
            commands::open_backups_folder,
            commands::open_path_in_explorer,
            commands::open_key_in_regedit,
            commands::open_external_url,
            commands::export_apps,
            commands::get_shell_integration_status,
            commands::set_shell_integration,
            commands::check_for_updates,
            commands::take_pending_cli_action,
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
