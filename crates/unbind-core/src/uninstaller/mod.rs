pub mod leftover;

use crate::discovery::appx::remove_appx_package;
use crate::models::{
    AppKind, CleanupItemResult, CleanupOptions, CleanupReport, CoreError, CoreResult,
    InstalledApp, LeftoverItem, LeftoverKind, UninstallExecutionResult,
};
use crate::safety::{
    create_system_restore_point, delete_fs_item, delete_registry_item, export_registry_backup,
};
use crate::unlocker::{find_locking_processes, terminate_processes};
use std::path::{Path, PathBuf};

/// Run an application's native uninstaller (Win32, MSI, or UWP/MSIX) and wait for the entire
/// uninstaller process tree (using a Windows Job Object) to finish.
pub fn run_native_uninstaller(
    app: &InstalledApp,
    quiet: bool,
) -> CoreResult<UninstallExecutionResult> {
    if app.kind == AppKind::StoreApp {
        if app.package_full_name.is_empty() {
            return Err(CoreError::InvalidInput(
                "Missing Store package_full_name".to_string(),
            ));
        }
        remove_appx_package(&app.package_full_name)?;
        return Ok(UninstallExecutionResult {
            launched: true,
            exit_code: Some(0),
            timed_out: false,
            message: format!("Removed Store package {}", app.name),
        });
    }

    let raw_cmd = if quiet && !app.quiet_uninstall_string.trim().is_empty() {
        app.quiet_uninstall_string.trim().to_string()
    } else if !app.uninstall_string.trim().is_empty() {
        app.uninstall_string.trim().to_string()
    } else if !app.product_code.is_empty() {
        if quiet {
            format!("msiexec.exe /X{} /qn /norestart", app.product_code)
        } else {
            format!("msiexec.exe /X{}", app.product_code)
        }
    } else {
        return Err(CoreError::InvalidInput(format!(
            "No uninstall command found for '{}'. Use Force Removal instead.",
            app.name
        )));
    };

    // Normalize MSI `/I{GUID}` uninstall strings to `/X{GUID}` so it directly uninstalls rather than repair.
    let cmd = normalize_msi_uninstall_command(&raw_cmd, quiet);
    run_command_with_job_tree_wait(&cmd, 600)
}

fn normalize_msi_uninstall_command(cmd: &str, quiet: bool) -> String {
    let lower = cmd.to_lowercase();
    if lower.contains("msiexec") {
        if let Some(guid) = crate::discovery::win32::extract_guid(cmd) {
            if quiet {
                return format!("msiexec.exe /X{} /qn /norestart", guid);
            }
            if lower.contains("/i") && !lower.contains("/x") {
                return format!("msiexec.exe /X{}", guid);
            }
        }
    }
    cmd.to_string()
}

/// Launch a command line via Win32 `CreateProcessW`, attach it to an anonymous Job Object so that
/// spawned child uninstallers (e.g. Inno Setup / NSIS temp helpers) are tracked as a group,
/// and wait until all processes in the job have terminated.
#[cfg(windows)]
pub fn run_command_with_job_tree_wait(
    command_line: &str,
    timeout_secs: u64,
) -> CoreResult<UninstallExecutionResult> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::time::{Duration, Instant};
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW,
    };
    use windows::Win32::System::Threading::{
        CreateProcessW, GetExitCodeProcess, ResumeThread, WaitForSingleObject,
        CREATE_SUSPENDED, PROCESS_INFORMATION, STARTUPINFOW,
    };

    let expanded = crate::discovery::win32::expand_env_vars(command_line);
    let mut cmd_wide: Vec<u16> = std::ffi::OsStr::new(&expanded)
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe {
        let si = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();

        // Create suspended so we can assign to Job Object before any child spawns.
        let created = CreateProcessW(
            PCWSTR::null(),
            PWSTR(cmd_wide.as_mut_ptr()),
            None,
            None,
            false,
            CREATE_SUSPENDED,
            None,
            PCWSTR::null(),
            &si,
            &mut pi,
        );

        if let Err(err) = created {
            let win_err = GetLastError().0;
            // ERROR_ELEVATION_REQUIRED = 740
            if win_err == 740 || err.code().0 as u32 == 0x800702E4 {
                return Err(CoreError::ElevationRequired(
                    "Uninstaller requires Administrator privileges".to_string(),
                ));
            }
            return Err(CoreError::WinApi(format!(
                "Failed to launch uninstaller '{}': {}",
                expanded, err
            )));
        }

        // Attach to anonymous Job Object (best effort; if already in non-nestable job, still wait on root process).
        let job: Option<HANDLE> = CreateJobObjectW(None, PCWSTR::null()).ok();
        let job_attached = if let Some(j) = job {
            AssignProcessToJobObject(j, pi.hProcess).is_ok()
        } else {
            false
        };

        let _ = ResumeThread(pi.hThread);
        let _ = CloseHandle(pi.hThread);

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut timed_out = false;

        // Wait for root uninstaller process to exit.
        loop {
            let rem = deadline.saturating_duration_since(Instant::now());
            if rem.is_zero() {
                timed_out = true;
                break;
            }
            let slice_ms = rem.as_millis().min(500) as u32;
            if WaitForSingleObject(pi.hProcess, slice_ms) == WAIT_OBJECT_0 {
                break;
            }
        }

        let mut exit_code_raw: u32 = 0;
        let exit_code = if !timed_out && GetExitCodeProcess(pi.hProcess, &mut exit_code_raw).is_ok()
        {
            Some(exit_code_raw as i32)
        } else {
            None
        };
        let _ = CloseHandle(pi.hProcess);

        // If job tracking succeeded, wait briefly for detached child uninstallers (Inno/NSIS %TEMP% copies) to finish.
        if job_attached && !timed_out {
            if let Some(j) = job {
                // Brief grace period to let child spawn register.
                std::thread::sleep(Duration::from_millis(350));
                loop {
                    if Instant::now() >= deadline {
                        timed_out = true;
                        break;
                    }
                    let active_count = query_job_process_count(j);
                    if active_count == 0 {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }

        if let Some(j) = job {
            let _ = CloseHandle(j);
        }

        Ok(UninstallExecutionResult {
            launched: true,
            exit_code,
            timed_out,
            message: if timed_out {
                "Uninstaller timed out while still running".to_string()
            } else {
                format!("Uninstaller exited with code {:?}", exit_code.unwrap_or(0))
            },
        })
    }
}

#[cfg(windows)]
unsafe fn query_job_process_count(job: windows::Win32::Foundation::HANDLE) -> u32 {
    use windows::Win32::System::JobObjects::{
        JobObjectBasicProcessIdList, QueryInformationJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
    };
    let mut buf = [0u8; 4096];
    let mut ret_len = 0u32;
    let res = QueryInformationJobObject(
        job,
        JobObjectBasicProcessIdList,
        buf.as_mut_ptr() as *mut _,
        buf.len() as u32,
        Some(&mut ret_len),
    );
    if res.is_ok() {
        let list = &*(buf.as_ptr() as *const JOBOBJECT_BASIC_PROCESS_ID_LIST);
        list.NumberOfProcessIdsInList
    } else {
        0
    }
}

#[cfg(not(windows))]
pub fn run_command_with_job_tree_wait(
    _command_line: &str,
    _timeout_secs: u64,
) -> CoreResult<UninstallExecutionResult> {
    Ok(UninstallExecutionResult {
        launched: true,
        exit_code: Some(0),
        timed_out: false,
        message: "Mock exited".to_string(),
    })
}

/// Safely execute cleanup of user-selected leftover items with optional `.reg` backup,
/// System Restore Point, Restart Manager unlocking, Recycle Bin deletion, and reboot fallback.
pub fn execute_cleanup(
    app_name: &str,
    items: &[LeftoverItem],
    options: &CleanupOptions,
    backup_dir: &Path,
) -> CoreResult<CleanupReport> {
    let mut restore_point_created = false;
    if options.create_restore_point {
        let desc = format!("Unbind - Before cleaning {}", app_name);
        if let Ok(created) = create_system_restore_point(&desc) {
            restore_point_created = created;
        }
    }

    let reg_backup_path = if options.backup_registry {
        export_registry_backup(items, app_name, backup_dir)?
            .map(|p| p.to_string_lossy().to_string())
    } else {
        None
    };

    // Optionally unlock files before deleting.
    if options.terminate_locking_processes {
        let fs_paths: Vec<PathBuf> = items
            .iter()
            .filter(|i| {
                matches!(
                    i.kind,
                    LeftoverKind::Directory | LeftoverKind::File | LeftoverKind::Shortcut
                )
            })
            .map(|i| PathBuf::from(&i.path))
            .collect();
        if let Ok(locks) = find_locking_processes(&fs_paths) {
            let pids: Vec<u32> = locks.into_iter().map(|l| l.pid).collect();
            if !pids.is_empty() {
                let _ = terminate_processes(&pids);
            }
        }
    }

    // Delete services & scheduled tasks first so running background services release file locks
    // before directory removal, then delete deeper paths before parent directories.
    let mut ordered: Vec<&LeftoverItem> = items.iter().collect();
    ordered.sort_by_key(|a| {
        let phase = match a.kind {
            LeftoverKind::Service => 0u8,
            LeftoverKind::ScheduledTask => 1u8,
            _ => 2u8,
        };
        (phase, std::cmp::Reverse(a.path.len()))
    });

    let mut succeeded = 0usize;
    let mut failed = 0usize;
    let mut scheduled_for_reboot = 0usize;
    let mut freed_bytes = 0u64;
    let mut details = Vec::with_capacity(ordered.len());

    for item in ordered {
        let outcome = match item.kind {
            LeftoverKind::Service => delete_windows_service(&item.path).map(|()| false),
            LeftoverKind::RegistryKey => delete_registry_item(&item.path, None).map(|()| false),
            LeftoverKind::RegistryValue => {
                delete_registry_item(&item.path, item.value_name.as_deref()).map(|()| false)
            }
            LeftoverKind::Directory | LeftoverKind::File | LeftoverKind::Shortcut => {
                delete_fs_item(
                    Path::new(&item.path),
                    options.use_recycle_bin,
                    options.schedule_reboot_on_lock,
                )
            }
            LeftoverKind::ScheduledTask => delete_scheduled_task(&item.path).map(|()| false),
        };

        match outcome {
            Ok(is_reboot) => {
                succeeded += 1;
                if is_reboot {
                    scheduled_for_reboot += 1;
                } else {
                    freed_bytes = freed_bytes.saturating_add(item.size_bytes);
                }
                details.push(CleanupItemResult {
                    item_id: item.id.clone(),
                    path: item.path.clone(),
                    success: true,
                    scheduled_for_reboot: is_reboot,
                    error: None,
                });
            }
            Err(err) => {
                failed += 1;
                details.push(CleanupItemResult {
                    item_id: item.id.clone(),
                    path: item.path.clone(),
                    success: false,
                    scheduled_for_reboot: false,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    Ok(CleanupReport {
        succeeded,
        failed,
        scheduled_for_reboot,
        freed_bytes,
        reg_backup_path,
        restore_point_created,
        details,
    })
}

fn delete_scheduled_task(task_name: &str) -> CoreResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let out = std::process::Command::new("schtasks.exe")
        .args(["/Delete", "/TN", task_name, "/F"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.to_lowercase().contains("access is denied") {
            Err(CoreError::ElevationRequired(format!(
                "Access denied deleting scheduled task {}",
                task_name
            )))
        } else {
            Err(CoreError::WinApi(format!(
                "Failed to delete scheduled task {}: {}",
                task_name,
                stderr.trim()
            )))
        }
    }
}

/// Extract service short name from `HKLM\SYSTEM\CurrentControlSet\Services\<Name>` or bare `<Name>`.
fn extract_service_name(service_ref: &str) -> Option<&str> {
    let trimmed = service_ref.trim().trim_end_matches('\\');
    if trimmed.is_empty() {
        return None;
    }
    let name = trimmed.rsplit('\\').next().unwrap_or(trimmed).trim();
    if name.is_empty() || name.eq_ignore_ascii_case("services") {
        None
    } else {
        Some(name)
    }
}

/// Stop and unregister a Windows Service cleanly through Service Control Manager (SCM).
/// Falls back to registry cleanup only if SCM reports the service record no longer exists.
#[cfg(windows)]
fn delete_windows_service(service_ref: &str) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::time::{Duration, Instant};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        GetLastError, ERROR_ACCESS_DENIED, ERROR_SERVICE_DOES_NOT_EXIST,
    };
    use windows::Win32::System::Services::{
        CloseServiceHandle, ControlService, DeleteService, OpenSCManagerW, OpenServiceW,
        QueryServiceStatus, SC_MANAGER_ALL_ACCESS, SERVICE_CONTROL_STOP, SERVICE_QUERY_STATUS,
        SERVICE_STATUS, SERVICE_STOP, SERVICE_STOPPED,
    };

    const SERVICE_DELETE_RIGHT: u32 = 0x0001_0000; // Win32 DELETE

    let svc_name = extract_service_name(service_ref)
        .ok_or_else(|| CoreError::InvalidInput(format!("Invalid service target: {}", service_ref)))?;

    let wide: Vec<u16> = std::ffi::OsStr::new(svc_name)
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe {
        let scm = match OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_ALL_ACCESS) {
            Ok(h) => h,
            Err(e) => {
                if GetLastError() == ERROR_ACCESS_DENIED {
                    return Err(CoreError::ElevationRequired(format!(
                        "Administrator privileges required to manage Windows Service '{}'",
                        svc_name
                    )));
                }
                return Err(CoreError::WinApi(format!(
                    "OpenSCManagerW failed for '{}': {}",
                    svc_name, e
                )));
            }
        };

        let svc_res = OpenServiceW(
            scm,
            PCWSTR(wide.as_ptr()),
            SERVICE_STOP | SERVICE_QUERY_STATUS | SERVICE_DELETE_RIGHT,
        );

        let svc = match svc_res {
            Ok(h) => h,
            Err(e) => {
                let last = GetLastError();
                let _ = CloseServiceHandle(scm);
                if last == ERROR_ACCESS_DENIED {
                    return Err(CoreError::ElevationRequired(format!(
                        "Access denied opening service '{}'",
                        svc_name
                    )));
                }
                if last == ERROR_SERVICE_DOES_NOT_EXIST {
                    // Orphan registry key only: clean via guarded registry deletion if full path.
                    if service_ref.contains('\\') {
                        return delete_registry_item(service_ref, None);
                    }
                    return Ok(());
                }
                return Err(CoreError::WinApi(format!(
                    "OpenServiceW failed for '{}': {}",
                    svc_name, e
                )));
            }
        };

        let mut status = SERVICE_STATUS::default();
        if QueryServiceStatus(svc, &mut status).is_ok() && status.dwCurrentState != SERVICE_STOPPED
        {
            let _ = ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(200));
                if QueryServiceStatus(svc, &mut status).is_ok()
                    && status.dwCurrentState == SERVICE_STOPPED
                {
                    break;
                }
            }
        }

        let del_res = DeleteService(svc);
        let _ = CloseServiceHandle(svc);
        let _ = CloseServiceHandle(scm);

        del_res.map_err(|e| {
            CoreError::WinApi(format!("DeleteService failed for '{}': {}", svc_name, e))
        })?;
        Ok(())
    }
}

#[cfg(not(windows))]
fn delete_windows_service(_service_ref: &str) -> CoreResult<()> {
    Ok(())
}
