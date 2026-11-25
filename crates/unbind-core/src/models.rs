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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledApp {
    pub id: String,
    pub name: String,
    pub publisher: String,
    pub version: String,
    /// Normalized YYYY-MM-DD when parseable, otherwise raw or empty.
    pub install_date: String,
    /// Estimated size in kilobytes (0 if unknown).
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
    pub total_size_bytes: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Html,
    Csv,
    Json,
}
