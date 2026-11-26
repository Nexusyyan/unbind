use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use unbind_core::models::{
    PendingUpdateOutcome, StagedUpdateInfo, UpdateCheckResult, UpdateProgressEvent, UpdateStage,
    UpdateStateSnapshot,
};
use unbind_core::safety::is_process_elevated;
use unbind_core::updater::{
    apply_staged_binary_swap, compute_file_sha256, now_epoch_ms, update_outcome_path,
    updater_temp_root, verify_pe_executable, write_update_outcome,
};
use uuid::Uuid;

pub const UPDATE_ELEVATION_CANCELLED: &str = "UPDATE_ELEVATION_CANCELLED";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlan {
    pub parent_pid: u32,
    pub worker_pid: Option<u32>,
    pub target_exe: String,
    pub staged_exe: String,
    pub expected_sha256: String,
    pub from_version: String,
    pub to_version: String,
    pub staging_dir: String,
    pub outcome_path: String,
    pub relaunch_unelevated: bool,
}

pub struct UpdateRuntimeState {
    latest_check: Mutex<Option<UpdateCheckResult>>,
    progress: Mutex<UpdateProgressEvent>,
    cancel_flag: Arc<AtomicBool>,
    downloading: AtomicBool,
}

impl Default for UpdateRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateRuntimeState {
    pub fn new() -> Self {
        Self {
            latest_check: Mutex::new(None),
            progress: Mutex::new(UpdateProgressEvent::default()),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            downloading: AtomicBool::new(false),
        }
    }

    pub fn snapshot(&self) -> UpdateStateSnapshot {
        let latest_check = self.latest_check.lock().ok().and_then(|g| g.clone());
        let progress = self
            .progress
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        UpdateStateSnapshot {
            latest_check,
            progress,
        }
    }

    pub fn set_latest_check(&self, result: UpdateCheckResult) {
        if let Ok(mut guard) = self.latest_check.lock() {
            *guard = Some(result);
        }
    }

    pub fn latest_check(&self) -> Option<UpdateCheckResult> {
        self.latest_check.lock().ok().and_then(|g| g.clone())
    }

    pub fn progress(&self) -> UpdateProgressEvent {
        self.progress
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn set_progress(&self, next: UpdateProgressEvent) {
        if let Ok(mut guard) = self.progress.lock() {
            *guard = next;
        }
    }

    pub fn try_begin_download(&self) -> bool {
        if self
            .downloading
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.cancel_flag.store(false, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn finish_download(&self) {
        self.downloading.store(false, Ordering::SeqCst);
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    pub fn request_cancel(&self) -> bool {
        if self.downloading.load(Ordering::SeqCst) {
            self.cancel_flag.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn valid_staged_update(&self) -> Option<StagedUpdateInfo> {
        let staged = self
            .progress
            .lock()
            .ok()
            .and_then(|g| {
                if g.stage == UpdateStage::Ready {
                    g.staged.clone()
                } else {
                    None
                }
            })?;
        let p = Path::new(&staged.staged_exe_path);
        if p.exists() {
            Some(staged)
        } else {
            None
        }
    }
}

/// Probe whether `dir` is writable by the current user token without relying on brittle ACL heuristics.
pub fn is_dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(
        ".unbind-write-probe-{}-{}.tmp",
        std::process::id(),
        Uuid::new_v4().simple()
    ));
    match fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
    {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Prepare and launch a detached `%TEMP%` copy of the current executable in `--apply-update` mode.
/// Elevates via UAC only if the target executable's directory is not writable by the current user.
pub fn launch_self_update_helper(
    staged: &StagedUpdateInfo,
    from_version: &str,
    worker_pid: Option<u32>,
) -> Result<(), String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed resolving current executable path: {}", e))?;
    let target_dir = current_exe
        .parent()
        .ok_or_else(|| "Current executable has no parent directory".to_string())?;
    let staged_path = Path::new(&staged.staged_exe_path);
    if !staged_path.exists() {
        return Err("Staged update file no longer exists; please download again".to_string());
    }

    // Re-verify integrity before spawning helper or exiting the main app.
    verify_pe_executable(staged_path).map_err(|e| e.to_string())?;
    let actual_sha = compute_file_sha256(staged_path).map_err(|e| e.to_string())?;
    if !actual_sha.eq_ignore_ascii_case(&staged.sha256) {
        return Err("Staged update checksum mismatch; please download again".to_string());
    }

    let temp_root = updater_temp_root();
    fs::create_dir_all(&temp_root)
        .map_err(|e| format!("Failed creating updater temp directory: {}", e))?;

    let run_id = Uuid::new_v4().simple().to_string();
    let helper_exe = temp_root.join(format!("helper-{}.exe", run_id));
    let plan_path = temp_root.join(format!("plan-{}.json", run_id));

    fs::copy(&current_exe, &helper_exe)
        .map_err(|e| format!("Failed preparing update helper executable: {}", e))?;

    let already_elevated = is_process_elevated();
    let needs_elevation = !already_elevated && !is_dir_writable(target_dir);

    let plan = UpdatePlan {
        parent_pid: std::process::id(),
        worker_pid,
        target_exe: current_exe.to_string_lossy().to_string(),
        staged_exe: staged.staged_exe_path.clone(),
        expected_sha256: staged.sha256.clone(),
        from_version: from_version.to_string(),
        to_version: staged.version.clone(),
        staging_dir: staged.staging_dir.clone(),
        outcome_path: update_outcome_path().to_string_lossy().to_string(),
        // Keep the relaunched desktop UI standard-user whenever it was standard-user before.
        relaunch_unelevated: needs_elevation && !already_elevated,
    };

    let plan_json = serde_json::to_string_pretty(&plan)
        .map_err(|e| format!("Failed serializing update plan: {}", e))?;
    if let Err(e) = fs::write(&plan_path, plan_json) {
        let _ = fs::remove_file(&helper_exe);
        return Err(format!("Failed writing update plan: {}", e));
    }

    let spawn_res = if needs_elevation {
        spawn_elevated_helper(&helper_exe, &plan_path)
    } else {
        spawn_standard_helper(&helper_exe, &plan_path)
    };

    if let Err(e) = spawn_res {
        let _ = fs::remove_file(&helper_exe);
        let _ = fs::remove_file(&plan_path);
        return Err(e);
    }

    Ok(())
}

/// Entrypoint invoked from `main.rs` when launched with `--apply-update --plan <path>`.
/// Never initializes Tauri, WebView2, or the GUI single-instance lock.
pub fn run_apply_update_from_args(args: &[String]) -> i32 {
    let mut plan_path: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--plan" && i + 1 < args.len() {
            plan_path = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else {
            i += 1;
        }
    }

    let Some(plan_file) = plan_path else {
        return 2;
    };
    let Ok(raw) = fs::read_to_string(&plan_file) else {
        return 3;
    };
    let Ok(plan) = serde_json::from_str::<UpdatePlan>(&raw) else {
        let _ = fs::remove_file(&plan_file);
        return 4;
    };

    let code = execute_update_plan(&plan);
    let _ = fs::remove_file(&plan_file);
    code
}

fn execute_update_plan(plan: &UpdatePlan) -> i32 {
    // 1. Wait for both parent GUI process and any elevated worker sharing the executable to exit.
    wait_for_pid_exit(plan.parent_pid, 20_000);
    if let Some(wpid) = plan.worker_pid {
        wait_for_pid_exit(wpid, 10_000);
    }
    // Give Windows kernel & single-instance mutex a brief settle window after process exit.
    std::thread::sleep(Duration::from_millis(320));

    let target_exe = Path::new(&plan.target_exe);
    let staged_exe = Path::new(&plan.staged_exe);

    // 2. Perform verified atomic binary swap with automatic rollback.
    let swap_result = apply_staged_binary_swap(staged_exe, target_exe, &plan.expected_sha256);

    let outcome = match &swap_result {
        Ok(()) => PendingUpdateOutcome {
            success: true,
            from_version: plan.from_version.clone(),
            to_version: plan.to_version.clone(),
            error: None,
            timestamp_ms: now_epoch_ms(),
        },
        Err(err) => PendingUpdateOutcome {
            success: false,
            from_version: plan.from_version.clone(),
            to_version: plan.to_version.clone(),
            error: Some(err.to_string()),
            timestamp_ms: now_epoch_ms(),
        },
    };
    let _ = write_update_outcome(Path::new(&plan.outcome_path), &outcome);

    // 3. Clean up the staging folder immediately (helper itself is cleaned on next launch).
    let staging_dir = Path::new(&plan.staging_dir);
    if staging_dir.exists() {
        let _ = fs::remove_dir_all(staging_dir);
    }

    // 4. Relaunch Unbind (new version on success, or rolled-back version on failure so user gets feedback).
    relaunch_app(target_exe, plan.relaunch_unelevated);

    if swap_result.is_ok() {
        0
    } else {
        1
    }
}

#[cfg(windows)]
fn spawn_standard_helper(helper_exe: &Path, plan_path: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    std::process::Command::new(helper_exe)
        .arg("--apply-update")
        .arg("--plan")
        .arg(plan_path)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed launching update helper: {}", e))
}

#[cfg(not(windows))]
fn spawn_standard_helper(_helper_exe: &Path, _plan_path: &Path) -> Result<(), String> {
    Err("Self-update is only supported on Windows".to_string())
}

#[cfg(windows)]
fn spawn_elevated_helper(helper_exe: &Path, plan_path: &Path) -> Result<(), String> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let exe_wide: Vec<u16> = helper_exe
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let params = format!("--apply-update --plan \"{}\"", plan_path.display());
    let params_wide: Vec<u16> = std::ffi::OsStr::new(&params)
        .encode_wide()
        .chain(Some(0))
        .collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe_wide.as_ptr()),
        lpParameters: PCWSTR(params_wide.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    unsafe {
        ShellExecuteExW(&mut sei).map_err(|_| UPDATE_ELEVATION_CANCELLED.to_string())?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn spawn_elevated_helper(_helper_exe: &Path, _plan_path: &Path) -> Result<(), String> {
    Err("Self-update is only supported on Windows".to_string())
}

#[cfg(windows)]
fn wait_for_pid_exit(pid: u32, timeout_ms: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    if pid <= 4 {
        return;
    }
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            let _ = WaitForSingleObject(handle, timeout_ms);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn wait_for_pid_exit(_pid: u32, _timeout_ms: u32) {}

fn relaunch_app(target_exe: &Path, prefer_unelevated: bool) {
    #[cfg(windows)]
    {
        if prefer_unelevated && is_process_elevated() && spawn_unelevated_via_shell(target_exe) {
            return;
        }
    }
    let _ = prefer_unelevated;
    let mut cmd = std::process::Command::new(target_exe);
    if let Some(dir) = target_exe.parent() {
        cmd.current_dir(dir);
    }
    let _ = cmd.spawn();
}

/// Drop privileges back to the interactive Explorer user token so the relaunched GUI
/// keeps standard-user Explorer drag-and-drop and WebView2 profile compatibility.
#[cfg(windows)]
fn spawn_unelevated_via_shell(target_exe: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        DuplicateTokenEx, SecurityImpersonation, TokenPrimary, TOKEN_ADJUST_DEFAULT,
        TOKEN_ADJUST_SESSIONID, TOKEN_ASSIGN_PRIMARY, TOKEN_DUPLICATE, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{
        CreateProcessWithTokenW, OpenProcess, OpenProcessToken, LOGON_WITH_PROFILE,
        PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        STARTUPINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId};

    unsafe {
        let shell_hwnd = GetShellWindow();
        if shell_hwnd.0.is_null() {
            return false;
        }
        let mut shell_pid: u32 = 0;
        let _ = GetWindowThreadProcessId(shell_hwnd, Some(&mut shell_pid));
        if shell_pid <= 4 {
            return false;
        }

        let Ok(shell_proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, shell_pid)
        else {
            return false;
        };

        let mut shell_token = HANDLE::default();
        let opened = OpenProcessToken(shell_proc, TOKEN_DUPLICATE, &mut shell_token).is_ok();
        let _ = CloseHandle(shell_proc);
        if !opened {
            return false;
        }

        let mut primary_token = HANDLE::default();
        let access = TOKEN_ASSIGN_PRIMARY
            | TOKEN_DUPLICATE
            | TOKEN_QUERY
            | TOKEN_ADJUST_DEFAULT
            | TOKEN_ADJUST_SESSIONID;
        let duplicated = DuplicateTokenEx(
            shell_token,
            access,
            None,
            SecurityImpersonation,
            TokenPrimary,
            &mut primary_token,
        )
        .is_ok();
        let _ = CloseHandle(shell_token);
        if !duplicated {
            return false;
        }

        let app_w: Vec<u16> = target_exe
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut cmdline_w: Vec<u16> = std::ffi::OsStr::new(&format!("\"{}\"", target_exe.display()))
            .encode_wide()
            .chain(Some(0))
            .collect();
        let cwd_w: Option<Vec<u16>> = target_exe
            .parent()
            .map(|p| p.as_os_str().encode_wide().chain(Some(0)).collect());

        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        let created = CreateProcessWithTokenW(
            primary_token,
            LOGON_WITH_PROFILE,
            PCWSTR(app_w.as_ptr()),
            PWSTR(cmdline_w.as_mut_ptr()),
            PROCESS_CREATION_FLAGS(0),
            None,
            cwd_w
                .as_ref()
                .map(|v| PCWSTR(v.as_ptr()))
                .unwrap_or(PCWSTR::null()),
            &si,
            &mut pi,
        )
        .is_ok();

        let _ = CloseHandle(primary_token);
        if created {
            if !pi.hProcess.is_invalid() {
                let _ = CloseHandle(pi.hProcess);
            }
            if !pi.hThread.is_invalid() {
                let _ = CloseHandle(pi.hThread);
            }
        }
        created
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dir_writable_and_plan_roundtrip() {
        let dir = std::env::temp_dir();
        assert!(is_dir_writable(&dir));

        let plan = UpdatePlan {
            parent_pid: 1234,
            worker_pid: Some(5678),
            target_exe: "C:\\Tools\\Unbind\\unbind.exe".to_string(),
            staged_exe: "C:\\Temp\\unbind-updater\\stage\\unbind.exe".to_string(),
            expected_sha256: "a".repeat(64),
            from_version: "0.5.0".to_string(),
            to_version: "0.6.0".to_string(),
            staging_dir: "C:\\Temp\\unbind-updater\\stage".to_string(),
            outcome_path: "C:\\Temp\\unbind-updater\\last-outcome.json".to_string(),
            relaunch_unelevated: true,
        };
        let raw = serde_json::to_string(&plan).unwrap();
        let decoded: UpdatePlan = serde_json::from_str(&raw).unwrap();
        assert_eq!(decoded.parent_pid, 1234);
        assert_eq!(decoded.worker_pid, Some(5678));
        assert!(decoded.relaunch_unelevated);
        assert_eq!(run_apply_update_from_args(&["unbind.exe".into(), "--apply-update".into()]), 2);
    }
}
