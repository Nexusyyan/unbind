use std::fs::File;
use std::sync::Mutex;
use unbind_core::ipc::{dispatch_worker_request, read_frame, write_frame, WorkerRequest, WorkerResponse};
use unbind_core::models::{CoreError, CoreResult};
use unbind_core::safety::is_process_elevated;

struct WorkerSession {
    pipe: File,
    pid: u32,
}

/// Manages an on-demand, reusable elevated worker session over a PID-verified Windows Named Pipe.
/// The UI remains a standard-user process (preserving Explorer drag-and-drop and avoiding
/// WebView2 elevated profile issues), while privileged operations run in an elevated twin instance.
#[derive(Default)]
pub struct ElevationBridge {
    session: Mutex<Option<WorkerSession>>,
}

impl ElevationBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_worker_connected(&self) -> bool {
        self.session
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Execute a privileged request:
    /// - If this process is already Administrator, execute in-process immediately.
    /// - Otherwise ensure the elevated worker is running (prompting UAC once per session)
    ///   and send the request over the authenticated Named Pipe.
    pub fn execute(&self, req: WorkerRequest) -> CoreResult<serde_json::Value> {
        if is_process_elevated() {
            return match dispatch_worker_request(req) {
                WorkerResponse::Ok(v) => Ok(v),
                WorkerResponse::Err(e) => Err(CoreError::WinApi(e)),
            };
        }

        self.ensure_connected()?;

        let mut guard = self
            .session
            .lock()
            .map_err(|_| CoreError::Ipc("Elevation lock poisoned".to_string()))?;

        let session = guard
            .as_mut()
            .ok_or_else(|| CoreError::Ipc("Elevated worker is not connected".to_string()))?;

        let payload = serde_json::to_vec(&req)?;
        if let Err(e) = write_frame(&mut session.pipe, &payload) {
            *guard = None;
            return Err(CoreError::Ipc(format!("Failed sending IPC request: {}", e)));
        }

        let resp_raw = match read_frame(&mut session.pipe) {
            Ok(b) => b,
            Err(e) => {
                *guard = None;
                return Err(CoreError::Ipc(format!("Failed reading IPC response: {}", e)));
            }
        };

        let resp: WorkerResponse = serde_json::from_slice(&resp_raw)?;
        match resp {
            WorkerResponse::Ok(val) => Ok(val),
            WorkerResponse::Err(msg) => Err(CoreError::WinApi(msg)),
        }
    }

    /// Prompt UAC (once) to spawn and connect the elevated worker session.
    pub fn ensure_connected(&self) -> CoreResult<()> {
        if is_process_elevated() {
            return Ok(());
        }

        {
            let mut guard = self
                .session
                .lock()
                .map_err(|_| CoreError::Ipc("Elevation lock poisoned".to_string()))?;
            if let Some(sess) = guard.as_mut() {
                // Verify pipe is still healthy via Ping.
                let ping = serde_json::to_vec(&WorkerRequest::Ping)?;
                if write_frame(&mut sess.pipe, &ping).is_ok() && read_frame(&mut sess.pipe).is_ok() {
                    return Ok(());
                }
                *guard = None;
            }
        }

        let new_session = spawn_elevated_worker_session()?;
        let mut guard = self
            .session
            .lock()
            .map_err(|_| CoreError::Ipc("Elevation lock poisoned".to_string()))?;
        *guard = Some(new_session);
        Ok(())
    }

    pub fn worker_pid(&self) -> Option<u32> {
        self.session
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|s| s.pid))
            .filter(|&pid| pid > 4)
    }

    pub fn shutdown(&self) -> Option<u32> {
        if let Ok(mut guard) = self.session.lock() {
            if let Some(mut sess) = guard.take() {
                let pid = if sess.pid > 4 { Some(sess.pid) } else { None };
                if let Ok(bytes) = serde_json::to_vec(&WorkerRequest::Shutdown) {
                    if write_frame(&mut sess.pipe, &bytes).is_ok() {
                        let _ = read_frame(&mut sess.pipe);
                    }
                }
                return pid;
            }
        }
        None
    }
}

#[cfg(windows)]
fn spawn_elevated_worker_session() -> CoreResult<WorkerSession> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use uuid::Uuid;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::GetProcessId;
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let self_pid = std::process::id();
    let pipe_name = format!(
        "\\\\.\\pipe\\Unbind_{}_{}",
        self_pid,
        Uuid::new_v4().simple()
    );
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());

    let pipe_wide: Vec<u16> = std::ffi::OsStr::new(&pipe_name)
        .encode_wide()
        .chain(Some(0))
        .collect();

    unsafe {
        let pipe_handle = CreateNamedPipeW(
            PCWSTR(pipe_wide.as_ptr()),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1, // single client instance only
            65536,
            65536,
            5000,
            None,
        );

        if pipe_handle == INVALID_HANDLE_VALUE {
            return Err(CoreError::Ipc(format!(
                "CreateNamedPipeW failed: {:?}",
                GetLastError()
            )));
        }

        let exe_path = std::env::current_exe()
            .map_err(|e| CoreError::Ipc(format!("Failed resolving current_exe: {}", e)))?;
        let exe_wide: Vec<u16> = exe_path.as_os_str().encode_wide().chain(Some(0)).collect();
        let params = format!(
            "--elevated-worker --pipe \"{}\" --token {} --parent-pid {}",
            pipe_name, token, self_pid
        );
        let params_wide: Vec<u16> = std::ffi::OsStr::new(&params)
            .encode_wide()
            .chain(Some(0))
            .collect();

        let mut sei = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("runas"),
            lpFile: PCWSTR(exe_wide.as_ptr()),
            lpParameters: PCWSTR(params_wide.as_ptr()),
            nShow: SW_HIDE.0,
            ..Default::default()
        };

        if let Err(e) = ShellExecuteExW(&mut sei) {
            let _ = CloseHandle(pipe_handle);
            return Err(CoreError::ElevationRequired(format!(
                "Administrator elevation was cancelled or denied: {}",
                e
            )));
        }

        let expected_child_pid = if !sei.hProcess.is_invalid() {
            let pid = GetProcessId(sei.hProcess);
            let _ = CloseHandle(sei.hProcess);
            pid
        } else {
            0
        };

        // Wait for our spawned elevated worker to connect.
        let connected = ConnectNamedPipe(pipe_handle, None);
        if let Err(err) = connected {
            if GetLastError() != ERROR_PIPE_CONNECTED {
                let _ = CloseHandle(pipe_handle);
                return Err(CoreError::Ipc(format!(
                    "ConnectNamedPipe failed: {}",
                    err
                )));
            }
        }

        // Verify connected peer PID matches spawned elevated worker PID when available.
        let mut connected_pid = expected_child_pid;
        let mut client_pid = 0u32;
        if GetNamedPipeClientProcessId(pipe_handle, &mut client_pid).is_ok() && client_pid != 0 {
            if expected_child_pid != 0 && client_pid != expected_child_pid {
                let _ = CloseHandle(pipe_handle);
                return Err(CoreError::Ipc(
                    "Rejected unexpected process on elevation pipe".to_string(),
                ));
            }
            connected_pid = client_pid;
        }

        let mut file = File::from_raw_handle(pipe_handle.0 as *mut _);

        // Verify session auth handshake token.
        let auth_raw = read_frame(&mut file)?;
        let auth_req: WorkerRequest = serde_json::from_slice(&auth_raw)?;
        match auth_req {
            WorkerRequest::Auth { token: got } if got == token => {
                let ack = serde_json::to_vec(&WorkerResponse::Ok(serde_json::json!({"ok": true})))?;
                write_frame(&mut file, &ack)?;
                Ok(WorkerSession {
                    pipe: file,
                    pid: connected_pid,
                })
            }
            _ => {
                let rej = serde_json::to_vec(&WorkerResponse::Err("Invalid token".into()))?;
                let _ = write_frame(&mut file, &rej);
                Err(CoreError::Ipc("Worker token handshake failed".to_string()))
            }
        }
    }
}

#[cfg(not(windows))]
fn spawn_elevated_worker_session() -> CoreResult<WorkerSession> {
    Err(CoreError::WinApi("Windows only".to_string()))
}
