use crate::elevation::ElevationBridge;
use crate::tray::{
    self, TrayNavigatePayload, TrayRuntimeState, TraySnapshot, TRAY_LOGICAL_WIDTH,
    TRAY_WINDOW_LABEL,
};
use crate::CliAction;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State};
use unbind_core::discovery::icon::IconCache;
use unbind_core::discovery::size::resolve_missing_sizes_batched;
use unbind_core::discovery::{
    discover_all_apps_with_cache, open_in_explorer, open_in_regedit, open_url,
};
use unbind_core::export::export_apps_list;
use unbind_core::ipc::WorkerRequest;
use unbind_core::models::{
    ActiveTrackSession, AppKind, AppSettings, BackupEntry, CleanupOptions, CleanupReport,
    CoreError, EtwCaptureResult, EtwSessionStats, ExportFormat, InstallScope, InstallTrace,
    InstalledApp, LeftoverItem, LockingProcessInfo, OperationLogEntry, PortableApp,
    RunningProcessInfo, ShellIntegrationStatus, ShortcutLocation, StartupItem, SystemStatus,
    TrackMode, UninstallExecutionResult, UpdateCheckResult, WingetCheckResult, WingetUpgradeResult,
};
use unbind_core::portable::AppStorage;
use unbind_core::portable_apps::{
    create_portable_shortcut as core_create_portable_shortcut,
    launch_portable_app as core_launch_portable_app, scan_portable_apps as core_scan_portable_apps,
};
use unbind_core::safety::{
    create_system_restore_point, delete_backup_file, is_process_elevated, list_backups,
    preview_registry_backup as core_preview_backup, reg_backup_requires_elevation,
    restore_registry_backup as core_restore_backup,
};
use unbind_core::shell_ext::{
    get_shell_integration_status as core_shell_status,
    set_shell_integration as core_set_shell_integration,
};
use unbind_core::startup::{
    delete_startup_item as core_delete_startup, list_startup_items as core_list_startup,
    set_startup_enabled as core_set_startup_enabled,
};
use unbind_core::tracker::{
    delete_trace_file, export_trace_to_file, import_trace_from_file, load_all_traces,
    load_trace_by_id, trace_to_leftover_items, TrackerManager,
};
use unbind_core::uninstaller::leftover::{build_force_target_from_path, scan_leftovers};
use unbind_core::uninstaller::{execute_cleanup, run_native_uninstaller};
use unbind_core::unlocker::{
    find_locking_processes, find_running_processes_in_dir, terminate_processes,
};
use unbind_core::updater::check_github_release;
use unbind_core::winget::{
    check_winget_updates as core_check_winget_updates,
    run_winget_upgrade as core_run_winget_upgrade,
};
use uuid::Uuid;

pub struct AppState {
    pub storage: Mutex<AppStorage>,
    pub apps_cache: Arc<Mutex<Vec<InstalledApp>>>,
    pub portable_cache: Arc<Mutex<Vec<PortableApp>>>,
    pub icon_cache: IconCache,
    pub tracker: TrackerManager,
    pub elevation: ElevationBridge,
    pub size_scan_gen: Arc<AtomicU64>,
    pub pending_cli_action: Mutex<Option<CliAction>>,
    pub tray: TrayRuntimeState,
}

impl AppState {
    pub fn new(initial_cli_action: Option<CliAction>) -> Self {
        Self {
            storage: Mutex::new(AppStorage::detect()),
            apps_cache: Arc::new(Mutex::new(Vec::new())),
            portable_cache: Arc::new(Mutex::new(Vec::new())),
            icon_cache: IconCache::new(),
            tracker: TrackerManager::new(),
            elevation: ElevationBridge::new(),
            size_scan_gen: Arc::new(AtomicU64::new(0)),
            pending_cli_action: Mutex::new(initial_cli_action),
            tray: TrayRuntimeState::new(),
        }
    }

    pub fn current_storage(&self) -> AppStorage {
        self.storage
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| AppStorage::detect())
    }
}

#[tauri::command]
pub fn get_system_status(state: State<'_, AppState>) -> SystemStatus {
    let storage = state.current_storage();
    SystemStatus {
        is_elevated: is_process_elevated(),
        worker_connected: state.elevation.is_worker_connected(),
        portable_mode: storage.portable_mode,
        data_dir: storage.data_dir.to_string_lossy().to_string(),
        windows_version: unbind_core::get_windows_version_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.current_storage().load_settings()
}

#[tauri::command]
pub fn save_settings(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let storage = state.current_storage();
    // Preserve stored window bounds when settings form omits them.
    let mut merged = settings;
    if merged.window_bounds.is_none() {
        merged.window_bounds = storage.load_settings().window_bounds;
    }
    let updated = storage.save_settings(&merged).map_err(|e| e.to_string())?;
    if let Ok(mut guard) = state.storage.lock() {
        *guard = AppStorage::for_dir(PathBuf::from(&updated.data_dir), updated.portable_mode);
    }
    let _ = app_handle.emit("settings://changed", &updated);
    tray::update_tray_tooltip(&app_handle);
    Ok(updated)
}

#[tauri::command]
pub fn get_tray_snapshot(app_handle: AppHandle, state: State<'_, AppState>) -> TraySnapshot {
    tray::build_tray_snapshot(&app_handle, &state)
}

#[tauri::command]
pub fn resize_tray_menu(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    height: f64,
) -> Result<(), String> {
    let clamped = state.tray.set_logical_height(height);
    if let Some(win) = app_handle.get_webview_window(TRAY_WINDOW_LABEL) {
        let _ = win.set_size(LogicalSize::new(TRAY_LOGICAL_WIDTH, clamped));
    }
    Ok(())
}

#[tauri::command]
pub fn hide_tray_menu(app_handle: AppHandle) {
    tray::hide_tray_menu(&app_handle);
}

#[tauri::command]
pub fn preview_tray_menu(app_handle: AppHandle) {
    tray::show_tray_menu_at(&app_handle, None);
}

#[tauri::command]
pub fn execute_tray_action(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    action: String,
) -> Result<(), String> {
    match action.as_str() {
        "showMain" => {
            tray::show_main_window(&app_handle);
        }
        "nav:apps" => {
            tray::show_main_window(&app_handle);
            let _ = app_handle.emit_to(
                "main",
                "tray://navigate",
                TrayNavigatePayload {
                    target: "apps".to_string(),
                },
            );
        }
        "nav:tools" => {
            tray::show_main_window(&app_handle);
            let _ = app_handle.emit_to(
                "main",
                "tray://navigate",
                TrayNavigatePayload {
                    target: "tools".to_string(),
                },
            );
        }
        "nav:tracker" => {
            tray::show_main_window(&app_handle);
            let _ = app_handle.emit_to(
                "main",
                "tray://navigate",
                TrayNavigatePayload {
                    target: "tracker".to_string(),
                },
            );
        }
        "nav:settings" => {
            tray::show_main_window(&app_handle);
            let _ = app_handle.emit_to(
                "main",
                "tray://navigate",
                TrayNavigatePayload {
                    target: "settings".to_string(),
                },
            );
        }
        "action:forceRemove" => {
            tray::show_main_window(&app_handle);
            let _ = app_handle.emit_to(
                "main",
                "tray://navigate",
                TrayNavigatePayload {
                    target: "forceRemove".to_string(),
                },
            );
        }
        "setLanguage:zh-CN" | "setLanguage:en" => {
            let lang = action.strip_prefix("setLanguage:").unwrap_or("en");
            let storage = state.current_storage();
            let mut settings = storage.load_settings();
            settings.language = lang.to_string();
            let updated = storage.save_settings(&settings).map_err(|e| e.to_string())?;
            let _ = app_handle.emit("settings://changed", &updated);
            tray::update_tray_tooltip(&app_handle);
        }
        "setTheme:system" | "setTheme:light" | "setTheme:dark" => {
            let theme = action.strip_prefix("setTheme:").unwrap_or("system");
            let storage = state.current_storage();
            let mut settings = storage.load_settings();
            settings.theme = theme.to_string();
            let updated = storage.save_settings(&settings).map_err(|e| e.to_string())?;
            let _ = app_handle.emit("settings://changed", &updated);
            tray::update_tray_tooltip(&app_handle);
        }
        "quit" => {
            tray::hide_tray_menu(&app_handle);
            state.tray.mark_exiting();
            crate::persist_main_window_bounds(&app_handle, &state);
            app_handle.exit(0);
        }
        other => {
            return Err(format!("Unknown tray action: {}", other));
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_apps(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<InstalledApp>, String> {
    let storage = state.current_storage();
    let traces_dir = storage.traces_dir();
    let cache_file = storage.size_cache_file();
    let apps = tauri::async_runtime::spawn_blocking(move || {
        discover_all_apps_with_cache(&traces_dir, Some(&cache_file))
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Ok(mut guard) = state.apps_cache.lock() {
        *guard = apps.clone();
    }
    tray::update_tray_tooltip(&app_handle);
    Ok(apps)
}

#[tauri::command]
pub async fn resolve_app_sizes(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let my_gen = state.size_scan_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let gen_ref = Arc::clone(&state.size_scan_gen);
    let apps_cache_ref = Arc::clone(&state.apps_cache);
    let cache_file = state.current_storage().size_cache_file();

    let snapshot: Vec<InstalledApp> = {
        let guard = state.apps_cache.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };

    tauri::async_runtime::spawn_blocking(move || {
        resolve_missing_sizes_batched(
            &snapshot,
            &cache_file,
            || gen_ref.load(Ordering::SeqCst) != my_gen,
            |batch| {
                if let Ok(mut guard) = apps_cache_ref.lock() {
                    for app in guard.iter_mut() {
                        if let Some(&kb) = batch.get(&app.id) {
                            app.estimated_size_kb = kb;
                            app.size_measured = true;
                        }
                    }
                }
                let _ = app_handle.emit("apps://size-batch", &batch);
            },
        );
        if gen_ref.load(Ordering::SeqCst) == my_gen {
            let _ = app_handle.emit("apps://size-done", true);
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn get_app_icons(
    state: State<'_, AppState>,
    app_ids: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let target_apps: Vec<InstalledApp> = {
        let guard = state.apps_cache.lock().map_err(|e| e.to_string())?;
        let id_set: std::collections::HashSet<&str> = app_ids.iter().map(|s| s.as_str()).collect();
        guard
            .iter()
            .filter(|a| id_set.contains(a.id.as_str()))
            .cloned()
            .collect()
    };

    let pairs: Vec<(String, Option<String>)> = target_apps
        .par_iter()
        .map(|app| (app.id.clone(), state.icon_cache.get_or_extract(app)))
        .collect();

    let mut out = HashMap::with_capacity(pairs.len());
    for (id, uri_opt) in pairs {
        if let Some(uri) = uri_opt {
            out.insert(id, uri);
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn elevate_session(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    state.elevation.ensure_connected().map_err(|e| e.to_string())?;
    tray::update_tray_tooltip(&app_handle);
    Ok(true)
}

#[tauri::command]
pub async fn run_uninstall(
    state: State<'_, AppState>,
    app: InstalledApp,
    quiet: bool,
) -> Result<UninstallExecutionResult, String> {
    let storage = state.current_storage();
    let needs_elev = app.kind != AppKind::StoreApp
        && (app.scope == InstallScope::Machine || app.registry_hive.eq_ignore_ascii_case("HKLM"));

    let res = if needs_elev && !is_process_elevated() {
        let val = state
            .elevation
            .execute(WorkerRequest::UninstallApp {
                app: Box::new(app.clone()),
                quiet,
            })
            .map_err(|e| e.to_string())?;
        serde_json::from_value::<UninstallExecutionResult>(val).map_err(|e| e.to_string())?
    } else {
        match run_native_uninstaller(&app, quiet) {
            Ok(r) => r,
            Err(CoreError::ElevationRequired(_)) => {
                let val = state
                    .elevation
                    .execute(WorkerRequest::UninstallApp {
                        app: Box::new(app.clone()),
                        quiet,
                    })
                    .map_err(|e| e.to_string())?;
                serde_json::from_value::<UninstallExecutionResult>(val)
                    .map_err(|e| e.to_string())?
            }
            Err(e) => return Err(e.to_string()),
        }
    };

    let _ = storage.append_log(
        if quiet { "quiet_uninstall" } else { "uninstall" },
        &app.name,
        &res.message,
        res.launched && !res.timed_out,
    );
    Ok(res)
}

#[tauri::command]
pub async fn scan_app_leftovers(
    state: State<'_, AppState>,
    app: InstalledApp,
) -> Result<Vec<LeftoverItem>, String> {
    let traces_dir = state.current_storage().traces_dir();
    let items = tauri::async_runtime::spawn_blocking(move || {
        let trace = app
            .tracked_trace_id
            .as_deref()
            .and_then(|tid| load_trace_by_id(&traces_dir, tid));
        scan_leftovers(&app, trace.as_ref())
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(items)
}

#[tauri::command]
pub async fn scan_force_target(path: String) -> Result<(InstalledApp, Vec<LeftoverItem>), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = build_force_target_from_path(Path::new(&path));
        let leftovers = scan_leftovers(&target, None);
        (target, leftovers)
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clean_leftovers(
    state: State<'_, AppState>,
    app_name: String,
    items: Vec<LeftoverItem>,
    options: CleanupOptions,
) -> Result<CleanupReport, String> {
    let storage = state.current_storage();
    let backup_dir = storage.backups_dir();
    let needs_elev = options.create_restore_point || items.iter().any(|i| i.requires_elevation);

    let report = if needs_elev && !is_process_elevated() {
        let val = state
            .elevation
            .execute(WorkerRequest::CleanLeftovers {
                app_name: app_name.clone(),
                items,
                options,
                backup_dir: backup_dir.to_string_lossy().to_string(),
            })
            .map_err(|e| e.to_string())?;
        serde_json::from_value::<CleanupReport>(val).map_err(|e| e.to_string())?
    } else {
        execute_cleanup(&app_name, &items, &options, &backup_dir).map_err(|e| e.to_string())?
    };

    let summary = format!(
        "Cleaned {} items ({} failed, {} reboot-scheduled, freed {} bytes)",
        report.succeeded, report.failed, report.scheduled_for_reboot, report.freed_bytes
    );
    let _ = storage.append_log("clean_leftovers", &app_name, &summary, report.failed == 0);
    Ok(report)
}

#[tauri::command]
pub async fn find_running_app_processes(
    install_location: String,
) -> Result<Vec<RunningProcessInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        find_running_processes_in_dir(Path::new(&install_location))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_file_locks(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> Result<Vec<LockingProcessInfo>, String> {
    let pbufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    match find_locking_processes(&pbufs) {
        Ok(list) => Ok(list),
        Err(_) if !is_process_elevated() => {
            let val = state
                .elevation
                .execute(WorkerRequest::CheckLocks { paths })
                .map_err(|e| e.to_string())?;
            serde_json::from_value(val).map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn terminate_locking_processes(
    state: State<'_, AppState>,
    pids: Vec<u32>,
) -> Result<Vec<(u32, bool, Option<String>)>, String> {
    if is_process_elevated() {
        terminate_processes(&pids).map_err(|e| e.to_string())
    } else {
        let local = terminate_processes(&pids).map_err(|e| e.to_string())?;
        let failed_pids: Vec<u32> = local
            .iter()
            .filter(|(_, ok, _)| !ok)
            .map(|(pid, _, _)| *pid)
            .collect();
        if failed_pids.is_empty() {
            return Ok(local);
        }
        let val = state
            .elevation
            .execute(WorkerRequest::TerminateProcesses { pids: failed_pids })
            .map_err(|e| e.to_string())?;
        serde_json::from_value(val).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn list_startup_items() -> Result<Vec<StartupItem>, String> {
    tauri::async_runtime::spawn_blocking(core_list_startup)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_startup_enabled(
    state: State<'_, AppState>,
    item: StartupItem,
    enabled: bool,
) -> Result<(), String> {
    let storage = state.current_storage();
    let res = if item.requires_elevation && !is_process_elevated() {
        state
            .elevation
            .execute(WorkerRequest::SetStartupEnabled {
                item: item.clone(),
                enabled,
            })
            .map(|_| ())
    } else {
        match core_set_startup_enabled(&item, enabled) {
            Ok(()) => Ok(()),
            Err(CoreError::ElevationRequired(_)) => state
                .elevation
                .execute(WorkerRequest::SetStartupEnabled {
                    item: item.clone(),
                    enabled,
                })
                .map(|_| ()),
            Err(e) => Err(e),
        }
    };

    let _ = storage.append_log(
        if enabled { "startup_enable" } else { "startup_disable" },
        &item.name,
        &item.location,
        res.is_ok(),
    );
    res.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_startup_item(
    state: State<'_, AppState>,
    item: StartupItem,
) -> Result<(), String> {
    let storage = state.current_storage();
    let res = if item.requires_elevation && !is_process_elevated() {
        state
            .elevation
            .execute(WorkerRequest::DeleteStartupItem { item: item.clone() })
            .map(|_| ())
    } else {
        match core_delete_startup(&item) {
            Ok(()) => Ok(()),
            Err(CoreError::ElevationRequired(_)) => state
                .elevation
                .execute(WorkerRequest::DeleteStartupItem { item: item.clone() })
                .map(|_| ()),
            Err(e) => Err(e),
        }
    };

    let _ = storage.append_log("startup_delete", &item.name, &item.location, res.is_ok());
    res.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn start_install_track(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    app_name: String,
    installer_path: String,
    launch_installer: bool,
    extra_roots: Option<Vec<String>>,
    track_mode: Option<TrackMode>,
) -> Result<ActiveTrackSession, String> {
    let roots = extra_roots.unwrap_or_default();
    let mode = track_mode.unwrap_or(TrackMode::HybridSnapshot);
    let handle = app_handle.clone();

    if mode == TrackMode::EtwRealtime {
        let trimmed_installer = installer_path.trim();
        if trimmed_installer.is_empty() || !launch_installer {
            return Err(
                "ETW real-time mode requires selecting an installer and enabling auto-launch"
                    .to_string(),
            );
        }

        let session_id = Uuid::new_v4().to_string();
        let track_msi_service = trimmed_installer.to_ascii_lowercase().ends_with(".msi");

        // 1. Start named real-time ETW session in pre-bind buffer mode BEFORE launching installer.
        state
            .elevation
            .execute(WorkerRequest::StartEtwTrack {
                session_id: session_id.clone(),
                track_msi_service,
            })
            .map_err(|e| e.to_string())?;

        // 2. Capture baseline snapshot and launch installer from the standard UI process.
        let started = state.tracker.start_session_with_mode(
            Some(session_id.clone()),
            &app_name,
            trimmed_installer,
            true,
            &roots,
            TrackMode::EtwRealtime,
            true,
            Some(Box::new(move |sid, exit_code| {
                let _ = handle.emit(
                    "tracker://installer-exited",
                    serde_json::json!({
                        "sessionId": sid,
                        "exitCode": exit_code,
                    }),
                );
            })),
        );

        let mut sess = match started {
            Ok(s) => s,
            Err(e) => {
                // Roll back kernel ETW session immediately on launch failure so no orphan session leaks.
                let _ = state
                    .elevation
                    .execute(WorkerRequest::CancelEtwTrack { session_id });
                return Err(e.to_string());
            }
        };

        // 3. Bind installer root PID immediately so pre-buffered startup events replay cleanly.
        if let Some(pid) = sess.installer_pid {
            if let Ok(val) = state.elevation.execute(WorkerRequest::BindEtwRootPid {
                session_id: sess.session_id.clone(),
                root_pid: pid,
            }) {
                if let Ok(stats) = serde_json::from_value::<EtwSessionStats>(val) {
                    sess.tracked_pid_count = stats.tracked_pid_count;
                    sess.captured_file_count = stats.captured_file_count;
                    sess.captured_reg_count = stats.captured_reg_count;
                    state.tracker.update_etw_stats(
                        &sess.session_id,
                        stats.etw_active,
                        stats.tracked_pid_count,
                        stats.captured_file_count,
                        stats.captured_reg_count,
                    );
                }
            }
        }

        tray::update_tray_tooltip(&app_handle);
        return Ok(sess);
    }

    let started = state
        .tracker
        .start_session_with_mode(
            None,
            &app_name,
            &installer_path,
            launch_installer,
            &roots,
            TrackMode::HybridSnapshot,
            false,
            Some(Box::new(move |session_id, exit_code| {
                let _ = handle.emit(
                    "tracker://installer-exited",
                    serde_json::json!({
                        "sessionId": session_id,
                        "exitCode": exit_code,
                    }),
                );
            })),
        )
        .map_err(|e| e.to_string())?;
    tray::update_tray_tooltip(&app_handle);
    Ok(started)
}

#[tauri::command]
pub async fn poll_install_track_stats(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<EtwSessionStats>, String> {
    if !state.tracker.session_uses_etw(&session_id) {
        return Ok(None);
    }
    let val = state
        .elevation
        .execute(WorkerRequest::PollEtwTrackStats {
            session_id: session_id.clone(),
        })
        .map_err(|e| e.to_string())?;
    let stats: Option<EtwSessionStats> = serde_json::from_value(val).map_err(|e| e.to_string())?;
    if let Some(ref s) = stats {
        state.tracker.update_etw_stats(
            &session_id,
            s.etw_active,
            s.tracked_pid_count,
            s.captured_file_count,
            s.captured_reg_count,
        );
    }
    Ok(stats)
}

#[tauri::command]
pub async fn finish_install_track(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<InstallTrace, String> {
    let storage = state.current_storage();
    let etw_capture = if state.tracker.session_uses_etw(&session_id) {
        match state.elevation.execute(WorkerRequest::StopEtwTrack {
            session_id: session_id.clone(),
        }) {
            Ok(val) => serde_json::from_value::<EtwCaptureResult>(val).ok(),
            Err(_) => None,
        }
    } else {
        None
    };

    let trace = state
        .tracker
        .finish_session_with_etw(&session_id, &storage.traces_dir(), etw_capture)
        .map_err(|e| e.to_string())?;
    let mode_tag = match trace.track_mode {
        TrackMode::EtwRealtime => "ETW",
        TrackMode::HybridSnapshot => "Snapshot",
    };
    let summary = format!(
        "[{}] Recorded {} dirs, {} files, {} reg keys, {} reg values, {} services, {} tasks",
        mode_tag,
        trace.added_dirs.len(),
        trace.added_files.len(),
        trace.added_registry_keys.len(),
        trace.added_registry_values.len(),
        trace.added_services.len(),
        trace.added_scheduled_tasks.len()
    );
    let _ = storage.append_log("install_trace", &trace.app_name, &summary, true);
    tray::update_tray_tooltip(&app_handle);
    Ok(trace)
}

#[tauri::command]
pub fn cancel_install_track(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> bool {
    if state.tracker.session_uses_etw(&session_id) {
        let _ = state.elevation.execute(WorkerRequest::CancelEtwTrack {
            session_id: session_id.clone(),
        });
    }
    let cancelled = state.tracker.cancel_session(&session_id);
    tray::update_tray_tooltip(&app_handle);
    cancelled
}

#[tauri::command]
pub fn list_active_track_sessions(state: State<'_, AppState>) -> Vec<ActiveTrackSession> {
    state.tracker.active_sessions()
}

#[tauri::command]
pub fn list_install_traces(state: State<'_, AppState>) -> Vec<InstallTrace> {
    load_all_traces(&state.current_storage().traces_dir())
}

#[tauri::command]
pub fn get_trace_leftovers(
    state: State<'_, AppState>,
    trace_id: String,
) -> Result<Vec<LeftoverItem>, String> {
    let trace = load_trace_by_id(&state.current_storage().traces_dir(), &trace_id)
        .ok_or_else(|| format!("Trace '{}' not found", trace_id))?;
    Ok(trace_to_leftover_items(&trace))
}

#[tauri::command]
pub fn delete_install_trace(state: State<'_, AppState>, trace_id: String) -> Result<(), String> {
    delete_trace_file(&state.current_storage().traces_dir(), &trace_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_install_trace(
    state: State<'_, AppState>,
    trace_id: String,
    output_path: String,
) -> Result<(), String> {
    export_trace_to_file(
        &state.current_storage().traces_dir(),
        &trace_id,
        Path::new(&output_path),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_install_trace(
    state: State<'_, AppState>,
    input_path: String,
) -> Result<InstallTrace, String> {
    import_trace_from_file(Path::new(&input_path), &state.current_storage().traces_dir())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_registry_backups(state: State<'_, AppState>) -> Vec<BackupEntry> {
    list_backups(&state.current_storage().backups_dir())
}

#[tauri::command]
pub fn preview_registry_backup(
    state: State<'_, AppState>,
    path: String,
) -> Result<String, String> {
    let backup_dir = state.current_storage().backups_dir();
    core_preview_backup(Path::new(&path), &backup_dir, 64_000).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restore_registry_backup(
    state: State<'_, AppState>,
    path: String,
) -> Result<(), String> {
    let storage = state.current_storage();
    let backup_dir = storage.backups_dir();
    let needs_elev =
        reg_backup_requires_elevation(Path::new(&path), &backup_dir).map_err(|e| e.to_string())?;

    let res = if needs_elev && !is_process_elevated() {
        state
            .elevation
            .execute(WorkerRequest::RestoreRegistryBackup { path: path.clone() })
            .map(|_| ())
    } else {
        match core_restore_backup(Path::new(&path)) {
            Ok(()) => Ok(()),
            Err(CoreError::ElevationRequired(_)) => state
                .elevation
                .execute(WorkerRequest::RestoreRegistryBackup { path: path.clone() })
                .map(|_| ()),
            Err(e) => Err(e),
        }
    };

    let filename = Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&path);
    let _ = storage.append_log(
        "restore_backup",
        filename,
        if res.is_ok() {
            "Imported .reg backup into Windows Registry"
        } else {
            "Failed to restore .reg backup"
        },
        res.is_ok(),
    );
    res.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_registry_backup(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let backup_dir = state.current_storage().backups_dir();
    delete_backup_file(Path::new(&path), &backup_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_operation_logs(state: State<'_, AppState>, limit: Option<usize>) -> Vec<OperationLogEntry> {
    state.current_storage().read_logs(limit.unwrap_or(200))
}

#[tauri::command]
pub fn clear_operation_logs(state: State<'_, AppState>) -> Result<(), String> {
    state
        .current_storage()
        .clear_logs()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_restore_point(
    state: State<'_, AppState>,
    description: String,
) -> Result<bool, String> {
    let storage = state.current_storage();
    let created = if is_process_elevated() {
        create_system_restore_point(&description).map_err(|e| e.to_string())?
    } else {
        let val = state
            .elevation
            .execute(WorkerRequest::CreateRestorePoint {
                description: description.clone(),
            })
            .map_err(|e| e.to_string())?;
        val.get("created").and_then(|v| v.as_bool()).unwrap_or(false)
    };
    let _ = storage.append_log(
        "restore_point",
        &description,
        "Created Windows System Restore Point",
        created,
    );
    Ok(created)
}

#[tauri::command]
pub fn open_backups_folder(state: State<'_, AppState>) -> Result<(), String> {
    let dir = state.current_storage().backups_dir();
    let _ = std::fs::create_dir_all(&dir);
    open_in_explorer(&dir.to_string_lossy()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_path_in_explorer(path: String) -> Result<(), String> {
    open_in_explorer(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_key_in_regedit(key_path: String) -> Result<(), String> {
    open_in_regedit(&key_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    open_url(&url).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_apps(
    state: State<'_, AppState>,
    format: ExportFormat,
    output_path: String,
    apps: Option<Vec<InstalledApp>>,
) -> Result<usize, String> {
    let list = match apps {
        Some(custom) => custom,
        None => state
            .apps_cache
            .lock()
            .map_err(|e| e.to_string())?
            .clone(),
    };
    export_apps_list(&list, format, Path::new(&output_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_shell_integration_status() -> ShellIntegrationStatus {
    core_shell_status()
}

#[tauri::command]
pub fn set_shell_integration(
    force_remove: bool,
    track_install: bool,
    locale: String,
) -> Result<ShellIntegrationStatus, String> {
    core_set_shell_integration(force_remove, track_install, &locale).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_for_updates() -> Result<UpdateCheckResult, String> {
    let cur = env!("CARGO_PKG_VERSION").to_string();
    tauri::async_runtime::spawn_blocking(move || check_github_release(&cur))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn take_pending_cli_action(state: State<'_, AppState>) -> Option<CliAction> {
    state
        .pending_cli_action
        .lock()
        .ok()
        .and_then(|mut g| g.take())
}

#[tauri::command]
pub async fn list_portable_apps(
    state: State<'_, AppState>,
    roots: Vec<String>,
) -> Result<Vec<PortableApp>, String> {
    let apps = tauri::async_runtime::spawn_blocking(move || core_scan_portable_apps(&roots))
        .await
        .map_err(|e| e.to_string())?;
    if let Ok(mut guard) = state.portable_cache.lock() {
        *guard = apps.clone();
    }
    Ok(apps)
}

#[tauri::command]
pub async fn get_portable_app_icons(
    state: State<'_, AppState>,
    app_ids: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let targets: Vec<PortableApp> = {
        let guard = state.portable_cache.lock().map_err(|e| e.to_string())?;
        let id_set: std::collections::HashSet<&str> = app_ids.iter().map(|s| s.as_str()).collect();
        guard
            .iter()
            .filter(|a| id_set.contains(a.id.as_str()))
            .cloned()
            .collect()
    };

    let pairs: Vec<(String, Option<String>)> = targets
        .par_iter()
        .map(|app| {
            let uri = state
                .icon_cache
                .get_or_extract_for_path(&app.id, Path::new(&app.exe_path));
            (app.id.clone(), uri)
        })
        .collect();

    let mut out = HashMap::with_capacity(pairs.len());
    for (id, uri_opt) in pairs {
        if let Some(uri) = uri_opt {
            out.insert(id, uri);
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn launch_portable_app(exe_path: String, working_dir: String) -> Result<(), String> {
    core_launch_portable_app(Path::new(&exe_path), Path::new(&working_dir))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_portable_shortcut(
    state: State<'_, AppState>,
    exe_path: String,
    name: String,
    location: ShortcutLocation,
) -> Result<String, String> {
    let storage = state.current_storage();
    let res = core_create_portable_shortcut(Path::new(&exe_path), &name, location);
    let loc_str = match location {
        ShortcutLocation::Desktop => "Desktop",
        ShortcutLocation::StartMenu => "Start Menu",
    };
    let _ = storage.append_log(
        "portable_shortcut",
        &name,
        &format!("Created {} shortcut", loc_str),
        res.is_ok(),
    );
    res.map(|p| p.to_string_lossy().to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_winget_updates(
    state: State<'_, AppState>,
) -> Result<WingetCheckResult, String> {
    let apps_snapshot: Vec<InstalledApp> = {
        let guard = state.apps_cache.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    tauri::async_runtime::spawn_blocking(move || core_check_winget_updates(&apps_snapshot))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_winget_upgrade(
    state: State<'_, AppState>,
    package_id: String,
    app_name: String,
) -> Result<WingetUpgradeResult, String> {
    let storage = state.current_storage();
    let pid_clone = package_id.clone();
    let name_clone = app_name.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        core_run_winget_upgrade(&pid_clone, &name_clone)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    let _ = storage.append_log(
        "winget_upgrade",
        &app_name,
        &format!("{} -> {}", package_id, res.message),
        res.success,
    );
    Ok(res)
}
