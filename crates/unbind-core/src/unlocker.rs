use crate::models::{CoreError, CoreResult, LockingProcessInfo};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Inspect which running Windows processes currently hold open file locks on any of the given
/// files or files inside the given directories, using the Windows Restart Manager API.
#[cfg(windows)]
pub fn find_locking_processes(paths: &[PathBuf]) -> CoreResult<Vec<LockingProcessInfo>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, CCH_RM_SESSION_KEY,
        RM_PROCESS_INFO,
    };

    // Collect concrete file paths (expand directories up to a safe cap).
    let mut files: Vec<PathBuf> = Vec::new();
    for p in paths {
        if p.is_file() {
            files.push(p.clone());
        } else if p.is_dir() {
            for entry in walkdir::WalkDir::new(p)
                .max_depth(6)
                .into_iter()
                .flatten()
                .take(400)
            {
                if entry.file_type().is_file() {
                    // Prioritize executables, DLLs, and open data files.
                    files.push(entry.into_path());
                    if files.len() >= 256 {
                        break;
                    }
                }
            }
        }
        if files.len() >= 256 {
            break;
        }
    }

    if files.is_empty() {
        return Ok(Vec::new());
    }

    let wide_storage: Vec<Vec<u16>> = files
        .iter()
        .map(|p| p.as_os_str().encode_wide().chain(Some(0)).collect())
        .collect();
    let ptrs: Vec<PCWSTR> = wide_storage
        .iter()
        .map(|w| PCWSTR(w.as_ptr()))
        .collect();

    unsafe {
        let mut session: u32 = 0;
        let mut session_key = [0u16; (CCH_RM_SESSION_KEY + 1) as usize];
        let start_err = RmStartSession(&mut session, 0, PWSTR(session_key.as_mut_ptr()));
        if start_err != ERROR_SUCCESS {
            return Err(CoreError::WinApi(format!(
                "RmStartSession failed with error {:?}",
                start_err
            )));
        }

        let reg_err = RmRegisterResources(session, Some(&ptrs), None, None);
        if reg_err != ERROR_SUCCESS {
            let _ = RmEndSession(session);
            return Err(CoreError::WinApi(format!(
                "RmRegisterResources failed with error {:?}",
                reg_err
            )));
        }

        let mut needed: u32 = 0;
        let mut count: u32 = 0;
        let mut reboot_reasons: u32 = 0;
        let first_err = RmGetList(
            session,
            &mut needed,
            &mut count,
            None,
            &mut reboot_reasons,
        );

        let mut dedup: HashMap<u32, LockingProcessInfo> = HashMap::new();
        if first_err == ERROR_MORE_DATA && needed > 0 {
            let mut infos = vec![RM_PROCESS_INFO::default(); needed as usize];
            count = needed;
            let second_err = RmGetList(
                session,
                &mut needed,
                &mut count,
                Some(infos.as_mut_ptr()),
                &mut reboot_reasons,
            );
            if second_err == ERROR_SUCCESS {
                let current_pid = std::process::id();
                for info in infos.into_iter().take(count as usize) {
                    let pid = info.Process.dwProcessId;
                    if pid == 0 || pid == current_pid {
                        continue;
                    }
                    let app_name = utf16_slice_to_string(&info.strAppName);
                    let service_name = utf16_slice_to_string(&info.strServiceShortName);
                    let app_type = format!("{:?}", info.ApplicationType);
                    dedup.entry(pid).or_insert(LockingProcessInfo {
                        pid,
                        name: if app_name.is_empty() {
                            format!("PID {}", pid)
                        } else {
                            app_name
                        },
                        service_short_name: service_name,
                        app_type,
                        restartable: info.bRestartable.as_bool(),
                    });
                }
            }
        }

        let _ = RmEndSession(session);
        let mut result: Vec<LockingProcessInfo> = dedup.into_values().collect();
        result.sort_by_key(|p| p.pid);
        Ok(result)
    }
}

#[cfg(not(windows))]
pub fn find_locking_processes(_paths: &[PathBuf]) -> CoreResult<Vec<LockingProcessInfo>> {
    Ok(Vec::new())
}

/// Terminate specified non-critical user processes by PID to release locked files.
#[cfg(windows)]
pub fn terminate_processes(pids: &[u32]) -> CoreResult<Vec<(u32, bool, Option<String>)>> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    let self_pid = std::process::id();
    let mut outcomes = Vec::with_capacity(pids.len());

    for &pid in pids {
        if pid <= 4 || pid == self_pid {
            outcomes.push((
                pid,
                false,
                Some("Refused to terminate protected/current process".to_string()),
            ));
            continue;
        }

        unsafe {
            match OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) {
                Ok(handle) => {
                    let term_res = TerminateProcess(handle, 1);
                    if term_res.is_ok() {
                        let _ = WaitForSingleObject(handle, 3000);
                        let _ = CloseHandle(handle);
                        outcomes.push((pid, true, None));
                    } else {
                        let _ = CloseHandle(handle);
                        outcomes.push((
                            pid,
                            false,
                            Some(format!("TerminateProcess failed for PID {}", pid)),
                        ));
                    }
                }
                Err(e) => {
                    outcomes.push((
                        pid,
                        false,
                        Some(format!("OpenProcess failed for PID {}: {}", pid, e)),
                    ));
                }
            }
        }
    }

    Ok(outcomes)
}

#[cfg(not(windows))]
pub fn terminate_processes(_pids: &[u32]) -> CoreResult<Vec<(u32, bool, Option<String>)>> {
    Ok(Vec::new())
}

fn utf16_slice_to_string(slice: &[u16]) -> String {
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..end]).trim().to_string()
}

/// Resolve a Windows `.lnk` shortcut file to its target executable/folder path via COM `IShellLinkW`.
#[cfg(windows)]
pub fn resolve_shortcut_target(lnk_path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED, STGM,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    if !lnk_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
    {
        return None;
    }

    unsafe {
        let init_hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let need_uninit = init_hr.is_ok();

        let result = (|| -> Option<PathBuf> {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let persist: IPersistFile = link.cast().ok()?;
            let wide: Vec<u16> = lnk_path.as_os_str().encode_wide().chain(Some(0)).collect();
            persist.Load(PCWSTR(wide.as_ptr()), STGM(0)).ok()?;

            let mut buf = [0u16; 1024];
            let mut find_data = WIN32_FIND_DATAW::default();
            link.GetPath(&mut buf, &mut find_data, 0).ok()?;
            let target = utf16_slice_to_string(&buf);
            if target.is_empty() {
                None
            } else {
                Some(PathBuf::from(target))
            }
        })();

        if need_uninit {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
pub fn resolve_shortcut_target(_lnk_path: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use tempfile::tempdir;

    #[test]
    fn test_find_locking_processes_on_temp_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("locked_test.dat");
        let _handle = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&file_path)
            .unwrap();

        // Current process holds the lock and is excluded from the returned list,
        // so Restart Manager call must succeed without error.
        let locks = find_locking_processes(&[file_path]).unwrap();
        assert!(locks.iter().all(|p| p.pid != std::process::id()));
    }
}
