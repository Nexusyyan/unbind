use crate::elevation::ElevationBridge;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use unbind_core::discovery::icon::IconCache;
use unbind_core::discovery::{discover_all_apps, open_in_explorer, open_in_regedit, open_url};
use unbind_core::export::export_apps_list;
use unbind_core::ipc::WorkerRequest;
use unbind_core::models::{
    ActiveTrackSession, AppKind, AppSettings, BackupEntry, CleanupOptions, CleanupReport,
    CoreError, ExportFormat, InstallScope, InstallTrace, InstalledApp, LeftoverItem,
    LockingProcessInfo, OperationLogEntry, StartupItem, SystemStatus, UninstallExecutionResult,
};
use unbind_core::portable::AppStorage;
use unbind_core::safety::{create_system_restore_point, is_process_elevated, list_backups};
use unbind_core::startup::{
    delete_startup_item as core_delete_startup, list_startup_items as core_list_startup,
    set_startup_enabled as core_set_startup_enabled,
};
use unbind_core::tracker::{
    delete_trace_file, load_all_traces, load_trace_by_id, trace_to_leftover_items, TrackerManager,
};
use unbind_core::uninstaller::leftover::{build_force_target_from_path, scan_leftovers};
use unbind_core::uninstaller::{execute_cleanup, run_native_uninstaller};
use unbind_core::unlocker::{find_locking_processes, terminate_processes};
use tauri::State;

pub struct AppState {
    pub storage: Mutex<AppStorage>,
    pub apps_cache: Mutex<Vec<InstalledApp>>,
    pub icon_cache: IconCache,
    pub tracker: TrackerManager,
    pub elevation: ElevationBridge,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            storage: Mutex::new(AppStorage::detect()),
            apps_cache: Mutex::new(Vec::new()),
            icon_cache: IconCache::new(),
            tracker: TrackerManager::new(),
            elevation: ElevationBridge::new(),
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
pub fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<AppSettings, String> {
    let storage = state.current_storage();
    let updated = storage.save_settings(&settings).map_err(|e| e.to_string())?;
    // Refresh in-memory AppStorage path after portable toggle.
    if let Ok(mut guard) = state.storage.lock() {
        *guard = AppStorage::for_dir(PathBuf::from(&updated.data_dir), updated.portable_mode);
    }
    Ok(updated)
}

#[tauri::command]
pub async fn list_apps(state: State<'_, AppState>) -> Result<Vec<InstalledApp>, String> {
    let traces_dir = state.current_storage().traces_dir();
    let apps = tauri::async_runtime::spawn_blocking(move || discover_all_apps(&traces_dir))
        .await
        .map_err(|e| e.to_string())?;

    if let Ok(mut guard) = state.apps_cache.lock() {
        *guard = apps.clone();
    }
    Ok(apps)
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
pub async fn elevate_session(state: State<'_, AppState>) -> Result<bool, String> {
    state.elevation.ensure_connected().map_err(|e| e.to_string())?;
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
        let target = build_force_target_from_path(std::path::Path::new(&path));
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
        // Try locally first; if any fail, escalate to elevated worker.
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
    state: State<'_, AppState>,
    app_name: String,
    installer_path: String,
    launch_installer: bool,
) -> Result<ActiveTrackSession, String> {
    state
        .tracker
        .start_session(&app_name, &installer_path, launch_installer)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn finish_install_track(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<InstallTrace, String> {
    let storage = state.current_storage();
    let trace = state
        .tracker
        .finish_session(&session_id, &storage.traces_dir())
        .map_err(|e| e.to_string())?;
    let summary = format!(
        "Recorded {} dirs, {} files, {} reg keys, {} reg values",
        trace.added_dirs.len(),
        trace.added_files.len(),
        trace.added_registry_keys.len(),
        trace.added_registry_values.len()
    );
    let _ = storage.append_log("install_trace", &trace.app_name, &summary, true);
    Ok(trace)
}

#[tauri::command]
pub fn cancel_install_track(state: State<'_, AppState>, session_id: String) -> bool {
    state.tracker.cancel_session(&session_id)
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
pub fn list_registry_backups(state: State<'_, AppState>) -> Vec<BackupEntry> {
    list_backups(&state.current_storage().backups_dir())
}

#[tauri::command]
pub fn list_operation_logs(state: State<'_, AppState>, limit: Option<usize>) -> Vec<OperationLogEntry> {
    state.current_storage().read_logs(limit.unwrap_or(200))
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
    let _ = storage.append_log("restore_point", &description, "Created Windows System Restore Point", created);
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
    export_apps_list(&list, format, std::path::Path::new(&output_path)).map_err(|e| e.to_string())
}
