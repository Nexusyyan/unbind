use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Windows API error: {0}")]
    WinApi(String),
    #[error("Safety guardrail blocked operation: {0}")]
    SafetyBlocked(String),
    #[error("Elevation required for operation: {0}")]
    ElevationRequired(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("IPC error: {0}")]
    Ipc(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppKind {
    Win32,
    Msi,
    StoreApp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallScope {
    Machine,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Arch {
    X64,
    X86,
    Arm64,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PackageManagerKind {
    #[default]
    None,
    Winget,
    Scoop,
    Chocolatey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// Normalized YYYY-MM-DD when parseable, otherwise raw or empty.
    pub install_date: String,
    /// Estimated or measured size in kilobytes (0 if unknown).
    pub estimated_size_kb: u64,
    pub install_location: String,
    pub uninstall_string: String,
    pub quiet_uninstall_string: String,
    pub display_icon: String,
    pub url_info_about: String,
    pub registry_key: String,
    pub registry_hive: String,
    pub registry_view: String,
    pub product_code: String,
    pub package_full_name: String,
    pub package_family_name: String,
    pub kind: AppKind,
    pub scope: InstallScope,
    pub arch: Arch,
    pub is_system_component: bool,
    pub is_windows_update: bool,
    pub is_valid_entry: bool,
    pub has_uninstaller: bool,
    pub tracked_trace_id: Option<String>,
    #[serde(default)]
    pub package_manager: PackageManagerKind,
    #[serde(default)]
    pub size_measured: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LeftoverKind {
    RegistryKey,
    RegistryValue,
    Directory,
    File,
    Shortcut,
    ScheduledTask,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfidenceLevel {
    High,
    Medium,
    Protected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverItem {
    pub id: String,
    pub kind: LeftoverKind,
    /// Full filesystem path or full registry key path (e.g. HKCU\Software\App).
    pub path: String,
    /// Present when kind == RegistryValue.
    pub value_name: Option<String>,
    pub size_bytes: u64,
    pub confidence: ConfidenceLevel,
    pub reason: String,
    #[serde(default)]
    pub reason_code: String,
    #[serde(default)]
    pub reason_param: Option<String>,
    pub selected_by_default: bool,
    pub requires_elevation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupOptions {
    pub use_recycle_bin: bool,
    pub backup_registry: bool,
    pub create_restore_point: bool,
    pub schedule_reboot_on_lock: bool,
    pub terminate_locking_processes: bool,
}

impl Default for CleanupOptions {
    fn default() -> Self {
        Self {
            use_recycle_bin: true,
            backup_registry: true,
            create_restore_point: false,
            schedule_reboot_on_lock: true,
            terminate_locking_processes: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItemResult {
    pub item_id: String,
    pub path: String,
    pub success: bool,
    pub scheduled_for_reboot: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub succeeded: usize,
    pub failed: usize,
    pub scheduled_for_reboot: usize,
    pub freed_bytes: u64,
    pub reg_backup_path: Option<String>,
    pub restore_point_created: bool,
    pub details: Vec<CleanupItemResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UninstallExecutionResult {
    pub launched: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockingProcessInfo {
    pub pid: u32,
    pub name: String,
    pub service_short_name: String,
    pub app_type: String,
    pub restartable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartupSource {
    RegistryCurrentUserRun,
    RegistryCurrentUserRunOnce,
    RegistryLocalMachineRun,
    RegistryLocalMachineWow64Run,
    StartupFolderUser,
    StartupFolderCommon,
    ScheduledTask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub executable_path: String,
    pub publisher: String,
    pub source: StartupSource,
    pub location: String,
    pub enabled: bool,
    pub is_valid_target: bool,
    pub requires_elevation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackMode {
    #[default]
    HybridSnapshot,
    EtwRealtime,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtwSessionStats {
    pub session_id: String,
    pub etw_active: bool,
    pub tracked_pid_count: usize,
    pub captured_file_count: usize,
    pub captured_reg_count: usize,
    pub total_events: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EtwCaptureResult {
    pub added_files: Vec<String>,
    pub added_dirs: Vec<String>,
    pub added_registry_keys: Vec<String>,
    pub added_registry_values: Vec<TrackedRegValue>,
    pub tracked_pids: Vec<u32>,
    pub total_events: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallTrace {
    pub id: String,
    pub app_name: String,
    pub installer_path: String,
    pub started_at: String,
    pub completed_at: String,
    pub added_dirs: Vec<String>,
    pub added_files: Vec<String>,
    pub added_registry_keys: Vec<String>,
    pub added_registry_values: Vec<TrackedRegValue>,
    #[serde(default)]
    pub added_services: Vec<String>,
    #[serde(default)]
    pub added_scheduled_tasks: Vec<String>,
    #[serde(default)]
    pub watched_roots: Vec<String>,
    pub total_size_bytes: u64,
    #[serde(default)]
    pub track_mode: TrackMode,
    #[serde(default)]
    pub etw_event_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedRegValue {
    pub key_path: String,
    pub value_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveTrackSession {
    pub session_id: String,
    pub app_name: String,
    pub installer_path: String,
    pub started_at: String,
    pub baseline_dir_count: usize,
    pub baseline_file_count: usize,
    pub baseline_reg_count: usize,
    #[serde(default)]
    pub watched_roots: Vec<String>,
    #[serde(default)]
    pub installer_pid: Option<u32>,
    #[serde(default)]
    pub installer_exited: bool,
    #[serde(default)]
    pub track_mode: TrackMode,
    #[serde(default)]
    pub etw_active: bool,
    #[serde(default)]
    pub tracked_pid_count: usize,
    #[serde(default)]
    pub captured_file_count: usize,
    #[serde(default)]
    pub captured_reg_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableApp {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    pub exe_path: String,
    pub install_dir: String,
    pub root_dir: String,
    pub size_bytes: u64,
    pub last_modified: String,
    pub is_single_file: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutLocation {
    Desktop,
    StartMenu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetUpdateInfo {
    pub name: String,
    pub package_id: String,
    pub current_version: String,
    pub available_version: String,
    pub source: String,
    pub matched_app_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetCheckResult {
    pub available: bool,
    pub winget_version: String,
    pub updates: Vec<WingetUpdateInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WingetUpgradeResult {
    pub package_id: String,
    pub app_name: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub id: String,
    pub filename: String,
    pub path: String,
    pub created_at: String,
    pub size_bytes: u64,
    pub app_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationLogEntry {
    pub timestamp: String,
    pub action: String,
    pub target: String,
    pub summary: String,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowBounds {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub portable_mode: bool,
    pub data_dir: String,
    pub use_recycle_bin: bool,
    pub auto_backup_registry: bool,
    pub create_restore_point: bool,
    pub show_system_components: bool,
    pub show_windows_updates: bool,
    pub language: String,
    pub theme: String,
    #[serde(default)]
    pub custom_track_roots: Vec<String>,
    #[serde(default)]
    pub portable_app_roots: Vec<String>,
    #[serde(default)]
    pub default_track_mode: TrackMode,
    #[serde(default)]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub window_bounds: Option<WindowBounds>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            portable_mode: false,
            data_dir: String::new(),
            use_recycle_bin: true,
            auto_backup_registry: true,
            create_restore_point: false,
            show_system_components: false,
            show_windows_updates: false,
            language: "auto".to_string(),
            theme: "system".to_string(),
            custom_track_roots: Vec::new(),
            portable_app_roots: Vec::new(),
            default_track_mode: TrackMode::HybridSnapshot,
            close_to_tray: false,
            auto_check_updates: true,
            window_bounds: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemStatus {
    pub is_elevated: bool,
    pub worker_connected: bool,
    pub portable_mode: bool,
    pub data_dir: String,
    pub windows_version: String,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    #[serde(default)]
    pub latest_tag: String,
    pub has_update: bool,
    pub release_url: String,
    pub published_at: String,
    pub notes_preview: String,
    #[serde(default)]
    pub exe_asset_name: Option<String>,
    #[serde(default)]
    pub exe_asset_url: Option<String>,
    #[serde(default)]
    pub exe_asset_size: Option<u64>,
    #[serde(default)]
    pub checksums_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateStage {
    #[default]
    Idle,
    Downloading,
    Verifying,
    Ready,
    Installing,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedUpdateInfo {
    pub version: String,
    pub tag: String,
    pub asset_name: String,
    pub sha256: String,
    pub file_size: u64,
    pub staged_exe_path: String,
    pub staging_dir: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgressEvent {
    pub stage: UpdateStage,
    pub version: String,
    pub tag: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f32,
    pub error: Option<String>,
    pub staged: Option<StagedUpdateInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStateSnapshot {
    pub latest_check: Option<UpdateCheckResult>,
    pub progress: UpdateProgressEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingUpdateOutcome {
    pub success: bool,
    pub from_version: String,
    pub to_version: String,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationStatus {
    pub force_remove_enabled: bool,
    pub track_install_enabled: bool,
    pub registered_exe: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Html,
    Csv,
    Json,
}
