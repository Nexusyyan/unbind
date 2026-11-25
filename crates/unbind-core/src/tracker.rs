use crate::models::{
    ActiveTrackSession, ConfidenceLevel, CoreError, CoreResult, InstallTrace, LeftoverItem,
    LeftoverKind, TrackedRegValue,
};
use crate::safety::{
    is_protected_path, is_protected_registry_key, open_predef_hive, split_registry_path,
};
use crate::uninstaller::leftover::{compute_dir_size_bounded, path_likely_needs_elevation};
use chrono::Utc;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use uuid::Uuid;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone)]
pub struct SystemSnapshot {
    pub dirs: HashSet<String>,
    pub files: HashSet<String>,
    pub reg_keys: HashSet<String>,
    pub reg_values: HashSet<TrackedRegValue>,
}

#[derive(Debug, Clone)]
struct RunningTrackSession {
    meta: ActiveTrackSession,
    baseline: SystemSnapshot,
}

/// Manages active Install Tracker snapshot sessions in memory and persists completed traces.
#[derive(Default)]
pub struct TrackerManager {
    sessions: Mutex<HashMap<String, RunningTrackSession>>,
}

impl TrackerManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Capture a fast parallel pre-install snapshot and optionally launch the installer executable.
    pub fn start_session(
        &self,
        app_name: &str,
        installer_path: &str,
        launch_installer: bool,
    ) -> CoreResult<ActiveTrackSession> {
        let clean_name = app_name.trim();
        if clean_name.is_empty() {
            return Err(CoreError::InvalidInput(
                "Application name cannot be empty".to_string(),
            ));
        }

        let baseline = capture_system_snapshot();
        let session_id = Uuid::new_v4().to_string();
        let meta = ActiveTrackSession {
            session_id: session_id.clone(),
            app_name: clean_name.to_string(),
            installer_path: installer_path.trim().to_string(),
            started_at: Utc::now().to_rfc3339(),
            baseline_dir_count: baseline.dirs.len(),
            baseline_file_count: baseline.files.len(),
            baseline_reg_count: baseline.reg_keys.len() + baseline.reg_values.len(),
        };

        if launch_installer && !installer_path.trim().is_empty() {
            launch_installer_file(Path::new(installer_path.trim()))?;
        }

        let mut guard = self
            .sessions
            .lock()
            .map_err(|_| CoreError::WinApi("Tracker lock poisoned".to_string()))?;
        guard.insert(
            session_id,
            RunningTrackSession {
                meta: meta.clone(),
                baseline,
            },
        );
        Ok(meta)
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
        let running = {
            let mut guard = self
                .sessions
                .lock()
                .map_err(|_| CoreError::WinApi("Tracker lock poisoned".to_string()))?;
            guard.remove(session_id).ok_or_else(|| {
                CoreError::NotFound(format!("Track session '{}' not found", session_id))
            })?
        };

        let after = capture_system_snapshot();
        let trace = compute_trace_diff(&running.meta, &running.baseline, &after);
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
    let mut raw_dirs: Vec<String> = after
        .dirs
        .difference(&before.dirs)
        .filter(|d| !is_tracker_noise_path(d) && !is_protected_path(Path::new(d)))
        .cloned()
        .collect();
    raw_dirs.sort_by_key(|s| s.len());

    // Keep top-most newly created directories so we don't list thousands of redundant child dirs.
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

    let added_files: Vec<String> = after
        .files
        .difference(&before.files)
        .filter(|f| !is_tracker_noise_path(f) && !is_protected_path(Path::new(f)))
        .filter(|f| {
            let lower = f.to_lowercase();
            !added_dirs
                .iter()
                .any(|parent| lower.starts_with(&format!("{}\\", parent.to_lowercase())))
        })
        .cloned()
        .collect();

    let mut raw_keys: Vec<String> = after
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

    let added_registry_values: Vec<TrackedRegValue> = after
        .reg_values
        .difference(&before.reg_values)
        .filter(|v| !is_tracker_noise_reg(&v.key_path))
        .filter(|v| {
            let lower = v.key_path.to_lowercase();
            !added_registry_keys.iter().any(|p| {
                let pl = p.to_lowercase();
                lower == pl || lower.starts_with(&format!("{}\\", pl))
            })
        })
        .cloned()
        .collect();

    let mut total_size_bytes = 0u64;
    for d in &added_dirs {
        total_size_bytes = total_size_bytes.saturating_add(compute_dir_size_bounded(Path::new(d), 4000));
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
        total_size_bytes,
    }
}

/// Capture a fast bounded snapshot across key Windows installation directories and registry trees.
pub fn capture_system_snapshot() -> SystemSnapshot {
    let ((dirs, files), (reg_keys, reg_values)) =
        rayon::join(snapshot_filesystem, snapshot_registry);
    SystemSnapshot {
        dirs,
        files,
        reg_keys,
        reg_values,
    }
}

fn snapshot_filesystem() -> (HashSet<String>, HashSet<String>) {
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
    ];

    for (hive, hkey, subkey, flags, depth) in targets {
        let root = RegKey::predef(hkey);
        if let Ok(k) = root.open_subkey_with_flags(subkey, flags) {
            walk_reg_depth(&k, hive, subkey, flags, depth, &mut keys);
        }
    }

    // Track startup Run values specifically.
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

#[cfg(windows)]
fn launch_installer_file(path: &Path) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if !path.exists() {
        return Err(CoreError::NotFound(format!(
            "Installer not found: {}",
            path.display()
        )));
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let res = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if (res.0 as isize) <= 32 {
            return Err(CoreError::WinApi(format!(
                "ShellExecuteW failed to launch installer (code {})",
                res.0 as isize
            )));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn launch_installer_file(_path: &Path) -> CoreResult<()> {
    Ok(())
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
                            selected_by_default: true,
                            requires_elevation: hive.eq_ignore_ascii_case("HKLM"),
                        });
                    }
                }
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_trace_diff_and_persistence() {
        let dir = tempdir().unwrap();
        let before = SystemSnapshot {
            dirs: HashSet::from(["C:\\Program Files\\Existing".to_string()]),
            files: HashSet::new(),
            reg_keys: HashSet::from(["HKCU\\Software\\Existing".to_string()]),
            reg_values: HashSet::new(),
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
        };

        let meta = ActiveTrackSession {
            session_id: "s1".into(),
            app_name: "NewTrackedApp".into(),
            installer_path: "C:\\Downloads\\setup.exe".into(),
            started_at: Utc::now().to_rfc3339(),
            baseline_dir_count: 1,
            baseline_file_count: 0,
            baseline_reg_count: 1,
        };

        let trace = compute_trace_diff(&meta, &before, &after);
        // Nested dir & nested file inside added_dir should collapse cleanly!
        assert_eq!(trace.added_dirs, vec!["C:\\Program Files\\NewTrackedApp"]);
        assert_eq!(
            trace.added_files,
            vec!["C:\\Users\\Test\\Desktop\\NewTrackedApp.lnk"]
        );
        assert_eq!(trace.added_registry_keys, vec!["HKCU\\Software\\NewTrackedApp"]);
        assert_eq!(trace.added_registry_values.len(), 1);

        save_trace(&trace, dir.path()).unwrap();
        let loaded = load_all_traces(dir.path());
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].app_name, "NewTrackedApp");
    }
}
