use crate::models::{CoreError, CoreResult, EtwCaptureResult, EtwSessionStats, TrackedRegValue};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::thread::JoinHandle;

const MAX_PENDING_EVENTS: usize = 16_384;
const MAX_CAPTURED_FILES: usize = 32_768;
const MAX_CAPTURED_REG_ITEMS: usize = 16_384;
const MAX_KEY_OBJECT_CACHE: usize = 32_768;

#[derive(Debug, Clone)]
enum PendingEvent {
    ProcessStart {
        pid: u32,
        parent_pid: u32,
        image_name: String,
    },
    FilePath {
        pid: u32,
        dos_path: String,
    },
    RegCreateKey {
        pid: u32,
        key_path: String,
        created_new: bool,
    },
    RegSetValue {
        pid: u32,
        key_path: String,
        value_name: String,
    },
}

/// Shared state updated in real time by the ETW consumer callback thread.
#[derive(Debug)]
struct EtwSessionState {
    session_id: String,
    active: bool,
    bound: bool,
    track_msi_service: bool,
    root_pid: Option<u32>,
    /// All PIDs belonging to the installer process tree (kept even after exit for late-flushed events).
    tracked_pids: HashSet<u32>,
    /// Parent map used when replaying pre-bind events.
    parent_map: HashMap<u32, u32>,
    /// Pre-bind event buffer (captures events in the brief window before `bind_root_pid` arrives).
    pending_events: Vec<PendingEvent>,
    /// Kernel `KeyObject` handle -> normalized registry key path cache for resolving relative keys/values.
    reg_key_objects: HashMap<u64, String>,
    /// Captured DOS filesystem paths (both files and directories; filtered by existence on finish).
    captured_paths: HashSet<String>,
    /// Captured registry keys created by tracked PIDs.
    captured_reg_keys: HashSet<String>,
    /// Captured registry values written by tracked PIDs.
    captured_reg_values: HashSet<TrackedRegValue>,
    total_events: u64,
}

impl EtwSessionState {
    fn new(session_id: String, track_msi_service: bool) -> Self {
        Self {
            session_id,
            active: true,
            bound: false,
            track_msi_service,
            root_pid: None,
            tracked_pids: HashSet::new(),
            parent_map: HashMap::new(),
            pending_events: Vec::with_capacity(256),
            reg_key_objects: HashMap::with_capacity(1024),
            captured_paths: HashSet::new(),
            captured_reg_keys: HashSet::new(),
            captured_reg_values: HashSet::new(),
            total_events: 0,
        }
    }

    fn is_tracked_pid(&self, pid: u32) -> bool {
        pid > 4 && self.tracked_pids.contains(&pid)
    }

    fn record_process_start(&mut self, pid: u32, parent_pid: u32, image_name: String) {
        if pid <= 4 {
            return;
        }
        self.parent_map.insert(pid, parent_pid);
        if !self.bound {
            if self.pending_events.len() < MAX_PENDING_EVENTS {
                self.pending_events.push(PendingEvent::ProcessStart {
                    pid,
                    parent_pid,
                    image_name,
                });
            }
            return;
        }

        let is_descendant = self.tracked_pids.contains(&parent_pid);
        let is_msi_worker =
            self.track_msi_service && image_name.to_ascii_lowercase().ends_with("msiexec.exe");
        if is_descendant || is_msi_worker {
            self.tracked_pids.insert(pid);
            self.total_events += 1;
        }
    }

    fn record_file_path(&mut self, pid: u32, dos_path: String) {
        if dos_path.is_empty() {
            return;
        }
        if !self.bound {
            if self.pending_events.len() < MAX_PENDING_EVENTS {
                self.pending_events
                    .push(PendingEvent::FilePath { pid, dos_path });
            }
            return;
        }
        if self.is_tracked_pid(pid) {
            self.total_events += 1;
            if self.captured_paths.len() < MAX_CAPTURED_FILES {
                self.captured_paths.insert(dos_path);
            }
        }
    }

    fn record_reg_create_key(&mut self, pid: u32, key_path: String, created_new: bool) {
        if key_path.is_empty() {
            return;
        }
        if !self.bound {
            if self.pending_events.len() < MAX_PENDING_EVENTS {
                self.pending_events.push(PendingEvent::RegCreateKey {
                    pid,
                    key_path,
                    created_new,
                });
            }
            return;
        }
        if self.is_tracked_pid(pid) {
            self.total_events += 1;
            if created_new && self.captured_reg_keys.len() < MAX_CAPTURED_REG_ITEMS {
                self.captured_reg_keys.insert(key_path);
            }
        }
    }

    fn record_reg_set_value(&mut self, pid: u32, key_path: String, value_name: String) {
        if key_path.is_empty() || value_name.is_empty() {
            return;
        }
        if !self.bound {
            if self.pending_events.len() < MAX_PENDING_EVENTS {
                self.pending_events.push(PendingEvent::RegSetValue {
                    pid,
                    key_path,
                    value_name,
                });
            }
            return;
        }
        if self.is_tracked_pid(pid) {
            self.total_events += 1;
            if self.captured_reg_values.len() < MAX_CAPTURED_REG_ITEMS {
                self.captured_reg_values.insert(TrackedRegValue {
                    key_path,
                    value_name,
                });
            }
        }
    }

    /// Bind the installer root PID (and optional existing `msiexec.exe` service PIDs) and replay
    /// buffered events captured during the pre-bind window so zero startup activity is missed.
    fn bind_root_pid(&mut self, root_pid: u32, extra_msi_pids: &[u32]) {
        self.root_pid = Some(root_pid);
        self.bound = true;
        if root_pid > 4 {
            self.tracked_pids.insert(root_pid);
        }
        for &p in extra_msi_pids {
            if p > 4 {
                self.tracked_pids.insert(p);
            }
        }

        // First pass: expand descendant PID tree from recorded parent_map (fixpoint loop).
        let mut changed = true;
        while changed {
            changed = false;
            for (&child, &parent) in &self.parent_map {
                if self.tracked_pids.contains(&parent) && self.tracked_pids.insert(child) {
                    changed = true;
                }
            }
        }

        // Second pass: replay buffered events against the now-known PID tree.
        let buffered = std::mem::take(&mut self.pending_events);
        for ev in buffered {
            match ev {
                PendingEvent::ProcessStart {
                    pid,
                    parent_pid,
                    image_name,
                } => {
                    self.record_process_start(pid, parent_pid, image_name);
                }
                PendingEvent::FilePath { pid, dos_path } => {
                    self.record_file_path(pid, dos_path);
                }
                PendingEvent::RegCreateKey {
                    pid,
                    key_path,
                    created_new,
                } => {
                    self.record_reg_create_key(pid, key_path, created_new);
                }
                PendingEvent::RegSetValue {
                    pid,
                    key_path,
                    value_name,
                } => {
                    self.record_reg_set_value(pid, key_path, value_name);
                }
            }
        }
    }

    fn stats(&self) -> EtwSessionStats {
        EtwSessionStats {
            session_id: self.session_id.clone(),
            etw_active: self.active,
            tracked_pid_count: self.tracked_pids.len(),
            captured_file_count: self.captured_paths.len(),
            captured_reg_count: self.captured_reg_keys.len() + self.captured_reg_values.len(),
            total_events: self.total_events,
        }
    }

    fn to_capture_result(&self) -> EtwCaptureResult {
        let mut added_dirs = Vec::new();
        let mut added_files = Vec::new();

        for p in &self.captured_paths {
            let path = std::path::Path::new(p);
            if path.is_dir() {
                added_dirs.push(p.clone());
            } else {
                added_files.push(p.clone());
            }
        }

        let mut added_registry_keys: Vec<String> = self.captured_reg_keys.iter().cloned().collect();
        added_registry_keys.sort();

        let added_registry_values: Vec<TrackedRegValue> =
            self.captured_reg_values.iter().cloned().collect();

        let mut tracked_pids: Vec<u32> = self.tracked_pids.iter().copied().collect();
        tracked_pids.sort_unstable();

        EtwCaptureResult {
            added_files,
            added_dirs,
            added_registry_keys,
            added_registry_values,
            tracked_pids,
            total_events: self.total_events,
        }
    }
}

#[cfg(windows)]
struct EtwCallbackContext {
    state: Arc<Mutex<EtwSessionState>>,
    device_map: HashMap<String, String>,
    stop_flag: Arc<AtomicBool>,
}

struct RunningEtwSession {
    #[cfg(windows)]
    etw_session_name: String,
    #[cfg(windows)]
    control_handle: u64,
    #[cfg(windows)]
    trace_handle: u64,
    #[cfg(windows)]
    stop_flag: Arc<AtomicBool>,
    #[cfg(windows)]
    consumer_thread: Option<JoinHandle<()>>,
    state: Arc<Mutex<EtwSessionState>>,
}

/// Manages active named real-time ETW sessions inside the elevated worker (or in-process when
/// already running as Administrator). Guarantees clean `ControlTraceW(STOP)` on all exit paths.
#[derive(Default)]
pub struct EtwTrackerManager {
    sessions: Mutex<HashMap<String, RunningEtwSession>>,
}

impl EtwTrackerManager {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(windows)]
    pub fn start_session(
        &self,
        session_id: &str,
        track_msi_service: bool,
    ) -> CoreResult<EtwSessionStats> {
        use std::ffi::c_void;
        use std::mem::size_of;
        use windows::core::{GUID, PCWSTR, PWSTR};
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Diagnostics::Etw::{
            CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
            CONTROLTRACE_HANDLE, EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_TRACE_CONTROL_STOP,
            EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE,
            PROCESSTRACE_HANDLE, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME,
            TRACE_LEVEL_INFORMATION, WNODE_FLAG_TRACED_GUID,
        };

        let short_id: String = session_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(12)
            .collect();
        let etw_session_name = format!("UnbindTrack_{}", short_id);
        let name_wide: Vec<u16> = etw_session_name
            .encode_utf16()
            .chain(Some(0))
            .collect();

        // Build aligned EVENT_TRACE_PROPERTIES buffer followed by logger name string.
        let mut props_buf = build_trace_properties_buf(&name_wide);
        let props_ptr = props_buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;

        let mut control_handle = CONTROLTRACE_HANDLE::default();
        let start_err = unsafe {
            StartTraceW(
                &mut control_handle,
                PCWSTR(name_wide.as_ptr()),
                props_ptr,
            )
        };
        if start_err != ERROR_SUCCESS {
            return Err(CoreError::WinApi(format!(
                "StartTraceW failed for '{}' with error {:?}",
                etw_session_name, start_err
            )));
        }

        // Subscribe to Windows built-in manifest-based kernel providers with minimal keywords.
        // Microsoft-Windows-Kernel-Process: {22FB2CD6-0E7B-422B-A0C7-2FAD1FD0E716}
        let proc_guid = GUID::from_u128(0x22fb2cd6_0e7b_422b_a0c7_2fad1fd0e716);
        // Microsoft-Windows-Kernel-File: {EDD08927-9CC4-4E65-B970-C2560FB5C289}
        let file_guid = GUID::from_u128(0xedd08927_9cc4_4e65_b970_c2560fb5c289);
        // Microsoft-Windows-Kernel-Registry: {70EB4F03-C1DE-4F73-A051-33D13D5413BD}
        let reg_guid = GUID::from_u128(0x70eb4f03_c1de_4f73_a051_33d13d5413bd);

        let providers: [(&str, GUID, u64); 3] = [
            // WINEVENT_KEYWORD_PROCESS (0x10)
            ("Kernel-Process", proc_guid, 0x0010),
            // CREATE (0x80) | RENAME_SETLINK_PATH (0x800) | CREATE_NEW_FILE (0x1000)
            ("Kernel-File", file_guid, 0x0080 | 0x0800 | 0x1000),
            // SetValueKey (0x100) | CreateKey (0x1000) | OpenKey (0x2000)
            ("Kernel-Registry", reg_guid, 0x0100 | 0x1000 | 0x2000),
        ];

        for (label, guid, keywords) in providers {
            let en_err = unsafe {
                EnableTraceEx2(
                    control_handle,
                    &guid,
                    EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
                    TRACE_LEVEL_INFORMATION as u8,
                    keywords,
                    0,
                    0,
                    None,
                )
            };
            if en_err != ERROR_SUCCESS {
                let mut stop_buf = build_trace_properties_buf(&name_wide);
                unsafe {
                    let _ = ControlTraceW(
                        control_handle,
                        PCWSTR(name_wide.as_ptr()),
                        stop_buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                        EVENT_TRACE_CONTROL_STOP,
                    );
                }
                return Err(CoreError::WinApi(format!(
                    "EnableTraceEx2 failed for {} with error {:?}",
                    label, en_err
                )));
            }
        }

        let state = Arc::new(Mutex::new(EtwSessionState::new(
            session_id.to_string(),
            track_msi_service,
        )));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let ctx = Arc::new(EtwCallbackContext {
            state: Arc::clone(&state),
            device_map: build_dos_device_map(),
            stop_flag: Arc::clone(&stop_flag),
        });
        let ctx_raw = Arc::into_raw(ctx) as *mut c_void;

        let mut logger_name_mut = name_wide.clone();
        let mut logfile = EVENT_TRACE_LOGFILEW {
            LoggerName: PWSTR(logger_name_mut.as_mut_ptr()),
            Context: ctx_raw,
            ..Default::default()
        };
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(etw_event_record_callback);

        let trace_handle = unsafe { OpenTraceW(&mut logfile) };
        if trace_handle.Value == u64::MAX {
            unsafe {
                let _ = Arc::from_raw(ctx_raw as *const EtwCallbackContext);
                let mut stop_buf = build_trace_properties_buf(&name_wide);
                let _ = ControlTraceW(
                    control_handle,
                    PCWSTR(name_wide.as_ptr()),
                    stop_buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                    EVENT_TRACE_CONTROL_STOP,
                );
            }
            return Err(CoreError::WinApi(format!(
                "OpenTraceW failed for '{}'",
                etw_session_name
            )));
        }

        let th_val = trace_handle.Value;
        let ctx_addr = ctx_raw as usize;
        let state_for_thread = Arc::clone(&state);
        let consumer_thread = std::thread::Builder::new()
            .name(format!("etw-{}", short_id))
            .spawn(move || {
                let handles = [PROCESSTRACE_HANDLE { Value: th_val }];
                unsafe {
                    let _ = ProcessTrace(&handles, None, None);
                    let _ = CloseTrace(PROCESSTRACE_HANDLE { Value: th_val });
                    // Reclaim context Arc once ProcessTrace has exited and no more callbacks run.
                    let _ = Arc::from_raw(ctx_addr as *const EtwCallbackContext);
                }
                if let Ok(mut g) = state_for_thread.lock() {
                    g.active = false;
                }
            })
            .map_err(|e| {
                unsafe {
                    let _ = CloseTrace(trace_handle);
                    let _ = Arc::from_raw(ctx_raw as *const EtwCallbackContext);
                    let mut stop_buf = build_trace_properties_buf(&name_wide);
                    let _ = ControlTraceW(
                        control_handle,
                        PCWSTR(name_wide.as_ptr()),
                        stop_buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
                        EVENT_TRACE_CONTROL_STOP,
                    );
                }
                CoreError::Io(e)
            })?;

        let _ = size_of::<EVENT_TRACE_PROPERTIES>();
        let _ = WNODE_FLAG_TRACED_GUID;
        let _ = EVENT_TRACE_REAL_TIME_MODE;

        let stats = state
            .lock()
            .map(|g| g.stats())
            .unwrap_or_else(|_| EtwSessionStats {
                session_id: session_id.to_string(),
                etw_active: true,
                ..Default::default()
            });

        let running = RunningEtwSession {
            etw_session_name,
            control_handle: control_handle.Value,
            trace_handle: trace_handle.Value,
            stop_flag,
            consumer_thread: Some(consumer_thread),
            state,
        };

        if let Ok(mut guard) = self.sessions.lock() {
            if let Some(prev) = guard.insert(session_id.to_string(), running) {
                stop_running_session(prev, false);
            }
        }

        Ok(stats)
    }

    #[cfg(not(windows))]
    pub fn start_session(
        &self,
        session_id: &str,
        _track_msi_service: bool,
    ) -> CoreResult<EtwSessionStats> {
        Err(CoreError::WinApi(format!(
            "ETW tracking is only supported on Windows (session {})",
            session_id
        )))
    }

    pub fn bind_root_pid(&self, session_id: &str, root_pid: u32) -> CoreResult<EtwSessionStats> {
        let guard = self
            .sessions
            .lock()
            .map_err(|_| CoreError::WinApi("ETW sessions lock poisoned".to_string()))?;
        let sess = guard.get(session_id).ok_or_else(|| {
            CoreError::NotFound(format!("ETW session '{}' not found", session_id))
        })?;

        let mut st = sess
            .state
            .lock()
            .map_err(|_| CoreError::WinApi("ETW state lock poisoned".to_string()))?;

        let extra_msi_pids = if st.track_msi_service {
            find_msiexec_pids()
        } else {
            Vec::new()
        };
        st.bind_root_pid(root_pid, &extra_msi_pids);
        Ok(st.stats())
    }

    pub fn poll_stats(&self, session_id: &str) -> Option<EtwSessionStats> {
        let guard = self.sessions.lock().ok()?;
        let sess = guard.get(session_id)?;
        let st = sess.state.lock().ok()?;
        Some(st.stats())
    }

    pub fn stop_session(&self, session_id: &str) -> CoreResult<EtwCaptureResult> {
        let running = {
            let mut guard = self
                .sessions
                .lock()
                .map_err(|_| CoreError::WinApi("ETW sessions lock poisoned".to_string()))?;
            guard.remove(session_id).ok_or_else(|| {
                CoreError::NotFound(format!("ETW session '{}' not found", session_id))
            })?
        };

        let state_arc = Arc::clone(&running.state);
        // Drain final flushed kernel buffers completely before returning capture results.
        stop_running_session(running, true);

        let result = state_arc
            .lock()
            .map(|g| g.to_capture_result())
            .unwrap_or_default();
        Ok(result)
    }

    pub fn cancel_session(&self, session_id: &str) -> bool {
        let running = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut g| g.remove(session_id));
        if let Some(sess) = running {
            stop_running_session(sess, false);
            true
        } else {
            false
        }
    }

    /// Stop every active ETW session cleanly (called on Shutdown, pipe disconnect, and watchdog exit).
    pub fn stop_all_sessions(&self) {
        let drained: Vec<RunningEtwSession> = self
            .sessions
            .lock()
            .map(|mut g| g.drain().map(|(_, v)| v).collect())
            .unwrap_or_default();
        for sess in drained {
            stop_running_session(sess, false);
        }
    }
}

impl Drop for EtwTrackerManager {
    fn drop(&mut self) {
        self.stop_all_sessions();
    }
}

#[cfg(windows)]
fn stop_running_session(mut running: RunningEtwSession, drain_final_buffers: bool) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Diagnostics::Etw::{
        CloseTrace, ControlTraceW, CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL_STOP,
        EVENT_TRACE_PROPERTIES, PROCESSTRACE_HANDLE,
    };

    // Only short-circuit callback processing on cancel/teardown. On normal stop_session(), keep
    // stop_flag false so ControlTraceW(STOP) flushes all final per-CPU buffers into our state.
    if !drain_final_buffers {
        running.stop_flag.store(true, Ordering::SeqCst);
    }

    let name_wide: Vec<u16> = running
        .etw_session_name
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut stop_buf = build_trace_properties_buf(&name_wide);
    unsafe {
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE {
                Value: running.control_handle,
            },
            PCWSTR(name_wide.as_ptr()),
            stop_buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES,
            EVENT_TRACE_CONTROL_STOP,
        );
    }

    if let Some(handle) = running.consumer_thread.take() {
        // ControlTraceW(STOP) causes ProcessTrace to flush final buffers and return promptly.
        let _ = handle.join();
    } else if running.trace_handle != u64::MAX {
        unsafe {
            let _ = CloseTrace(PROCESSTRACE_HANDLE {
                Value: running.trace_handle,
            });
        }
    }
    running.stop_flag.store(true, Ordering::SeqCst);
}

#[cfg(not(windows))]
fn stop_running_session(_running: RunningEtwSession, _drain_final_buffers: bool) {}

#[cfg(windows)]
fn build_trace_properties_buf(name_wide: &[u16]) -> Vec<u64> {
    use std::mem::size_of;
    use windows::Win32::System::Diagnostics::Etw::{
        EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, WNODE_FLAG_TRACED_GUID,
    };

    let props_size = size_of::<EVENT_TRACE_PROPERTIES>();
    let name_bytes = std::mem::size_of_val(name_wide);
    // Reserve generous room for logger name + optional log file name.
    let total_bytes = props_size + name_bytes + 512;
    let u64_len = total_bytes.div_ceil(size_of::<u64>());
    let mut buf = vec![0u64; u64_len];

    unsafe {
        let props = &mut *(buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES);
        props.Wnode.BufferSize = (u64_len * size_of::<u64>()) as u32;
        props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        props.Wnode.ClientContext = 1; // QPC high-resolution timestamps
        props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        props.BufferSize = 64; // 64 KB per ETW buffer
        props.MinimumBuffers = 8;
        props.MaximumBuffers = 64;
        props.FlushTimer = 1; // flush every 1 second for live UI counters
        props.LoggerNameOffset = props_size as u32;
    }

    buf
}

#[cfg(windows)]
unsafe extern "system" fn etw_event_record_callback(
    event_record: *mut windows::Win32::System::Diagnostics::Etw::EVENT_RECORD,
) {
    use windows::core::GUID;

    if event_record.is_null() {
        return;
    }
    let record = &*event_record;
    if record.UserContext.is_null() {
        return;
    }
    let ctx = &*(record.UserContext as *const EtwCallbackContext);
    if ctx.stop_flag.load(Ordering::Relaxed) {
        return;
    }

    const PROC_GUID: GUID = GUID::from_u128(0x22fb2cd6_0e7b_422b_a0c7_2fad1fd0e716);
    const FILE_GUID: GUID = GUID::from_u128(0xedd08927_9cc4_4e65_b970_c2560fb5c289);
    const REG_GUID: GUID = GUID::from_u128(0x70eb4f03_c1de_4f73_a051_33d13d5413bd);

    let provider = record.EventHeader.ProviderId;
    let event_id = record.EventHeader.EventDescriptor.Id;
    let header_pid = record.EventHeader.ProcessId;

    if provider == PROC_GUID {
        if event_id == 1 {
            // ProcessStart
            let child_pid = tdh_read_u32(record, "ProcessID").unwrap_or(0);
            let parent_pid = tdh_read_u32(record, "ParentProcessID").unwrap_or(header_pid);
            let image_name = tdh_read_wstring(record, "ImageName").unwrap_or_default();
            if child_pid > 4 {
                if let Ok(mut st) = ctx.state.lock() {
                    st.record_process_start(child_pid, parent_pid, image_name);
                }
            }
        }
        return;
    }

    // Fast-path PID check before invoking TDH for high-volume File and Registry providers.
    let should_decode = match ctx.state.lock() {
        Ok(st) => !st.bound || st.is_tracked_pid(header_pid),
        Err(_) => false,
    };
    if !should_decode {
        return;
    }

    if provider == FILE_GUID {
        // 12 = Create, 30 = CreateNewFile, 27 = RenamePath
        if matches!(event_id, 12 | 27 | 30) {
            let raw_name = if event_id == 27 {
                tdh_read_wstring(record, "FilePath")
                    .or_else(|| tdh_read_wstring(record, "FileName"))
            } else {
                tdh_read_wstring(record, "FileName")
            };
            if let Some(nt_path) = raw_name {
                if let Some(dos_path) = convert_nt_device_path_to_dos(&nt_path, &ctx.device_map) {
                    if let Ok(mut st) = ctx.state.lock() {
                        st.record_file_path(header_pid, dos_path);
                    }
                }
            }
        }
        return;
    }

    if provider == REG_GUID {
        // 1 = CreateKey, 2 = OpenKey, 5 = SetValueKey
        match event_id {
            1 | 2 => {
                let base_obj = tdh_read_ptr(record, "BaseObject").unwrap_or(0);
                let key_obj = tdh_read_ptr(record, "KeyObject").unwrap_or(0);
                let rel_name = tdh_read_wstring(record, "RelativeName").unwrap_or_default();
                let disposition = if event_id == 1 {
                    tdh_read_u32(record, "Disposition").unwrap_or(0)
                } else {
                    0
                };

                if let Ok(mut st) = ctx.state.lock() {
                    let full_native = if base_obj != 0 {
                        if let Some(base_path) = st.reg_key_objects.get(&base_obj) {
                            join_registry_path(base_path, &rel_name)
                        } else {
                            rel_name
                        }
                    } else {
                        rel_name
                    };

                    if let Some(norm_key) = normalize_kernel_registry_path(&full_native) {
                        if key_obj != 0 {
                            if st.reg_key_objects.len() >= MAX_KEY_OBJECT_CACHE {
                                st.reg_key_objects.clear();
                            }
                            st.reg_key_objects.insert(key_obj, norm_key.clone());
                        }
                        if event_id == 1 {
                            // REG_CREATED_NEW_KEY == 1 (also accept 0 when disposition is absent)
                            let created_new = disposition == 1 || disposition == 0;
                            st.record_reg_create_key(header_pid, norm_key, created_new);
                        }
                    }
                }
            }
            5 => {
                let key_obj = tdh_read_ptr(record, "KeyObject").unwrap_or(0);
                let value_name = tdh_read_wstring(record, "ValueName").unwrap_or_default();
                if key_obj != 0 && !value_name.is_empty() {
                    if let Ok(mut st) = ctx.state.lock() {
                        if let Some(key_path) = st.reg_key_objects.get(&key_obj).cloned() {
                            st.record_reg_set_value(header_pid, key_path, value_name);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(windows)]
unsafe fn tdh_read_bytes(
    record: &windows::Win32::System::Diagnostics::Etw::EVENT_RECORD,
    prop_name: &str,
) -> Option<Vec<u8>> {
    use windows::Win32::System::Diagnostics::Etw::{
        TdhGetProperty, TdhGetPropertySize, PROPERTY_DATA_DESCRIPTOR,
    };

    let wide: Vec<u16> = prop_name.encode_utf16().chain(Some(0)).collect();
    let desc = [PROPERTY_DATA_DESCRIPTOR {
        PropertyName: wide.as_ptr() as u64,
        ArrayIndex: u32::MAX,
        Reserved: 0,
    }];

    let mut size: u32 = 0;
    if TdhGetPropertySize(record, None, &desc, &mut size) != 0 || size == 0 || size > 16_384 {
        return None;
    }

    let mut buf = vec![0u8; size as usize];
    if TdhGetProperty(record, None, &desc, &mut buf) != 0 {
        return None;
    }
    Some(buf)
}

#[cfg(windows)]
unsafe fn tdh_read_u32(
    record: &windows::Win32::System::Diagnostics::Etw::EVENT_RECORD,
    prop_name: &str,
) -> Option<u32> {
    let bytes = tdh_read_bytes(record, prop_name)?;
    if bytes.len() >= 4 {
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    } else {
        None
    }
}

#[cfg(windows)]
unsafe fn tdh_read_ptr(
    record: &windows::Win32::System::Diagnostics::Etw::EVENT_RECORD,
    prop_name: &str,
) -> Option<u64> {
    let bytes = tdh_read_bytes(record, prop_name)?;
    if bytes.len() >= 8 {
        Some(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    } else if bytes.len() >= 4 {
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64)
    } else {
        None
    }
}

#[cfg(windows)]
unsafe fn tdh_read_wstring(
    record: &windows::Win32::System::Diagnostics::Etw::EVENT_RECORD,
    prop_name: &str,
) -> Option<String> {
    let bytes = tdh_read_bytes(record, prop_name)?;
    if bytes.len() < 2 {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    let s = String::from_utf16_lossy(&units[..end]).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Build a mapping from lowercase NT device paths (e.g. `\device\harddiskvolume3`) to DOS drive
/// roots (e.g. `C:`) using `QueryDosDeviceW`.
#[cfg(windows)]
pub fn build_dos_device_map() -> HashMap<String, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::QueryDosDeviceW;

    let mut map = HashMap::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:", letter as char);
        let wide_drive: Vec<u16> = drive.encode_utf16().chain(Some(0)).collect();
        let mut target_buf = [0u16; 512];
        let len = unsafe { QueryDosDeviceW(PCWSTR(wide_drive.as_ptr()), Some(&mut target_buf)) };
        if len > 0 {
            let end = target_buf
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(len as usize);
            let nt_dev = String::from_utf16_lossy(&target_buf[..end])
                .trim()
                .to_lowercase();
            if !nt_dev.is_empty() {
                map.insert(nt_dev, drive);
            }
        }
    }
    map
}

#[cfg(not(windows))]
pub fn build_dos_device_map() -> HashMap<String, String> {
    HashMap::new()
}

/// Convert an NT kernel path (`\Device\HarddiskVolume3\Program Files\Foo\bar.exe`) into a standard
/// DOS path (`C:\Program Files\Foo\bar.exe`) using the cached device map.
pub fn convert_nt_device_path_to_dos(
    nt_path: &str,
    device_map: &HashMap<String, String>,
) -> Option<String> {
    let trimmed = nt_path.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(stripped) = trimmed.strip_prefix("\\??\\") {
        if stripped.len() >= 3 && stripped.as_bytes()[1] == b':' {
            return Some(stripped.to_string());
        }
    }
    if trimmed.len() >= 3 && trimmed.as_bytes()[1] == b':' {
        return Some(trimmed.to_string());
    }

    let lower = trimmed.to_lowercase();
    for (dev_prefix, drive) in device_map {
        if lower == *dev_prefix {
            return Some(format!("{}\\", drive));
        }
        if lower.starts_with(dev_prefix) {
            let rest = &trimmed[dev_prefix.len()..];
            if rest.starts_with('\\') {
                return Some(format!("{}{}", drive, rest));
            }
        }
    }
    None
}

fn join_registry_path(base: &str, rel: &str) -> String {
    let r = rel.trim_start_matches('\\');
    if r.is_empty() {
        return base.to_string();
    }
    // If relative name is already a full kernel or hive path, prefer it directly.
    let rl = r.to_ascii_lowercase();
    if rl.starts_with("registry\\") || rl.starts_with("hklm\\") || rl.starts_with("hkcu\\") {
        return rel.to_string();
    }
    format!("{}\\{}", base.trim_end_matches('\\'), r)
}

/// Normalize kernel-format registry paths (`\REGISTRY\MACHINE\...`, `\REGISTRY\USER\<SID>\...`)
/// into Unbind's canonical `HKLM\...` / `HKCU\...` representation.
pub fn normalize_kernel_registry_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('\\');
    if trimmed.is_empty() {
        return None;
    }

    // Already normalized?
    let upper = trimmed.to_ascii_uppercase();
    if upper.starts_with("HKLM\\") || upper.starts_with("HKCU\\") {
        return Some(trimmed.to_string());
    }

    if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "\\REGISTRY\\MACHINE\\") {
        if rest.is_empty() {
            return None;
        }
        return Some(format!("HKLM\\{}", rest));
    }

    if let Some(rest) = strip_prefix_ignore_ascii_case(trimmed, "\\REGISTRY\\USER\\") {
        // Expect `<SID>\<subkey>` or `<SID>_Classes\<subkey>`.
        let (sid_part, sub) = rest.split_once('\\')?;
        let sid_upper = sid_part.to_ascii_uppercase();
        // Skip system service hives (.DEFAULT, S-1-5-18, S-1-5-19, S-1-5-20).
        if sid_upper == ".DEFAULT"
            || sid_upper == "S-1-5-18"
            || sid_upper == "S-1-5-19"
            || sid_upper == "S-1-5-20"
            || !sid_upper.starts_with("S-1-5-21-")
        {
            return None;
        }
        if sid_upper.ends_with("_CLASSES") {
            return Some(format!("HKCU\\Software\\Classes\\{}", sub));
        }
        return Some(format!("HKCU\\{}", sub));
    }

    None
}

fn strip_prefix_ignore_ascii_case<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

#[cfg(windows)]
fn find_msiexec_pids() -> Vec<u32> {
    use std::mem::size_of;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut pids = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return pids;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if name.eq_ignore_ascii_case("msiexec.exe") && entry.th32ProcessID > 4 {
                    pids.push(entry.th32ProcessID);
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

#[cfg(not(windows))]
fn find_msiexec_pids() -> Vec<u32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_nt_device_path_to_dos() {
        let mut map = HashMap::new();
        map.insert("\\device\\harddiskvolume3".to_string(), "C:".to_string());
        map.insert("\\device\\harddiskvolume5".to_string(), "D:".to_string());

        assert_eq!(
            convert_nt_device_path_to_dos("\\Device\\HarddiskVolume3\\Program Files\\App\\a.exe", &map),
            Some("C:\\Program Files\\App\\a.exe".to_string())
        );
        assert_eq!(
            convert_nt_device_path_to_dos("\\Device\\HarddiskVolume5\\Apps\\tool.dll", &map),
            Some("D:\\Apps\\tool.dll".to_string())
        );
        assert_eq!(
            convert_nt_device_path_to_dos("\\??\\C:\\Users\\Test\\file.txt", &map),
            Some("C:\\Users\\Test\\file.txt".to_string())
        );
        assert_eq!(
            convert_nt_device_path_to_dos("\\Device\\NamedPipe\\foo", &map),
            None
        );
    }

    #[test]
    fn test_normalize_kernel_registry_path() {
        assert_eq!(
            normalize_kernel_registry_path("\\REGISTRY\\MACHINE\\SOFTWARE\\Vendor\\MyApp"),
            Some("HKLM\\SOFTWARE\\Vendor\\MyApp".to_string())
        );
        assert_eq!(
            normalize_kernel_registry_path(
                "\\REGISTRY\\USER\\S-1-5-21-123456-7890-1001\\Software\\MyApp"
            ),
            Some("HKCU\\Software\\MyApp".to_string())
        );
        assert_eq!(
            normalize_kernel_registry_path(
                "\\REGISTRY\\USER\\S-1-5-21-123456-7890-1001_Classes\\MyApp.File"
            ),
            Some("HKCU\\Software\\Classes\\MyApp.File".to_string())
        );
        // Built-in SYSTEM hive should be filtered out.
        assert_eq!(
            normalize_kernel_registry_path("\\REGISTRY\\USER\\S-1-5-18\\Software\\Noise"),
            None
        );
    }

    #[test]
    fn test_prebind_pid_tree_replay_isolates_installer_noise() {
        let mut st = EtwSessionState::new("sess-1".to_string(), false);
        // Events arrive BEFORE root PID 1000 is bound:
        // PID 1000 spawns child 1001 -> grandchild 1002; unrelated PID 9999 writes noise.
        st.record_process_start(1001, 1000, "subsetup.exe".to_string());
        st.record_process_start(1002, 1001, "helper.exe".to_string());
        st.record_file_path(1002, "C:\\Apps\\Demo\\app.exe".to_string());
        st.record_file_path(9999, "C:\\Users\\Me\\Chrome\\Cache\\noise.dat".to_string());
        st.record_reg_create_key(1001, "HKCU\\Software\\DemoApp".to_string(), true);
        st.record_reg_create_key(9999, "HKCU\\Software\\NoiseApp".to_string(), true);

        st.bind_root_pid(1000, &[]);

        assert!(st.tracked_pids.contains(&1000));
        assert!(st.tracked_pids.contains(&1001));
        assert!(st.tracked_pids.contains(&1002));
        assert!(!st.tracked_pids.contains(&9999));
        assert!(st.captured_paths.contains("C:\\Apps\\Demo\\app.exe"));
        assert!(!st.captured_paths.contains("C:\\Users\\Me\\Chrome\\Cache\\noise.dat"));
        assert!(st.captured_reg_keys.contains("HKCU\\Software\\DemoApp"));
        assert!(!st.captured_reg_keys.contains("HKCU\\Software\\NoiseApp"));
    }

    #[test]
    fn test_live_etw_session_smoke_when_elevated() {
        if !crate::safety::is_process_elevated() {
            // ETW kernel session creation requires Administrator privileges; skip cleanly otherwise.
            return;
        }
        let mgr = EtwTrackerManager::new();
        let sid = format!("smoke-{}", uuid::Uuid::new_v4().simple());
        let stats = mgr.start_session(&sid, false).expect("ETW start_session");
        assert!(stats.etw_active);

        let self_pid = std::process::id();
        let bound = mgr.bind_root_pid(&sid, self_pid).expect("bind_root_pid");
        assert!(bound.tracked_pid_count >= 1);

        // Allow kernel provider enablement to settle across CPUs before writing the probe file.
        std::thread::sleep(std::time::Duration::from_millis(120));

        let dir = tempfile::tempdir().unwrap();
        let probe_file = dir.path().join("etw_probe_file.txt");
        std::fs::write(&probe_file, b"hello-etw").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));

        let capture = mgr.stop_session(&sid).expect("stop_session");
        // Windows kernel ETW reports canonical long paths (e.g. "runneradmin"), whereas
        // tempdir() may use 8.3 short paths (e.g. "RUNNER~1"). Compare canonical paths.
        let target_canon = std::fs::canonicalize(&probe_file).unwrap_or_else(|_| probe_file.clone());
        let found = capture.added_files.iter().any(|p| {
            std::fs::canonicalize(p)
                .map(|c| c == target_canon)
                .unwrap_or(false)
        });
        assert!(
            found,
            "Expected ETW to capture newly created probe file {} (canonical {}), captured {} files",
            probe_file.display(),
            target_canon.display(),
            capture.added_files.len()
        );
    }
}
