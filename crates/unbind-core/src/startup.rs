use crate::discovery::win32::extract_executable_from_command;
use crate::models::{CoreError, CoreResult, StartupItem, StartupSource};
use crate::unlocker::resolve_shortcut_target;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

const RUN_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_ONCE_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce";
const RUN_WOW64_PATH: &str = "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run";
const APPROVED_RUN: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
const APPROVED_RUN32: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run32";
const APPROVED_FOLDER: &str =
    "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\StartupFolder";

/// Discover startup entries across Registry Run/RunOnce, Startup Folders, and user Scheduled Tasks.
pub fn list_startup_items() -> Vec<StartupItem> {
    let mut items = Vec::new();

    // 1. Registry Run keys (synchronized with Windows Task Manager StartupApproved status).
    scan_reg_run(
        HKEY_CURRENT_USER,
        "HKCU",
        RUN_PATH,
        KEY_READ | KEY_WOW64_64KEY,
        APPROVED_RUN,
        StartupSource::RegistryCurrentUserRun,
        false,
        &mut items,
    );
    scan_reg_run(
        HKEY_CURRENT_USER,
        "HKCU",
        RUN_ONCE_PATH,
        KEY_READ | KEY_WOW64_64KEY,
        APPROVED_RUN,
        StartupSource::RegistryCurrentUserRunOnce,
        false,
        &mut items,
    );
    scan_reg_run(
        HKEY_LOCAL_MACHINE,
        "HKLM",
        RUN_PATH,
        KEY_READ | KEY_WOW64_64KEY,
        APPROVED_RUN,
        StartupSource::RegistryLocalMachineRun,
        true,
        &mut items,
    );
    scan_reg_run(
        HKEY_LOCAL_MACHINE,
        "HKLM",
        RUN_WOW64_PATH,
        KEY_READ | KEY_WOW64_64KEY,
        APPROVED_RUN32,
        StartupSource::RegistryLocalMachineWow64Run,
        true,
        &mut items,
    );

    // 2. Startup Folders (User & All Users).
    if let Ok(appdata) = std::env::var("APPDATA") {
        let user_startup =
            PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup");
        scan_startup_folder(
            &user_startup,
            HKEY_CURRENT_USER,
            StartupSource::StartupFolderUser,
            false,
            &mut items,
        );
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        let common_startup =
            PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs\\Startup");
        scan_startup_folder(
            &common_startup,
            HKEY_LOCAL_MACHINE,
            StartupSource::StartupFolderCommon,
            true,
            &mut items,
        );
    }

    // 3. Non-Microsoft Scheduled Tasks (Logon / Boot / Startup).
    items.extend(scan_scheduled_tasks());

    items.sort_by_key(|a| a.name.to_lowercase());
    items
}

#[allow(clippy::too_many_arguments)]
fn scan_reg_run(
    hkey: winreg::HKEY,
    hive_label: &str,
    subkey: &str,
    flags: u32,
    approved_subkey: &str,
    source: StartupSource,
    requires_elevation: bool,
    out: &mut Vec<StartupItem>,
) {
    let root = RegKey::predef(hkey);
    let Ok(key) = root.open_subkey_with_flags(subkey, flags) else {
        return;
    };
    let approved_map = read_startup_approved_map(hkey, approved_subkey);

    for (val_name, val) in key.enum_values().flatten() {
        let command = val.to_string().trim().to_string();
        if command.is_empty() {
            continue;
        }
        let exe_path = extract_executable_from_command(&command);
        let exe_str = exe_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let is_valid = exe_path
            .as_ref()
            .map(|p| {
                p.exists()
                    || p.to_string_lossy().to_lowercase().ends_with("rundll32.exe")
                    || p.to_string_lossy().to_lowercase().ends_with("cmd.exe")
            })
            .unwrap_or(false);

        let enabled = approved_map
            .get(&val_name.to_lowercase())
            .copied()
            .unwrap_or(true);

        let location = format!("{}\\{}", hive_label, subkey);
        let id = format!("startup:{:?}:{}", source, val_name);

        out.push(StartupItem {
            id,
            name: val_name,
            command,
            executable_path: exe_str,
            publisher: String::new(),
            source,
            location,
            enabled,
            is_valid_target: is_valid,
            requires_elevation,
        });
    }
}

fn scan_startup_folder(
    folder: &Path,
    approved_hive: winreg::HKEY,
    source: StartupSource,
    requires_elevation: bool,
    out: &mut Vec<StartupItem>,
) {
    if !folder.is_dir() {
        return;
    }
    let approved_map = read_startup_approved_map(approved_hive, APPROVED_FOLDER);
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if filename.eq_ignore_ascii_case("desktop.ini") {
            continue;
        }

        let target = resolve_shortcut_target(&path).unwrap_or_else(|| path.clone());
        let is_valid = target.exists();
        let enabled = approved_map
            .get(&filename.to_lowercase())
            .copied()
            .unwrap_or(true);
        let display_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename)
            .to_string();

        out.push(StartupItem {
            id: format!("startup:{:?}:{}", source, filename),
            name: display_name,
            command: path.to_string_lossy().to_string(),
            executable_path: target.to_string_lossy().to_string(),
            publisher: String::new(),
            source,
            location: folder.to_string_lossy().to_string(),
            enabled,
            is_valid_target: is_valid,
            requires_elevation,
        });
    }
}

/// Read Windows `StartupApproved` binary map (first byte even = enabled, odd = disabled).
fn read_startup_approved_map(hkey: winreg::HKEY, subkey: &str) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    let root = RegKey::predef(hkey);
    let Ok(key) = root.open_subkey_with_flags(subkey, KEY_READ) else {
        return map;
    };
    for (name, val) in key.enum_values().flatten() {
        if let Some(&first_byte) = val.bytes.first() {
            // 0x02 / 0x06 (even) => Enabled; 0x03 / 0x07 (odd) => Disabled
            let is_enabled = (first_byte & 1) == 0;
            map.insert(name.to_lowercase(), is_enabled);
        }
    }
    map
}

fn write_startup_approved(hkey: winreg::HKEY, subkey: &str, name: &str, enabled: bool) -> CoreResult<()> {
    let root = RegKey::predef(hkey);
    let (key, _) = root.create_subkey(subkey).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5) {
            CoreError::ElevationRequired(format!("Access denied updating {}", subkey))
        } else {
            CoreError::Io(e)
        }
    })?;

    let mut bytes = [0u8; 12];
    if enabled {
        bytes[0] = 0x02;
    } else {
        bytes[0] = 0x03;
        // Write current Windows FILETIME into bytes[4..12].
        let now_unix = chrono::Utc::now().timestamp();
        let filetime = ((now_unix + 11_644_473_600) as u64).saturating_mul(10_000_000);
        bytes[4..12].copy_from_slice(&filetime.to_le_bytes());
    }

    let reg_val = RegValue {
        bytes: std::borrow::Cow::Borrowed(&bytes),
        vtype: REG_BINARY,
    };
    key.set_raw_value(name, &reg_val).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied || e.raw_os_error() == Some(5) {
            CoreError::ElevationRequired(format!("Access denied writing StartupApproved {}", name))
        } else {
            CoreError::Io(e)
        }
    })?;
    Ok(())
}

/// Enable or disable a startup item non-destructively (synced with Windows Task Manager).
pub fn set_startup_enabled(item: &StartupItem, enabled: bool) -> CoreResult<()> {
    match item.source {
        StartupSource::RegistryCurrentUserRun | StartupSource::RegistryCurrentUserRunOnce => {
            write_startup_approved(HKEY_CURRENT_USER, APPROVED_RUN, &item.name, enabled)
        }
        StartupSource::RegistryLocalMachineRun => {
            write_startup_approved(HKEY_LOCAL_MACHINE, APPROVED_RUN, &item.name, enabled)
        }
        StartupSource::RegistryLocalMachineWow64Run => {
            write_startup_approved(HKEY_LOCAL_MACHINE, APPROVED_RUN32, &item.name, enabled)
        }
        StartupSource::StartupFolderUser => {
            let filename = Path::new(&item.command)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&item.name);
            write_startup_approved(HKEY_CURRENT_USER, APPROVED_FOLDER, filename, enabled)
        }
        StartupSource::StartupFolderCommon => {
            let filename = Path::new(&item.command)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&item.name);
            write_startup_approved(HKEY_LOCAL_MACHINE, APPROVED_FOLDER, filename, enabled)
        }
        StartupSource::ScheduledTask => {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let flag = if enabled { "/ENABLE" } else { "/DISABLE" };
            let out = std::process::Command::new("schtasks.exe")
                .args(["/Change", "/TN", &item.location, flag])
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;
            if out.status.success() {
                Ok(())
            } else {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if err.to_lowercase().contains("access is denied") {
                    Err(CoreError::ElevationRequired(format!(
                        "Access denied changing scheduled task {}",
                        item.location
                    )))
                } else {
                    Err(CoreError::WinApi(err))
                }
            }
        }
    }
}

/// Permanently delete a startup item.
pub fn delete_startup_item(item: &StartupItem) -> CoreResult<()> {
    match item.source {
        StartupSource::RegistryCurrentUserRun
        | StartupSource::RegistryCurrentUserRunOnce
        | StartupSource::RegistryLocalMachineRun
        | StartupSource::RegistryLocalMachineWow64Run => {
            crate::safety::delete_registry_item(&item.location, Some(&item.name))
        }
        StartupSource::StartupFolderUser | StartupSource::StartupFolderCommon => {
            let p = Path::new(&item.command);
            crate::safety::delete_fs_item(p, true, false)?;
            Ok(())
        }
        StartupSource::ScheduledTask => {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            let out = std::process::Command::new("schtasks.exe")
                .args(["/Delete", "/TN", &item.location, "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()?;
            if out.status.success() {
                Ok(())
            } else {
                let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                if err.to_lowercase().contains("access is denied") {
                    Err(CoreError::ElevationRequired(format!(
                        "Access denied deleting scheduled task {}",
                        item.location
                    )))
                } else {
                    Err(CoreError::WinApi(err))
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledTaskInfo {
    pub name: String,
    pub task_path: String,
    pub command: String,
    pub executable_path: String,
    pub enabled: bool,
}

/// Enumerate third-party Windows Scheduled Tasks (skipping built-in Microsoft OS folders)
/// up to depth 3 directly from the Windows TaskCache registry tree + System32\Tasks XML.
pub fn enumerate_third_party_scheduled_tasks() -> Vec<ScheduledTaskInfo> {
    let mut results = Vec::new();
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    let tree_base = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\Schedule\\TaskCache\\Tree";
    let Ok(tree_key) = root.open_subkey_with_flags(tree_base, KEY_READ) else {
        return results;
    };
    walk_task_tree(&tree_key, "", 0, &mut results);
    results
}

fn walk_task_tree(key: &RegKey, rel_prefix: &str, depth: usize, out: &mut Vec<ScheduledTaskInfo>) {
    if depth > 2 {
        return;
    }
    for sub_name in key.enum_keys().flatten() {
        if depth == 0
            && (sub_name.eq_ignore_ascii_case("Microsoft")
                || sub_name.eq_ignore_ascii_case("MicrosoftEdgeUpdateTaskMachineCore"))
        {
            continue;
        }
        let Ok(sub) = key.open_subkey_with_flags(&sub_name, KEY_READ) else {
            continue;
        };
        let rel_path = if rel_prefix.is_empty() {
            sub_name.clone()
        } else {
            format!("{}\\{}", rel_prefix, sub_name)
        };
        let task_id: String = sub.get_value("Id").unwrap_or_default();
        if !task_id.is_empty() {
            let task_path = format!("\\{}", rel_path);
            let (command, enabled) = inspect_task_xml(&rel_path);
            let exe = extract_executable_from_command(&command);
            let executable_path = exe
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            out.push(ScheduledTaskInfo {
                name: sub_name,
                task_path,
                command,
                executable_path,
                enabled,
            });
        } else {
            walk_task_tree(&sub, &rel_path, depth + 1, out);
        }
    }
}

/// Read third-party scheduled tasks directly from the Windows Tasks registry tree (much faster and
/// locale-independent compared to parsing `schtasks /V` text!).
fn scan_scheduled_tasks() -> Vec<StartupItem> {
    enumerate_third_party_scheduled_tasks()
        .into_iter()
        .map(|info| {
            let valid = if info.executable_path.is_empty() {
                true
            } else {
                Path::new(&info.executable_path).exists()
            };
            StartupItem {
                id: format!("startup:task:{}", info.task_path),
                name: info.name,
                command: info.command,
                executable_path: info.executable_path,
                publisher: String::new(),
                source: StartupSource::ScheduledTask,
                location: info.task_path,
                enabled: info.enabled,
                is_valid_target: valid,
                requires_elevation: true,
            }
        })
        .collect()
}

fn inspect_task_xml(task_name: &str) -> (String, bool) {
    let windir = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let xml_path = PathBuf::from(windir)
        .join("System32\\Tasks")
        .join(task_name);
    let Ok(bytes) = fs::read(&xml_path) else {
        return (String::new(), true);
    };
    // Task XML files on Windows are usually UTF-16LE.
    let text = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(&bytes).to_string()
    };

    let cmd = extract_xml_tag(&text, "Command").unwrap_or_default();
    let args = extract_xml_tag(&text, "Arguments").unwrap_or_default();
    let full_cmd = if args.is_empty() {
        cmd
    } else {
        format!("{} {}", cmd, args)
    };
    let enabled = extract_xml_tag(&text, "Enabled")
        .map(|v| !v.eq_ignore_ascii_case("false"))
        .unwrap_or(true);
    (full_cmd, enabled)
}

fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xml_tag_extraction() {
        let sample = "<Task><Settings><Enabled>false</Enabled></Settings><Actions><Exec><Command>C:\\Tools\\demo.exe</Command><Arguments>--bg</Arguments></Exec></Actions></Task>";
        assert_eq!(
            extract_xml_tag(sample, "Command").as_deref(),
            Some("C:\\Tools\\demo.exe")
        );
        assert_eq!(extract_xml_tag(sample, "Enabled").as_deref(), Some("false"));
    }

    #[test]
    fn test_list_startup_items_runs_safely() {
        // Smoke test on live Windows host (read-only).
        let _items = list_startup_items();
    }
}
