use crate::models::{AppSettings, CoreResult, OperationLogEntry};
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const PORTABLE_MARKER: &str = "portable.json";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Debug, Clone)]
pub struct AppStorage {
    pub portable_mode: bool,
    pub data_dir: PathBuf,
}

impl AppStorage {
    /// Detect portable mode: if `portable.json` sits next to the running executable,
    /// store all settings, backups, traces, and logs inside `<exe_dir>/unbind-data`.
    /// Otherwise use `%LOCALAPPDATA%\Unbind`.
    pub fn detect() -> Self {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let marker = exe_dir.join(PORTABLE_MARKER);
                if marker.exists() {
                    let data_dir = exe_dir.join("unbind-data");
                    let _ = Self::ensure_dirs(&data_dir);
                    return Self {
                        portable_mode: true,
                        data_dir,
                    };
                }
            }
        }

        let local_app_data = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let data_dir = local_app_data.join("Unbind");
        let _ = Self::ensure_dirs(&data_dir);
        Self {
            portable_mode: false,
            data_dir,
        }
    }

    pub fn for_dir(data_dir: PathBuf, portable_mode: bool) -> Self {
        let _ = Self::ensure_dirs(&data_dir);
        Self {
            portable_mode,
            data_dir,
        }
    }

    fn ensure_dirs(base: &Path) -> CoreResult<()> {
        fs::create_dir_all(base.join("backups"))?;
        fs::create_dir_all(base.join("traces"))?;
        fs::create_dir_all(base.join("logs"))?;
        Ok(())
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    pub fn traces_dir(&self) -> PathBuf {
        self.data_dir.join("traces")
    }

    pub fn logs_file(&self) -> PathBuf {
        self.data_dir.join("logs").join("operations.jsonl")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join(SETTINGS_FILE)
    }

    pub fn load_settings(&self) -> AppSettings {
        let mut settings = fs::read_to_string(self.settings_file())
            .ok()
            .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
            .unwrap_or_default();
        settings.portable_mode = self.portable_mode;
        settings.data_dir = self.data_dir.to_string_lossy().to_string();
        settings
    }

    pub fn save_settings(&self, settings: &AppSettings) -> CoreResult<AppSettings> {
        // Toggle portable.json marker next to executable when requested.
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let marker = exe_dir.join(PORTABLE_MARKER);
                if settings.portable_mode && !marker.exists() {
                    let _ = fs::write(&marker, "{\"portable\":true}\n");
                } else if !settings.portable_mode && marker.exists() {
                    let _ = fs::remove_file(&marker);
                }
            }
        }

        let target_storage = if settings.portable_mode {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(exe_dir) = exe_path.parent() {
                    Self::for_dir(exe_dir.join("unbind-data"), true)
                } else {
                    self.clone()
                }
            } else {
                self.clone()
            }
        } else {
            let local_app_data = std::env::var("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::temp_dir());
            Self::for_dir(local_app_data.join("Unbind"), false)
        };

        let mut normalized = settings.clone();
        normalized.portable_mode = target_storage.portable_mode;
        normalized.data_dir = target_storage.data_dir.to_string_lossy().to_string();

        let json = serde_json::to_string_pretty(&normalized)?;
        fs::write(target_storage.settings_file(), json)?;
        Ok(normalized)
    }

    pub fn append_log(
        &self,
        action: &str,
        target: &str,
        summary: &str,
        success: bool,
    ) -> CoreResult<()> {
        Self::ensure_dirs(&self.data_dir)?;
        let entry = OperationLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            action: action.to_string(),
            target: target.to_string(),
            summary: summary.to_string(),
            success,
        };
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.logs_file())?;
        file.write_all(line.as_bytes())?;
        Ok(())
    }

    pub fn read_logs(&self, limit: usize) -> Vec<OperationLogEntry> {
        let Ok(content) = fs::read_to_string(self.logs_file()) else {
            return Vec::new();
        };
        let mut items: Vec<OperationLogEntry> = content
            .lines()
            .filter_map(|l| serde_json::from_str::<OperationLogEntry>(l).ok())
            .collect();
        items.reverse();
        items.truncate(limit);
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_storage_settings_and_logs() {
        let dir = tempdir().unwrap();
        let storage = AppStorage::for_dir(dir.path().to_path_buf(), true);
        let default_settings = storage.load_settings();
        assert!(default_settings.portable_mode);
        assert!(default_settings.use_recycle_bin);

        storage
            .append_log("uninstall", "TestApp", "Removed 3 items", true)
            .unwrap();
        let logs = storage.read_logs(10);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].target, "TestApp");
        assert!(logs[0].success);
    }
}
