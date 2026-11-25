use crate::discovery::appx::remove_appx_package;
use crate::models::{
    CleanupOptions, CoreError, CoreResult, InstalledApp, LeftoverItem, StartupItem,
};
use crate::safety::{create_system_restore_point, restore_registry_backup};
use crate::startup::{delete_startup_item, set_startup_enabled};
use crate::uninstaller::{execute_cleanup, run_native_uninstaller};
use crate::unlocker::{find_locking_processes, terminate_processes};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "payload", rename_all = "camelCase")]
pub enum WorkerRequest {
    Auth {
        token: String,
    },
    Ping,
    Shutdown,
    UninstallApp {
        app: Box<InstalledApp>,
        quiet: bool,
    },
    CleanLeftovers {
        app_name: String,
        items: Vec<LeftoverItem>,
        options: CleanupOptions,
        backup_dir: String,
    },
    CheckLocks {
        paths: Vec<String>,
    },
    TerminateProcesses {
        pids: Vec<u32>,
    },
    SetStartupEnabled {
        item: StartupItem,
        enabled: bool,
    },
    DeleteStartupItem {
        item: StartupItem,
    },
    CreateRestorePoint {
        description: String,
    },
    RemoveAppxPackage {
        package_full_name: String,
    },
    RestoreRegistryBackup {
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", content = "data", rename_all = "camelCase")]
pub enum WorkerResponse {
    Ok(serde_json::Value),
    Err(String),
}

const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

pub fn write_frame<W: Write>(writer: &mut W, payload: &[u8]) -> CoreResult<()> {
    let len = payload.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

pub fn read_frame<R: Read>(reader: &mut R) -> CoreResult<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(CoreError::Ipc(format!("IPC frame exceeds limit: {}", len)));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

/// Dispatch a privileged WorkerRequest locally (used both in-process when already elevated,
/// and inside the `--elevated-worker` helper process).
pub fn dispatch_worker_request(req: WorkerRequest) -> WorkerResponse {
    let result: CoreResult<serde_json::Value> = (|| match req {
        WorkerRequest::Auth { .. } | WorkerRequest::Ping => Ok(serde_json::json!({ "pong": true })),
        WorkerRequest::Shutdown => Ok(serde_json::json!({ "bye": true })),
        WorkerRequest::UninstallApp { app, quiet } => {
            let res = run_native_uninstaller(&app, quiet)?;
            Ok(serde_json::to_value(res)?)
        }
        WorkerRequest::CleanLeftovers {
            app_name,
            items,
            options,
            backup_dir,
        } => {
            let report = execute_cleanup(&app_name, &items, &options, Path::new(&backup_dir))?;
            Ok(serde_json::to_value(report)?)
        }
        WorkerRequest::CheckLocks { paths } => {
            let pbufs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
            let locks = find_locking_processes(&pbufs)?;
            Ok(serde_json::to_value(locks)?)
        }
        WorkerRequest::TerminateProcesses { pids } => {
            let outcomes = terminate_processes(&pids)?;
            Ok(serde_json::to_value(outcomes)?)
        }
        WorkerRequest::SetStartupEnabled { item, enabled } => {
            set_startup_enabled(&item, enabled)?;
            Ok(serde_json::json!({ "updated": true }))
        }
        WorkerRequest::DeleteStartupItem { item } => {
            delete_startup_item(&item)?;
            Ok(serde_json::json!({ "deleted": true }))
        }
        WorkerRequest::CreateRestorePoint { description } => {
            let ok = create_system_restore_point(&description)?;
            Ok(serde_json::json!({ "created": ok }))
        }
        WorkerRequest::RemoveAppxPackage { package_full_name } => {
            remove_appx_package(&package_full_name)?;
            Ok(serde_json::json!({ "removed": true }))
        }
        WorkerRequest::RestoreRegistryBackup { path } => {
            restore_registry_backup(Path::new(&path))?;
            Ok(serde_json::json!({ "restored": true }))
        }
    })();

    match result {
        Ok(val) => WorkerResponse::Ok(val),
        Err(err) => WorkerResponse::Err(err.to_string()),
    }
}

/// Run the standalone elevated worker loop: connects to the parent UI's Named Pipe,
/// authenticates with the session token, watches parent PID so it never becomes orphaned,
/// and serves privileged requests until Shutdown or disconnect.
#[cfg(windows)]
pub fn run_elevated_worker(pipe_name: &str, token: &str, parent_pid: u32) -> CoreResult<()> {
    use std::fs::OpenOptions;
    use std::time::Duration;

    // Spawn parent watchdog thread so this elevated process exits immediately if the UI terminates.
    if parent_pid > 4 {
        std::thread::spawn(move || {
            watch_parent_exit(parent_pid);
            std::process::exit(0);
        });
    }

    // Retry briefly while parent server transitions into listening state.
    let mut pipe = None;
    for _ in 0..40 {
        match OpenOptions::new().read(true).write(true).open(pipe_name) {
            Ok(f) => {
                pipe = Some(f);
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }

    let mut pipe = pipe.ok_or_else(|| {
        CoreError::Ipc(format!("Failed to connect to named pipe {}", pipe_name))
    })?;

    // Send initial authentication token so parent confirms identity.
    let auth_bytes = serde_json::to_vec(&WorkerRequest::Auth {
        token: token.to_string(),
    })?;
    write_frame(&mut pipe, &auth_bytes)?;

    // Wait for auth ack.
    let ack_raw = read_frame(&mut pipe)?;
    let ack: WorkerResponse = serde_json::from_slice(&ack_raw)?;
    if matches!(ack, WorkerResponse::Err(_)) {
        return Err(CoreError::Ipc("Worker authentication rejected".to_string()));
    }

    // Main RPC loop.
    while let Ok(req_bytes) = read_frame(&mut pipe) {
        let req: WorkerRequest = match serde_json::from_slice(&req_bytes) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = serde_json::to_vec(&WorkerResponse::Err(e.to_string()))?;
                let _ = write_frame(&mut pipe, &err_resp);
                continue;
            }
        };

        let is_shutdown = matches!(req, WorkerRequest::Shutdown);
        let resp = dispatch_worker_request(req);
        let resp_bytes = serde_json::to_vec(&resp)?;
        if write_frame(&mut pipe, &resp_bytes).is_err() {
            break;
        }
        if is_shutdown {
            break;
        }
    }

    Ok(())
}

#[cfg(windows)]
fn watch_parent_exit(parent_pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent_pid) {
            let _ = WaitForSingleObject(handle, INFINITE);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
pub fn run_elevated_worker(_pipe_name: &str, _token: &str, _parent_pid: u32) -> CoreResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_ipc_framing_roundtrip() {
        let req = WorkerRequest::Ping;
        let bytes = serde_json::to_vec(&req).unwrap();
        let mut buf = Vec::new();
        write_frame(&mut buf, &bytes).unwrap();

        let mut cursor = Cursor::new(buf);
        let decoded = read_frame(&mut cursor).unwrap();
        assert_eq!(decoded, bytes);

        let resp = dispatch_worker_request(WorkerRequest::Ping);
        assert!(matches!(resp, WorkerResponse::Ok(_)));
    }
}
