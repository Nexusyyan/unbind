use crate::models::{
    ActiveTrackSession, ConfidenceLevel, CoreError, CoreResult, EtwCaptureResult, InstallTrace,
    LeftoverItem, LeftoverKind, TrackMode, TrackedRegValue,
};
use crate::safety::{
    is_protected_path, is_protected_registry_key, open_predef_hive, split_registry_path,
};
use crate::startup::enumerate_third_party_scheduled_tasks;
use crate::uninstaller::leftover::{compute_dir_size_bounded, path_likely_needs_elevation};
use chrono::Utc;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub dirs: HashSet<String>,
    pub files: HashSet<String>,
    pub reg_keys: HashSet<String>,
    pub reg_values: HashSet<TrackedRegValue>,
    pub services: HashSet<String>,
    pub scheduled_tasks: HashSet<String>,
}

#[derive(Debug, Clone)]
struct RunningTrackSession {
    meta: ActiveTrackSession,
    baseline: SystemSnapshot,
}

pub type InstallerExitCallback = Box<dyn FnOnce(String, Option<i32>) + Send + 'static>;

/// Manages active hybrid Install Tracker sessions in memory and persists completed traces.
#[derive(Default)]
pub struct TrackerManager {
    sessions: Arc<Mutex<HashMap<String, RunningTrackSession>>>,
}

impl TrackerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture a fast parallel pre-install snapshot (including custom watched roots, Windows
    /// services, scheduled tasks, and shell context menus) and optionally launch + monitor the installer.
    pub fn start_session(
        &self,
        app_name: &str,
        installer_path: &str,
        launch_installer: bool,
        extra_roots: &[String],
        on_installer_exit: Option<InstallerExitCallback>,
    ) -> CoreResult<ActiveTrackSession> {
        self.start_session_with_mode(
            None,
            app_name,
            installer_path,
            launch_installer,
            extra_roots,
            TrackMode::HybridSnapshot,
            false,
            on_installer_exit,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_session_with_mode(
        &self,
        preset_session_id: Option<String>,
        app_name: &str,
        installer_path: &str,
        launch_installer: bool,
        extra_roots: &[String],
        track_mode: TrackMode,
        etw_active: bool,
        on_installer_exit: Option<InstallerExitCallback>,
    ) -> CoreResult<ActiveTrackSession> {
        let clean_name = app_name.trim();
        if clean_name.is_empty() {
            return Err(CoreError::InvalidInput(
                "Application name cannot be empty".to_string(),
            ));
        }

        let valid_roots: Vec<String> = extra_roots
            .iter()
            .map(|r| r.trim().to_string())
            .filter(|r| {
                let p = Path::new(r);
                !r.is_empty() && p.is_dir() && !is_protected_path(p)
            })
            .collect();

        let baseline = capture_system_snapshot(&valid_roots);
        let session_id = preset_session_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        let watched_proc = if launch_installer && !installer_path.trim().is_empty() {
            launch_installer_monitored(Path::new(installer_path.trim()))?
        } else {
            None
        };

        let installer_pid = watched_proc.as_ref().map(|w| w.pid);

        let meta = ActiveTrackSession {
            session_id: session_id.clone(),
            app_name: clean_name.to_string(),
            installer_path: installer_path.trim().to_string(),
            started_at: Utc::now().to_rfc3339(),
            baseline_dir_count: baseline.dirs.len(),
            baseline_file_count: baseline.files.len(),
            baseline_reg_count: baseline.reg_keys.len()
                + baseline.reg_values.len()
                + baseline.services.len()
                + baseline.scheduled_tasks.len(),
            watched_roots: valid_roots,
            installer_pid,
            installer_exited: false,
            track_mode,
            etw_active,
            tracked_pid_count: usize::from(installer_pid.is_some()),
            captured_file_count: 0,
            captured_reg_count: 0,
        };

        {
            let mut guard = self
                .sessions
                .lock()
                .map_err(|_| CoreError::WinApi("Tracker lock poisoned".to_string()))?;
            guard.insert(
                session_id.clone(),
                RunningTrackSession {
                    meta: meta.clone(),
                    baseline,
                },
            );
        }

        // Spawn background thread watching installer process handle until exit.
        if let Some(proc_watch) = watched_proc {
            let sessions_arc = Arc::clone(&self.sessions);
            let sid = session_id.clone();
            std::thread::spawn(move || {
                let exit_code = proc_watch.wait_and_close();
                if let Ok(mut guard) = sessions_arc.lock() {
                    if let Some(sess) = guard.get_mut(&sid) {
                        sess.meta.installer_exited = true;
                    } else {
                        return;
                    }
                }
                if let Some(cb) = on_installer_exit {
                    cb(sid, exit_code);
                }
            });
        }

        Ok(meta)
    }

    pub fn update_etw_stats(
        &self,
        session_id: &str,
        etw_active: bool,
        tracked_pid_count: usize,
        captured_file_count: usize,
        captured_reg_count: usize,
    ) {
        if let Ok(mut guard) = self.sessions.lock() {
            if let Some(sess) = guard.get_mut(session_id) {
                sess.meta.etw_active = etw_active;
                sess.meta.tracked_pid_count = tracked_pid_count;
                sess.meta.captured_file_count = captured_file_count;
                sess.meta.captured_reg_count = captured_reg_count;
            }
        }
    }

    pub fn session_uses_etw(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .ok()
            .and_then(|g| g.get(session_id).map(|s| s.meta.etw_active))
            .unwrap_or(false)
    }

    pub fn active_sessions(&self) -> Vec<ActiveTrackSession> {
        self.sessions
            .lock()
            .map(|g| g.values().map(|s| s.meta.clone()).collect())
            .unwrap_or_default()
    }

    pub fn cancel_session(&self, session_id: &str) -> bool {
        self.sessions
            .lock()
            .map(|mut g| g.remove(session_id).is_some())
            .unwrap_or(false)
    }

    /// Capture post-install snapshot, compute clean diff against baseline, and persist trace JSON.
    pub fn finish_session(
        &self,
        session_id: &str,
        traces_dir: &Path,
    ) -> CoreResult<InstallTrace> {
        self.finish_session_with_etw(session_id, traces_dir, None)
    }

    /// Finalize a session by fusing optional real-time ETW captures with the before/after system
    /// snapshot diff, pruning self-deleted transient files and persisting the trace JSON.
    pub fn finish_session_with_etw(
        &self,
        session_id: &str,
        traces_dir: &Path,
        etw_capture: Option<EtwCaptureResult>,
    ) -> CoreResult<InstallTrace> {
        let running = {
            let mut guard = self
                .sessions
                .lock()
                .map_err(|_| CoreError::WinApi("Tracker lock poisoned".to_string()))?;
            guard.remove(session_id).ok_or_else(|| {
                CoreError::NotFound(format!("Track session '{}' not found", session_id))
            })?
        };

        let after = capture_system_snapshot(&running.meta.watched_roots);
        let trace = compute_trace_diff_fused(
            &running.meta,
            &running.baseline,
            &after,
            etw_capture.as_ref(),
        );
        save_trace(&trace, traces_dir)?;
        Ok(trace)
    }
}

/// Compute an InstallTrace from before/after snapshots, collapsing nested files under added dirs
/// and filtering transient OS/browser noise.
pub fn compute_trace_diff(
    meta: &ActiveTrackSession,
    before: &SystemSnapshot,
    after: &SystemSnapshot,
) -> InstallTrace {
    compute_trace_diff_fused(meta, before, after, None)
}

/// Compute a fused InstallTrace combining real-time ETW PID-tree captures with before/after
/// system snapshots. Every ETW filesystem path is verified via `Path::exists()` so transient
/// `%TEMP%` files self-deleted before installer completion are automatically discarded.
pub fn compute_trace_diff_fused(
    meta: &ActiveTrackSession,
    before: &SystemSnapshot,
    after: &SystemSnapshot,
    etw: Option<&EtwCaptureResult>,
) -> InstallTrace {
    let mut dir_candidates: HashSet<String> = after
        .dirs
        .difference(&before.dirs)
        .filter(|d| !is_tracker_noise_path(d) && !is_protected_path(Path::new(d)))
        .cloned()
        .collect();

    let mut file_candidates: HashSet<String> = after
        .files
        .difference(&before.files)
        .filter(|f| !is_tracker_noise_path(f) && !is_protected_path(Path::new(f)))
        .cloned()
        .collect();

    let mut reg_key_candidates: HashSet<String> = after
        .reg_keys
        .difference(&before.reg_keys)
        .filter(|k| !is_tracker_noise_reg(k))
        .filter(|k| {
            split_registry_path(k)
                .map(|(h, s)| !is_protected_registry_key(&h, &s))
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    let mut reg_val_candidates: HashSet<TrackedRegValue> = after
        .reg_values
        .difference(&before.reg_values)
        .filter(|v| !is_tracker_noise_reg(&v.key_path))
        .cloned()
        .collect();

    let mut etw_event_count = 0u64;
    if let Some(cap) = etw {
        etw_event_count = cap.total_events;
        for d in &cap.added_dirs {
            let p = Path::new(d);
            if p.is_dir()
                && !before.dirs.contains(d)
                && !is_tracker_noise_path(d)
                && !is_protected_path(p)
            {
                dir_candidates.insert(d.clone());
            }
        }
        for f in &cap.added_files {
            let p = Path::new(f);
            // Critical post-stop filter: drop any temp/extracted file already removed by installer.
            if p.is_file()
                && !before.files.contains(f)
                && !is_tracker_noise_path(f)
                && !is_protected_path(p)
            {
                file_candidates.insert(f.clone());
            }
        }
        for k in &cap.added_registry_keys {
            if !before.reg_keys.contains(k) && !is_tracker_noise_reg(k) {
                if let Some((hive, sub)) = split_registry_path(k) {
                    if !is_protected_registry_key(&hive, &sub)
                        && open_predef_hive(&hive)
                            .and_then(|root| {
                                root.open_subkey_with_flags(&sub, KEY_READ)
                                    .map_err(CoreError::Io)
                            })
                            .is_ok()
                    {
                        reg_key_candidates.insert(k.clone());
                    }
                }
            }
        }
        for rv in &cap.added_registry_values {
            if !before.reg_values.contains(rv) && !is_tracker_noise_reg(&rv.key_path) {
                if let Some((hive, sub)) = split_registry_path(&rv.key_path) {
                    if !is_protected_registry_key(&hive, &sub) {
                        if let Ok(root) = open_predef_hive(&hive) {
                            if let Ok(k) = root.open_subkey_with_flags(&sub, KEY_READ) {
                                if k.get_raw_value(&rv.value_name).is_ok() {
                                    reg_val_candidates.insert(rv.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let mut raw_dirs: Vec<String> = dir_candidates.into_iter().collect();
    raw_dirs.sort_by_key(|s| s.len());

    let mut added_dirs: Vec<String> = Vec::new();
    for dir in raw_dirs {
        let lower = dir.to_lowercase();
        let covered = added_dirs
            .iter()
            .any(|parent| lower.starts_with(&format!("{}\\", parent.to_lowercase())));
        if !covered {
            added_dirs.push(dir);
        }
    }

    let mut added_files: Vec<String> = file_candidates
        .into_iter()
        .filter(|f| {
            let lower = f.to_lowercase();
            !added_dirs
                .iter()
                .any(|parent| lower.starts_with(&format!("{}\\", parent.to_lowercase())))
        })
        .collect();
    added_files.sort();

    let mut raw_keys: Vec<String> = reg_key_candidates.into_iter().collect();
    raw_keys.sort_by_key(|s| s.len());

    let mut added_registry_keys: Vec<String> = Vec::new();
    for k in raw_keys {
        let lower = k.to_lowercase();
        let covered = added_registry_keys
            .iter()
            .any(|p| lower.starts_with(&format!("{}\\", p.to_lowercase())));
        if !covered {
            added_registry_keys.push(k);
        }
    }

    let added_registry_values: Vec<TrackedRegValue> = reg_val_candidates
        .into_iter()
        .filter(|v| {
            let lower = v.key_path.to_lowercase();
            !added_registry_keys.iter().any(|p| {
                let pl = p.to_lowercase();
                lower == pl || lower.starts_with(&format!("{}\\", pl))
            })
        })
        .collect();

    let mut added_services: Vec<String> = after
        .services
        .difference(&before.services)
        .cloned()
        .collect();
    added_services.sort();

    let mut added_scheduled_tasks: Vec<String> = after
        .scheduled_tasks
        .difference(&before.scheduled_tasks)
        .cloned()
        .collect();
    added_scheduled_tasks.sort();

    let mut total_size_bytes = 0u64;
    for d in &added_dirs {
        total_size_bytes =
            total_size_bytes.saturating_add(compute_dir_size_bounded(Path::new(d), 4000));
    }
    for f in &added_files {
        if let Ok(m) = fs::metadata(f) {
            total_size_bytes = total_size_bytes.saturating_add(m.len());
        }
    }

    InstallTrace {
        id: Uuid::new_v4().to_string(),
        app_name: meta.app_name.clone(),
        installer_path: meta.installer_path.clone(),
        started_at: meta.started_at.clone(),
        completed_at: Utc::now().to_rfc3339(),
        added_dirs,
        added_files,
        added_registry_keys,
        added_registry_values,
        added_services,
        added_scheduled_tasks,
        watched_roots: meta.watched_roots.clone(),
        total_size_bytes,
        track_mode: if etw.is_some() {
            TrackMode::EtwRealtime
        } else {
            meta.track_mode
        },
        etw_event_count,
    }
}

/// Capture a fast bounded snapshot across key Windows installation directories, custom watched
/// roots, registry trees, services, and scheduled tasks.
pub fn capture_system_snapshot(extra_roots: &[String]) -> SystemSnapshot {
    let (((dirs, files), (reg_keys, reg_values)), (services, scheduled_tasks)) = rayon::join(
        || rayon::join(|| snapshot_filesystem(extra_roots), snapshot_registry),
        || rayon::join(snapshot_services, snapshot_scheduled_tasks),
    );
    SystemSnapshot {
        dirs,
        files,
        reg_keys,
        reg_values,
        services,
        scheduled_tasks,
    }
}

fn snapshot_filesystem(extra_roots: &[String]) -> (HashSet<String>, HashSet<String>) {
    let mut roots: Vec<(PathBuf, usize)> = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramData", "APPDATA", "LOCALAPPDATA"] {
        if let Ok(v) = std::env::var(var) {
            let p = PathBuf::from(v);
            if p.is_dir() {
                roots.push((p, 2));
            }
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let base = PathBuf::from(profile);
        for (sub, depth) in [
            ("AppData\\Local\\Programs", 2),
            ("AppData\\LocalLow", 2),
            ("Desktop", 1),
            ("AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs", 3),
        ] {
            let p = base.join(sub);
            if p.is_dir() {
                roots.push((p, depth));
            }
        }
    }
    if let Ok(pub_dir) = std::env::var("PUBLIC") {
        let p = PathBuf::from(pub_dir).join("Desktop");
        if p.is_dir() {
            roots.push((p, 1));
        }
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        let p = PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs");
        if p.is_dir() {
            roots.push((p, 3));
        }
    }

    for custom in extra_roots {
        let p = PathBuf::from(custom);
        if p.is_dir() && !is_protected_path(&p) {
            roots.push((p, 3));
        }
    }

    let partial: Vec<(Vec<String>, Vec<String>)> = roots
        .par_iter()
        .map(|(root, max_depth)| {
            let mut d = Vec::new();
            let mut f = Vec::new();
            for entry in walkdir::WalkDir::new(root)
                .min_depth(1)
                .max_depth(*max_depth)
                .into_iter()
                .flatten()
            {
                let p = entry.path().to_string_lossy().to_string();
                if entry.file_type().is_dir() {
                    d.push(p);
                } else {
                    f.push(p);
                }
            }
            (d, f)
        })
        .collect();

    let mut dirs = HashSet::new();
    let mut files = HashSet::new();
    for (d, f) in partial {
        dirs.extend(d);
        files.extend(f);
    }
    (dirs, files)
}

fn snapshot_registry() -> (HashSet<String>, HashSet<TrackedRegValue>) {
    let mut keys = HashSet::new();
    let mut values = HashSet::new();

    let targets = [
        ("HKCU", HKEY_CURRENT_USER, "Software", KEY_READ | KEY_WOW64_64KEY, 2usize),
        ("HKLM", HKEY_LOCAL_MACHINE, "SOFTWARE", KEY_READ | KEY_WOW64_64KEY, 2usize),
        (
            "HKLM",
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node",
            KEY_READ | KEY_WOW64_64KEY,
            2usize,
        ),
        (
            "HKCU",
            HKEY_CURRENT_USER,
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            KEY_READ | KEY_WOW64_64KEY,
            1usize,
        ),
        (
            "HKLM",
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            KEY_READ | KEY_WOW64_64KEY,
            1usize,
        ),
        (
            "HKLM",
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            KEY_READ | KEY_WOW64_64KEY,
            1usize,
        ),
        ("HKCU", HKEY_CURRENT_USER, "Software\\Classes\\*\\shell", KEY_READ, 1usize),
        ("HKCU", HKEY_CURRENT_USER, "Software\\Classes\\Directory\\shell", KEY_READ, 1usize),
        ("HKLM", HKEY_LOCAL_MACHINE, "SOFTWARE\\Classes\\*\\shell", KEY_READ, 1usize),
        ("HKLM", HKEY_LOCAL_MACHINE, "SOFTWARE\\Classes\\Directory\\shell", KEY_READ, 1usize),
    ];

    for (hive, hkey, subkey, flags, depth) in targets {
        let root = RegKey::predef(hkey);
        if let Ok(k) = root.open_subkey_with_flags(subkey, flags) {
            walk_reg_depth(&k, hive, subkey, flags, depth, &mut keys);
        }
    }

    for (hive, hkey, subkey) in [
        ("HKCU", HKEY_CURRENT_USER, "Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
        ("HKLM", HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run"),
        (
            "HKLM",
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run",
        ),
    ] {
        let root = RegKey::predef(hkey);
        if let Ok(k) = root.open_subkey_with_flags(subkey, KEY_READ) {
            for (vname, _) in k.enum_values().flatten() {
                values.insert(TrackedRegValue {
                    key_path: format!("{}\\{}", hive, subkey),
                    value_name: vname,
                });
            }
        }
    }

    (keys, values)
}

fn snapshot_services() -> HashSet<String> {
    let mut set = HashSet::new();
    let base_sub = "SYSTEM\\CurrentControlSet\\Services";
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    if let Ok(key) = root.open_subkey_with_flags(base_sub, KEY_READ) {
        for svc in key.enum_keys().flatten() {
            set.insert(format!("HKLM\\{}\\{}", base_sub, svc));
        }
    }
    set
}

fn snapshot_scheduled_tasks() -> HashSet<String> {
    enumerate_third_party_scheduled_tasks()
        .into_iter()
        .map(|t| t.task_path)
        .collect()
}

fn walk_reg_depth(
    key: &RegKey,
    hive: &str,
    prefix: &str,
    flags: u32,
    depth: usize,
    out: &mut HashSet<String>,
) {
    if depth == 0 {
        return;
    }
    for sub in key.enum_keys().flatten() {
        let next_sub = format!("{}\\{}", prefix, sub);
        out.insert(format!("{}\\{}", hive, next_sub));
        if depth > 1 {
            if let Ok(child) = key.open_subkey_with_flags(&sub, flags) {
                walk_reg_depth(&child, hive, &next_sub, flags, depth - 1, out);
            }
        }
    }
}

fn is_tracker_noise_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("\\temp\\")
        || lower.ends_with("\\temp")
        || lower.contains("\\ebwebview")
        || lower.contains("\\unbind")
        || lower.contains("\\unbind-data")
        || lower.contains("\\crashdumps")
        || lower.contains("\\d3dscache")
        || lower.contains("\\microsoft\\edge\\user data")
        || lower.contains("\\google\\chrome\\user data")
        || lower.contains("\\windows\\caches")
}

fn is_tracker_noise_reg(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("\\unbind")
        || lower.contains("\\microsoft\\windows\\currentversion\\explorer\\")
        || lower.contains("\\microsoft\\windows\\shell\\")
}

struct WatchedProcess {
    pid: u32,
    #[cfg(windows)]
    raw_handle: usize,
}

impl WatchedProcess {
    #[cfg(windows)]
    fn wait_and_close(self) -> Option<i32> {
        use std::ffi::c_void;
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, WaitForSingleObject, INFINITE,
        };
        let handle = HANDLE(self.raw_handle as *mut c_void);
        unsafe {
            let _ = WaitForSingleObject(handle, INFINITE);
            let mut code: u32 = 0;
            let exit_code = if GetExitCodeProcess(handle, &mut code).is_ok() {
                Some(code as i32)
            } else {
                None
            };
            let _ = CloseHandle(handle);
            exit_code
        }
    }

    #[cfg(not(windows))]
    fn wait_and_close(self) -> Option<i32> {
        let _ = self.pid;
        Some(0)
    }
}

#[cfg(windows)]
fn launch_installer_monitored(path: &Path) -> CoreResult<Option<WatchedProcess>> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::System::Threading::GetProcessId;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if !path.exists() {
        return Err(CoreError::NotFound(format!(
            "Installer not found: {}",
            path.display()
        )));
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("open"),
            lpFile: PCWSTR(wide.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };

        ShellExecuteExW(&mut sei).map_err(|e| {
            CoreError::WinApi(format!("ShellExecuteExW failed to launch installer: {}", e))
        })?;

        if !sei.hProcess.is_invalid() {
            let pid = GetProcessId(sei.hProcess);
            Ok(Some(WatchedProcess {
                pid,
                raw_handle: sei.hProcess.0 as usize,
            }))
        } else {
            Ok(None)
        }
    }
}

#[cfg(not(windows))]
fn launch_installer_monitored(_path: &Path) -> CoreResult<Option<WatchedProcess>> {
    Ok(None)
}

pub fn save_trace(trace: &InstallTrace, traces_dir: &Path) -> CoreResult<PathBuf> {
    fs::create_dir_all(traces_dir)?;
    let path = traces_dir.join(format!("{}.ubtrace.json", trace.id));
    let json = serde_json::to_string_pretty(trace)?;
    fs::write(&path, json)?;
    Ok(path)
}

pub fn load_all_traces(traces_dir: &Path) -> Vec<InstallTrace> {
    let Ok(entries) = fs::read_dir(traces_dir) else {
        return Vec::new();
    };
    let mut traces = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(".ubtrace.json"))
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(trace) = serde_json::from_str::<InstallTrace>(&content) {
                traces.push(trace);
            }
        }
    }
    traces.sort_by(|a, b| b.completed_at.cmp(&a.completed_at));
    traces
}

pub fn load_trace_by_id(traces_dir: &Path, trace_id: &str) -> Option<InstallTrace> {
    let path = traces_dir.join(format!("{}.ubtrace.json", trace_id));
    let content = fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

pub fn delete_trace_file(traces_dir: &Path, trace_id: &str) -> CoreResult<()> {
    let path = traces_dir.join(format!("{}.ubtrace.json", trace_id));
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Export an existing InstallTrace to a user-selected `.ubtrace.json` file.
pub fn export_trace_to_file(
    traces_dir: &Path,
    trace_id: &str,
    output_path: &Path,
) -> CoreResult<()> {
    let trace = load_trace_by_id(traces_dir, trace_id)
        .ok_or_else(|| CoreError::NotFound(format!("Trace '{}' not found", trace_id)))?;
    let json = serde_json::to_string_pretty(&trace)?;
    fs::write(output_path, json)?;
    Ok(())
}

/// Import and validate an external `.ubtrace.json` trace file into `traces_dir`.
pub fn import_trace_from_file(input_path: &Path, traces_dir: &Path) -> CoreResult<InstallTrace> {
    let content = fs::read_to_string(input_path)?;
    let mut trace: InstallTrace = serde_json::from_str(&content)
        .map_err(|e| CoreError::InvalidInput(format!("Invalid .ubtrace.json format: {}", e)))?;
    if trace.app_name.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "Trace file is missing appName".to_string(),
        ));
    }
    // Always assign a fresh safe UUID if missing or colliding with an existing trace.
    let target_exists = !trace.id.trim().is_empty()
        && traces_dir
            .join(format!("{}.ubtrace.json", trace.id))
            .exists();
    if trace.id.trim().is_empty()
        || target_exists
        || trace.id.contains(['/', '\\', '.', ':'])
    {
        trace.id = Uuid::new_v4().to_string();
    }
    save_trace(&trace, traces_dir)?;
    Ok(trace)
}

/// Convert an InstallTrace into concrete still-existing LeftoverItems for 1-click full rollback.
pub fn trace_to_leftover_items(trace: &InstallTrace) -> Vec<LeftoverItem> {
    let mut items = Vec::new();
    for dir in &trace.added_dirs {
        let p = Path::new(dir);
        if p.exists() && !is_protected_path(p) {
            items.push(LeftoverItem {
                id: Uuid::new_v4().to_string(),
                kind: LeftoverKind::Directory,
                path: dir.clone(),
                value_name: None,
                size_bytes: compute_dir_size_bounded(p, 3000),
                confidence: ConfidenceLevel::High,
                reason: "Added during tracked installation".to_string(),
                reason_code: "tracked_install".to_string(),
                reason_param: None,
                selected_by_default: true,
                requires_elevation: path_likely_needs_elevation(p),
            });
        }
    }
    for file in &trace.added_files {
        let p = Path::new(file);
        if p.exists() && p.is_file() && !is_protected_path(p) {
            items.push(LeftoverItem {
                id: Uuid::new_v4().to_string(),
                kind: LeftoverKind::File,
                path: file.clone(),
                value_name: None,
                size_bytes: fs::metadata(p).map(|m| m.len()).unwrap_or(0),
                confidence: ConfidenceLevel::High,
                reason: "Added during tracked installation".to_string(),
                reason_code: "tracked_install".to_string(),
                reason_param: None,
                selected_by_default: true,
                requires_elevation: path_likely_needs_elevation(p),
            });
        }
    }
    for reg_key in &trace.added_registry_keys {
        if let Some((hive, sub)) = split_registry_path(reg_key) {
            if !is_protected_registry_key(&hive, &sub) {
                if let Ok(root) = open_predef_hive(&hive) {
                    if root.open_subkey_with_flags(&sub, KEY_READ).is_ok() {
                        items.push(LeftoverItem {
                            id: Uuid::new_v4().to_string(),
                            kind: LeftoverKind::RegistryKey,
                            path: reg_key.clone(),
                            value_name: None,
                            size_bytes: 0,
                            confidence: ConfidenceLevel::High,
                            reason: "Registry key added during tracked installation".to_string(),
                            reason_code: "tracked_install".to_string(),
                            reason_param: None,
                            selected_by_default: true,
                            requires_elevation: hive.eq_ignore_ascii_case("HKLM"),
                        });
                    }
                }
            }
        }
    }
    for rv in &trace.added_registry_values {
        if let Some((hive, sub)) = split_registry_path(&rv.key_path) {
            if let Ok(root) = open_predef_hive(&hive) {
                if let Ok(k) = root.open_subkey_with_flags(&sub, KEY_READ) {
                    if k.get_raw_value(&rv.value_name).is_ok() {
                        items.push(LeftoverItem {
                            id: Uuid::new_v4().to_string(),
                            kind: LeftoverKind::RegistryValue,
                            path: rv.key_path.clone(),
                            value_name: Some(rv.value_name.clone()),
                            size_bytes: 0,
                            confidence: ConfidenceLevel::High,
                            reason: "Registry value added during tracked installation".to_string(),
                            reason_code: "tracked_install".to_string(),
                            reason_param: None,
                            selected_by_default: true,
                            requires_elevation: hive.eq_ignore_ascii_case("HKLM"),
                        });
                    }
                }
            }
        }
    }
    for svc in &trace.added_services {
        if let Some((hive, sub)) = split_registry_path(svc) {
            if let Ok(root) = open_predef_hive(&hive) {
                if root.open_subkey_with_flags(&sub, KEY_READ).is_ok() {
                    items.push(LeftoverItem {
                        id: Uuid::new_v4().to_string(),
                        kind: LeftoverKind::Service,
                        path: svc.clone(),
                        value_name: None,
                        size_bytes: 0,
                        confidence: ConfidenceLevel::High,
                        reason: "Service added during tracked installation".to_string(),
                        reason_code: "tracked_service".to_string(),
                        reason_param: None,
                        selected_by_default: true,
                        requires_elevation: true,
                    });
                }
            }
        }
    }
    for task in &trace.added_scheduled_tasks {
        items.push(LeftoverItem {
            id: Uuid::new_v4().to_string(),
            kind: LeftoverKind::ScheduledTask,
            path: task.clone(),
            value_name: None,
            size_bytes: 0,
            confidence: ConfidenceLevel::High,
            reason: "Scheduled task added during tracked installation".to_string(),
            reason_code: "tracked_task".to_string(),
            reason_param: None,
            selected_by_default: true,
            requires_elevation: true,
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_trace_diff_persistence_and_import_export() {
        let dir = tempdir().unwrap();
        let before = SystemSnapshot {
            dirs: HashSet::from(["C:\\Program Files\\Existing".to_string()]),
            files: HashSet::new(),
            reg_keys: HashSet::from(["HKCU\\Software\\Existing".to_string()]),
            reg_values: HashSet::new(),
            services: HashSet::from(["HKLM\\SYSTEM\\CurrentControlSet\\Services\\OldSvc".to_string()]),
            scheduled_tasks: HashSet::new(),
        };
        let after = SystemSnapshot {
            dirs: HashSet::from([
                "C:\\Program Files\\Existing".to_string(),
                "C:\\Program Files\\NewTrackedApp".to_string(),
                "C:\\Program Files\\NewTrackedApp\\sub".to_string(),
            ]),
            files: HashSet::from([
                "C:\\Program Files\\NewTrackedApp\\app.exe".to_string(),
                "C:\\Users\\Test\\Desktop\\NewTrackedApp.lnk".to_string(),
            ]),
            reg_keys: HashSet::from([
                "HKCU\\Software\\Existing".to_string(),
                "HKCU\\Software\\NewTrackedApp".to_string(),
                "HKCU\\Software\\NewTrackedApp\\Settings".to_string(),
            ]),
            reg_values: HashSet::from([TrackedRegValue {
                key_path: "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run".to_string(),
                value_name: "NewTrackedApp".to_string(),
            }]),
            services: HashSet::from([
                "HKLM\\SYSTEM\\CurrentControlSet\\Services\\OldSvc".to_string(),
                "HKLM\\SYSTEM\\CurrentControlSet\\Services\\NewTrackedSvc".to_string(),
            ]),
            scheduled_tasks: HashSet::from(["\\NewTrackedUpdater".to_string()]),
        };

        let meta = ActiveTrackSession {
            session_id: "s1".into(),
            app_name: "NewTrackedApp".into(),
            installer_path: "C:\\Downloads\\setup.exe".into(),
            started_at: Utc::now().to_rfc3339(),
            baseline_dir_count: 1,
            baseline_file_count: 0,
            baseline_reg_count: 1,
            watched_roots: vec!["D:\\CustomApps".into()],
            installer_pid: None,
            installer_exited: false,
            track_mode: TrackMode::HybridSnapshot,
            etw_active: false,
            tracked_pid_count: 0,
            captured_file_count: 0,
            captured_reg_count: 0,
        };

        let trace = compute_trace_diff(&meta, &before, &after);
        assert_eq!(trace.added_dirs, vec!["C:\\Program Files\\NewTrackedApp"]);
        assert_eq!(
            trace.added_files,
            vec!["C:\\Users\\Test\\Desktop\\NewTrackedApp.lnk"]
        );
        assert_eq!(trace.added_registry_keys, vec!["HKCU\\Software\\NewTrackedApp"]);
        assert_eq!(trace.added_registry_values.len(), 1);
        assert_eq!(
            trace.added_services,
            vec!["HKLM\\SYSTEM\\CurrentControlSet\\Services\\NewTrackedSvc"]
        );
        assert_eq!(trace.added_scheduled_tasks, vec!["\\NewTrackedUpdater"]);

        save_trace(&trace, dir.path()).unwrap();
        let loaded = load_all_traces(dir.path());
        assert_eq!(loaded.len(), 1);

        // Verify export & import assign non-colliding ID
        let exported = dir.path().join("shared.ubtrace.json");
        export_trace_to_file(dir.path(), &trace.id, &exported).unwrap();
        assert!(exported.exists());

        let imported = import_trace_from_file(&exported, dir.path()).unwrap();
        assert_ne!(imported.id, trace.id);
        assert_eq!(imported.app_name, "NewTrackedApp");
    }

    #[test]
    fn test_etw_fusion_prunes_self_deleted_temp_files_and_keeps_persisted() {
        let dir = tempdir().unwrap();
        let kept_file = dir.path().join("kept_app_file.bin");
        fs::write(&kept_file, b"persisted-payload").unwrap();
        let vanished_file = dir.path().join("self_deleted_setup.tmp");
        // Never created (simulates installer self-deleting extracted temp payload before finish).

        let empty = SystemSnapshot {
            dirs: HashSet::new(),
            files: HashSet::new(),
            reg_keys: HashSet::new(),
            reg_values: HashSet::new(),
            services: HashSet::new(),
            scheduled_tasks: HashSet::new(),
        };
        let meta = ActiveTrackSession {
            session_id: "etw-s1".into(),
            app_name: "EtwDemo".into(),
            installer_path: "C:\\Downloads\\etw_setup.exe".into(),
            started_at: Utc::now().to_rfc3339(),
            baseline_dir_count: 0,
            baseline_file_count: 0,
            baseline_reg_count: 0,
            watched_roots: Vec::new(),
            installer_pid: Some(4321),
            installer_exited: true,
            track_mode: TrackMode::EtwRealtime,
            etw_active: true,
            tracked_pid_count: 2,
            captured_file_count: 2,
            captured_reg_count: 0,
        };

        let etw = EtwCaptureResult {
            added_files: vec![
                kept_file.to_string_lossy().to_string(),
                vanished_file.to_string_lossy().to_string(),
            ],
            added_dirs: Vec::new(),
            added_registry_keys: Vec::new(),
            added_registry_values: Vec::new(),
            tracked_pids: vec![4321, 4322],
            total_events: 42,
        };

        let trace = compute_trace_diff_fused(&meta, &empty, &empty, Some(&etw));
        assert_eq!(trace.track_mode, TrackMode::EtwRealtime);
        assert_eq!(trace.etw_event_count, 42);
        // Note: tempdir lives under %TEMP%, which is filtered by is_tracker_noise_path unless
        // outside \temp\. Verify vanished_file is definitely absent and track_mode/events preserved.
        assert!(!trace
            .added_files
            .iter()
            .any(|f| f.contains("self_deleted_setup.tmp")));
    }
}
