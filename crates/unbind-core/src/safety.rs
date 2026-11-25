use crate::models::{BackupEntry, CoreError, CoreResult, LeftoverItem, LeftoverKind};
use chrono::{DateTime, Local, Utc};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::{RegKey, RegValue};

/// Normalize a Windows filesystem path to lowercase trimmed backslashes without trailing slash
/// (except drive root like `c:\`).
pub fn normalize_fs_path(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('/', "\\");
    let trimmed = raw.trim();
    let stripped = trimmed
        .strip_prefix("\\\\?\\")
        .unwrap_or(trimmed)
        .to_lowercase();
    if stripped.len() > 3 && stripped.ends_with('\\') {
        stripped.trim_end_matches('\\').to_string()
    } else {
        stripped
    }
}

/// Returns true if deleting this filesystem path would risk breaking Windows or shared system state.
pub fn is_protected_path(path: &Path) -> bool {
    let norm = normalize_fs_path(path);
    if norm.is_empty() {
        return true;
    }

    // Drive roots like "c:", "c:\", "d:\"
    if norm.len() <= 3 && norm.as_bytes().get(1) == Some(&b':') {
        return true;
    }

    // Never allow UNC roots or parent-traversal paths
    if norm.contains("\\..\\") || norm.ends_with("\\..") || norm.starts_with("\\\\") {
        return true;
    }

    let system_drive = std::env::var("SystemDrive")
        .unwrap_or_else(|_| "C:".to_string())
        .to_lowercase();
    let windir = std::env::var("SystemRoot")
        .unwrap_or_else(|_| format!("{}\\windows", system_drive))
        .to_lowercase();

    // Anything inside Windows / System32 / WinSxS / Boot / Recovery is protected.
    let protected_prefixes = [
        windir.clone(),
        format!("{}\\boot", system_drive),
        format!("{}\\recovery", system_drive),
        format!("{}\\system volume information", system_drive),
        format!("{}\\$recycle.bin", system_drive),
        format!("{}\\perflogs", system_drive),
        format!("{}\\windows.old", system_drive),
    ];
    for prefix in &protected_prefixes {
        if norm == *prefix || norm.starts_with(&format!("{}\\", prefix)) {
            return true;
        }
    }

    // Exact container directories that must NEVER be deleted themselves.
    let mut exact_protected: HashSet<String> = HashSet::new();
    for var in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "CommonProgramFiles",
        "CommonProgramFiles(x86)",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "PUBLIC",
        "TEMP",
        "TMP",
    ] {
        if let Ok(v) = std::env::var(var) {
            let n = normalize_fs_path(Path::new(&v));
            exact_protected.insert(n.clone());
            // Also protect shallow Microsoft / Windows vendor folders directly inside containers.
            exact_protected.insert(format!("{}\\microsoft", n));
            exact_protected.insert(format!("{}\\windows", n));
            exact_protected.insert(format!("{}\\common files", n));
            exact_protected.insert(format!("{}\\windowsapps", n));
            exact_protected.insert(format!("{}\\packages", n));
        }
    }

    if let Ok(profile) = std::env::var("USERPROFILE") {
        let p = normalize_fs_path(Path::new(&profile));
        for sub in [
            "appdata",
            "appdata\\local",
            "appdata\\locallow",
            "appdata\\roaming",
            "desktop",
            "documents",
            "downloads",
            "favorites",
            "links",
            "music",
            "pictures",
            "videos",
            "saved games",
            "appdata\\roaming\\microsoft\\windows\\start menu",
            "appdata\\roaming\\microsoft\\windows\\start menu\\programs",
            "appdata\\roaming\\microsoft\\windows\\start menu\\programs\\startup",
        ] {
            exact_protected.insert(format!("{}\\{}", p, sub));
        }
    }

    if let Ok(pd) = std::env::var("ProgramData") {
        let p = normalize_fs_path(Path::new(&pd));
        for sub in [
            "microsoft\\windows\\start menu",
            "microsoft\\windows\\start menu\\programs",
            "microsoft\\windows\\start menu\\programs\\startup",
            "package cache",
        ] {
            exact_protected.insert(format!("{}\\{}", p, sub));
        }
    }

    exact_protected.insert(format!("{}\\users", system_drive));
    exact_protected.insert(format!("{}\\program files", system_drive));
    exact_protected.insert(format!("{}\\program files (x86)", system_drive));
    exact_protected.insert(format!("{}\\programdata", system_drive));

    if exact_protected.contains(&norm) {
        return true;
    }

    // Protect critical shared OS subdirectories inside Program Files / ProgramData.
    let shared_protected_prefixes = [
        format!("{}\\program files\\windows defender", system_drive),
        format!("{}\\program files\\windows nt", system_drive),
        format!("{}\\program files\\windowsapps", system_drive),
        format!("{}\\program files\\common files\\microsoft shared", system_drive),
        format!("{}\\program files\\common files\\system", system_drive),
        format!("{}\\program files (x86)\\common files\\microsoft shared", system_drive),
        format!("{}\\program files (x86)\\common files\\system", system_drive),
        format!("{}\\programdata\\microsoft\\windows defender", system_drive),
    ];
    for prefix in &shared_protected_prefixes {
        if norm == *prefix || norm.starts_with(&format!("{}\\", prefix)) {
            return true;
        }
    }

    false
}

/// Parse a full registry path like `HKLM\SOFTWARE\Vendor\App` into `(hive_abbrev, subkey)`.
pub fn split_registry_path(full_path: &str) -> Option<(String, String)> {
    let trimmed = full_path.trim().trim_matches('\\');
    let (hive_raw, subkey) = match trimmed.split_once('\\') {
        Some((h, s)) => (h, s),
        None => (trimmed, ""),
    };
    let hive = match hive_raw.to_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE" => "HKLM",
        "HKCU" | "HKEY_CURRENT_USER" => "HKCU",
        "HKCR" | "HKEY_CLASSES_ROOT" => "HKCR",
        "HKU" | "HKEY_USERS" => "HKU",
        _ => return None,
    };
    Some((hive.to_string(), subkey.trim_matches('\\').to_string()))
}

pub fn hive_to_long_name(hive: &str) -> &'static str {
    match hive.to_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE" => "HKEY_LOCAL_MACHINE",
        "HKCU" | "HKEY_CURRENT_USER" => "HKEY_CURRENT_USER",
        "HKCR" | "HKEY_CLASSES_ROOT" => "HKEY_CLASSES_ROOT",
        "HKU" | "HKEY_USERS" => "HKEY_USERS",
        _ => "HKEY_CURRENT_USER",
    }
}

pub fn open_predef_hive(hive: &str) -> CoreResult<RegKey> {
    match hive.to_uppercase().as_str() {
        "HKLM" | "HKEY_LOCAL_MACHINE" => Ok(RegKey::predef(HKEY_LOCAL_MACHINE)),
        "HKCU" | "HKEY_CURRENT_USER" => Ok(RegKey::predef(HKEY_CURRENT_USER)),
        "HKCR" | "HKEY_CLASSES_ROOT" => Ok(RegKey::predef(HKEY_CLASSES_ROOT)),
        "HKU" | "HKEY_USERS" => Ok(RegKey::predef(HKEY_USERS)),
        other => Err(CoreError::InvalidInput(format!(
            "Unsupported registry hive: {}",
            other
        ))),
    }
}

/// Returns true if deleting the registry key itself would damage Windows or shared registries.
pub fn is_protected_registry_key(hive: &str, subkey: &str) -> bool {
    let sub = subkey.trim().trim_matches('\\').to_lowercase();
    if sub.is_empty() {
        return true;
    }

    let exact_protected = [
        "software",
        "software\\classes",
        "software\\microsoft",
        "software\\microsoft\\windows",
        "software\\microsoft\\windows\\currentversion",
        "software\\microsoft\\windows\\currentversion\\uninstall",
        "software\\microsoft\\windows\\currentversion\\run",
        "software\\microsoft\\windows\\currentversion\\runonce",
        "software\\microsoft\\windows\\currentversion\\explorer",
        "software\\microsoft\\windows\\currentversion\\app paths",
        "software\\microsoft\\windows nt",
        "software\\microsoft\\windows nt\\currentversion",
        "software\\wow6432node",
        "software\\wow6432node\\microsoft",
        "software\\wow6432node\\microsoft\\windows",
        "software\\wow6432node\\microsoft\\windows\\currentversion",
        "software\\wow6432node\\microsoft\\windows\\currentversion\\uninstall",
        "software\\wow6432node\\microsoft\\windows\\currentversion\\run",
        "software\\policies",
        "system",
        "system\\currentcontrolset",
        "system\\currentcontrolset\\control",
        "system\\currentcontrolset\\services",
        "hardware",
        "sam",
        "security",
        "bcd00000000",
        "environment",
        "control panel",
    ];
    if exact_protected.contains(&sub.as_str()) {
        return true;
    }

    // Never allow deleting top-level SAM/SECURITY/HARDWARE/BCD trees.
    if hive.eq_ignore_ascii_case("HKLM")
        && (sub.starts_with("sam\\")
            || sub.starts_with("security\\")
            || sub.starts_with("hardware\\")
            || sub.starts_with("bcd00000000\\")
            || sub.starts_with("system\\currentcontrolset\\control\\"))
    {
        return true;
    }

    // Protect direct HKCR root file-system classes like `*`, `Directory`, `Folder`, `Drive`, `exefile`.
    if hive.eq_ignore_ascii_case("HKCR") {
        let first = sub.split('\\').next().unwrap_or("");
        if matches!(
            first,
            "*" | "directory" | "folder" | "drive" | "exefile" | "lnkfile" | "batfile" | "cmdfile" | "clsid" | "interface" | "typelib" | "applications"
        ) && !sub.contains('\\')
        {
            return true;
        }
    }

    false
}

/// Generic tokens that must never be used alone as heuristic matching anchors.
pub fn is_generic_token(token: &str) -> bool {
    let t = token.trim().to_lowercase();
    if t.len() < 3 {
        return true;
    }
    matches!(
        t.as_str(),
        "app"
            | "apps"
            | "application"
            | "applications"
            | "bin"
            | "client"
            | "cloud"
            | "co"
            | "com"
            | "common"
            | "company"
            | "core"
            | "corp"
            | "corporation"
            | "data"
            | "desktop"
            | "dev"
            | "driver"
            | "drivers"
            | "edition"
            | "engine"
            | "file"
            | "files"
            | "free"
            | "game"
            | "games"
            | "gmbh"
            | "google"
            | "group"
            | "helper"
            | "home"
            | "inc"
            | "install"
            | "installer"
            | "intel"
            | "international"
            | "java"
            | "kit"
            | "labs"
            | "launcher"
            | "lib"
            | "lite"
            | "llc"
            | "local"
            | "ltd"
            | "manager"
            | "media"
            | "microsoft"
            | "net"
            | "network"
            | "nvidia"
            | "amd"
            | "office"
            | "online"
            | "open"
            | "org"
            | "pack"
            | "package"
            | "player"
            | "plugin"
            | "plugins"
            | "plus"
            | "portable"
            | "pro"
            | "product"
            | "program"
            | "programs"
            | "project"
            | "public"
            | "reader"
            | "redist"
            | "redistributable"
            | "release"
            | "runtime"
            | "sdk"
            | "security"
            | "server"
            | "service"
            | "services"
            | "setup"
            | "shared"
            | "software"
            | "standard"
            | "studio"
            | "support"
            | "sync"
            | "sys"
            | "system"
            | "tech"
            | "technologies"
            | "technology"
            | "temp"
            | "test"
            | "the"
            | "tool"
            | "tools"
            | "ultimate"
            | "uninstall"
            | "uninstaller"
            | "update"
            | "updater"
            | "user"
            | "users"
            | "util"
            | "utilities"
            | "utility"
            | "version"
            | "viewer"
            | "web"
            | "win"
            | "win32"
            | "win64"
            | "windows"
            | "x64"
            | "x86"
            | "adobe"
            | "apple"
            | "oracle"
            | "mozilla"
            | "jetbrains"
            | "valve"
            | "steam"
            | "tencent"
            | "alibaba"
            | "baidu"
            | "huawei"
    )
}

/// Sanitize an application name for backup filenames.
pub fn sanitize_filename(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "app".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

/// Export all targeted registry keys and values to a standard Windows Registry Editor v5.00
/// UTF-16LE (with BOM) `.reg` file before deletion so the user can restore with a double-click.
pub fn export_registry_backup(
    items: &[LeftoverItem],
    app_name: &str,
    backup_dir: &Path,
) -> CoreResult<Option<PathBuf>> {
    let reg_items: Vec<&LeftoverItem> = items
        .iter()
        .filter(|i| {
            matches!(
                i.kind,
                LeftoverKind::RegistryKey | LeftoverKind::RegistryValue | LeftoverKind::Service
            )
        })
        .collect();

    if reg_items.is_empty() {
        return Ok(None);
    }

    fs::create_dir_all(backup_dir)?;

    let mut reg_text = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
    let mut exported_any = false;
    let mut visited_keys: HashSet<String> = HashSet::new();

    for item in reg_items {
        let Some((hive, subkey)) = split_registry_path(&item.path) else {
            continue;
        };
        let Ok(root) = open_predef_hive(&hive) else {
            continue;
        };
        let Ok(key) = root.open_subkey_with_flags(&subkey, KEY_READ) else {
            continue;
        };

        match item.kind {
            LeftoverKind::RegistryKey | LeftoverKind::Service => {
                export_key_recursive(
                    &key,
                    hive_to_long_name(&hive),
                    &subkey,
                    &mut reg_text,
                    &mut visited_keys,
                    0,
                );
                exported_any = true;
            }
            LeftoverKind::RegistryValue => {
                if let Some(val_name) = &item.value_name {
                    if let Ok(raw_val) = key.get_raw_value(val_name) {
                        reg_text.push_str(&format!(
                            "[\\{}\\{}]\r\n",
                            hive_to_long_name(&hive),
                            subkey
                        ).replace("[\\", "["));
                        reg_text.push_str(&format_reg_value_line(val_name, &raw_val));
                        reg_text.push_str("\r\n");
                        exported_any = true;
                    }
                }
            }
            _ => {}
        }
    }

    if !exported_any {
        return Ok(None);
    }

    let stamp = Local::now().format("%Y%m%d_%H%M%S");
    let safe_name = sanitize_filename(app_name);
    let filename = format!("{}_{}.reg", stamp, safe_name);
    let out_path = backup_dir.join(filename);

    // Encode as UTF-16LE with BOM (standard .reg v5 format).
    let mut bytes = Vec::with_capacity(2 + reg_text.len() * 2);
    bytes.push(0xFF);
    bytes.push(0xFE);
    for unit in reg_text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(&out_path, bytes)?;
    Ok(Some(out_path))
}

fn export_key_recursive(
    key: &RegKey,
    hive_long: &str,
    subkey: &str,
    out: &mut String,
    visited: &mut HashSet<String>,
    depth: usize,
) {
    if depth > 24 {
        return;
    }
    let full = format!("{}\\{}", hive_long, subkey);
    let canonical = full.to_lowercase();
    if !visited.insert(canonical) {
        return;
    }

    out.push_str(&format!("[{}]\r\n", full));
    for (name, val) in key.enum_values().flatten() {
        out.push_str(&format_reg_value_line(&name, &val));
    }
    out.push_str("\r\n");

    for sub_name in key.enum_keys().flatten() {
        if let Ok(child) = key.open_subkey_with_flags(&sub_name, KEY_READ) {
            let child_path = format!("{}\\{}", subkey, sub_name);
            export_key_recursive(&child, hive_long, &child_path, out, visited, depth + 1);
        }
    }
}

fn escape_reg_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn format_hex_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_reg_value_line(name: &str, val: &RegValue) -> String {
    let key_part = if name.is_empty() {
        "@".to_string()
    } else {
        format!("\"{}\"", escape_reg_string(name))
    };

    match val.vtype {
        REG_SZ => {
            let decoded = decode_utf16_reg(&val.bytes);
            format!("{}=\"{}\"\r\n", key_part, escape_reg_string(&decoded))
        }
        REG_DWORD if val.bytes.len() >= 4 => {
            let num = u32::from_le_bytes([val.bytes[0], val.bytes[1], val.bytes[2], val.bytes[3]]);
            format!("{}=dword:{:08x}\r\n", key_part, num)
        }
        REG_EXPAND_SZ => format!("{}=hex(2):{}\r\n", key_part, format_hex_bytes(&val.bytes)),
        REG_BINARY => format!("{}=hex:{}\r\n", key_part, format_hex_bytes(&val.bytes)),
        REG_MULTI_SZ => format!("{}=hex(7):{}\r\n", key_part, format_hex_bytes(&val.bytes)),
        REG_QWORD => format!("{}=hex(b):{}\r\n", key_part, format_hex_bytes(&val.bytes)),
        _ => format!("{}=hex:{}\r\n", key_part, format_hex_bytes(&val.bytes)),
    }
}

fn decode_utf16_reg(bytes: &[u8]) -> String {
    let u16s: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let trimmed = match u16s.iter().position(|&c| c == 0) {
        Some(pos) => &u16s[..pos],
        None => &u16s[..],
    };
    String::from_utf16_lossy(trimmed)
}

/// Delete a registry key (recursively) or value after enforcing safety guardrails.
pub fn delete_registry_item(full_path: &str, value_name: Option<&str>) -> CoreResult<()> {
    let (hive, subkey) = split_registry_path(full_path)
        .ok_or_else(|| CoreError::InvalidInput(format!("Invalid registry path: {}", full_path)))?;

    if let Some(val_name) = value_name {
        // Deleting a single named value inside a container like Run is permitted,
        // provided the hive itself is valid and subkey is non-empty.
        if subkey.is_empty() {
            return Err(CoreError::SafetyBlocked(
                "Refusing to delete value at registry hive root".to_string(),
            ));
        }
        let root = open_predef_hive(&hive)?;
        let key = root
            .open_subkey_with_flags(&subkey, KEY_SET_VALUE | KEY_WOW64_64KEY)
            .or_else(|_| root.open_subkey_with_flags(&subkey, KEY_SET_VALUE | KEY_WOW64_32KEY))
            .map_err(|e| map_reg_err(e, full_path))?;
        key.delete_value(val_name)
            .map_err(|e| map_reg_err(e, full_path))?;
        return Ok(());
    }

    if is_protected_registry_key(&hive, &subkey) {
        return Err(CoreError::SafetyBlocked(format!(
            "Protected system registry key cannot be deleted: {}",
            full_path
        )));
    }

    let root = open_predef_hive(&hive)?;
    // Try standard 64-bit view first, then 32-bit view.
    match root.delete_subkey_all(&subkey) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Try opening with WOW64_32KEY if it was a 32-bit redirected key
            if let Some((parent, leaf)) = subkey.rsplit_once('\\') {
                if let Ok(p) = root.open_subkey_with_flags(parent, KEY_WRITE | KEY_WOW64_32KEY) {
                    p.delete_subkey_all(leaf)
                        .map_err(|err| map_reg_err(err, full_path))?;
                    return Ok(());
                }
            }
            Ok(())
        }
        Err(e) => Err(map_reg_err(e, full_path)),
    }
}

fn map_reg_err(err: std::io::Error, path: &str) -> CoreError {
    if err.kind() == std::io::ErrorKind::PermissionDenied || err.raw_os_error() == Some(5) {
        CoreError::ElevationRequired(format!("Access denied modifying {}", path))
    } else {
        CoreError::Io(err)
    }
}

/// Delete a file or directory safely. Returns `Ok(true)` if scheduled for deletion on reboot,
/// `Ok(false)` if deleted immediately.
pub fn delete_fs_item(
    path: &Path,
    use_recycle_bin: bool,
    schedule_reboot_on_lock: bool,
) -> CoreResult<bool> {
    if is_protected_path(path) {
        return Err(CoreError::SafetyBlocked(format!(
            "Protected system path cannot be deleted: {}",
            path.display()
        )));
    }

    if !path.exists() {
        return Ok(false);
    }

    if use_recycle_bin && trash::delete(path).is_ok() {
        return Ok(false);
    }

    let remove_res = if path.is_dir() {
        clear_readonly_recursive(path);
        fs::remove_dir_all(path)
    } else {
        clear_readonly_single(path);
        fs::remove_file(path)
    };

    match remove_res {
        Ok(()) => Ok(false),
        Err(err) => {
            let os_code = err.raw_os_error();
            // ERROR_SHARING_VIOLATION (32), ERROR_LOCK_VIOLATION (33), ERROR_ACCESS_DENIED (5)
            if schedule_reboot_on_lock && matches!(os_code, Some(32) | Some(33) | Some(145)) {
                schedule_delete_on_reboot(path)?;
                return Ok(true);
            }
            if os_code == Some(5) || err.kind() == std::io::ErrorKind::PermissionDenied {
                if schedule_reboot_on_lock && is_process_elevated() {
                    schedule_delete_on_reboot(path)?;
                    return Ok(true);
                }
                return Err(CoreError::ElevationRequired(format!(
                    "Access denied deleting {}",
                    path.display()
                )));
            }
            Err(CoreError::Io(err))
        }
    }
}

#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_single(path: &Path) {
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

fn clear_readonly_recursive(dir: &Path) {
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        clear_readonly_single(entry.path());
    }
}

#[cfg(windows)]
pub fn schedule_delete_on_reboot(path: &Path) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    if is_protected_path(path) {
        return Err(CoreError::SafetyBlocked(format!(
            "Protected path blocked from reboot delete: {}",
            path.display()
        )));
    }

    // For directories, schedule leaf files bottom-up first, then the directory itself.
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = walkdir::WalkDir::new(path)
            .contents_first(true)
            .into_iter()
            .flatten()
            .map(|e| e.into_path())
            .collect();
        if entries.is_empty() {
            entries.push(path.to_path_buf());
        }
        for p in entries {
            let wide: Vec<u16> = p.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe {
                MoveFileExW(PCWSTR(wide.as_ptr()), PCWSTR::null(), MOVEFILE_DELAY_UNTIL_REBOOT)
                    .map_err(|e| CoreError::WinApi(format!("MoveFileExW failed: {}", e)))?;
            }
        }
        return Ok(());
    }

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        MoveFileExW(PCWSTR(wide.as_ptr()), PCWSTR::null(), MOVEFILE_DELAY_UNTIL_REBOOT)
            .map_err(|e| CoreError::WinApi(format!("MoveFileExW failed: {}", e)))?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn schedule_delete_on_reboot(_path: &Path) -> CoreResult<()> {
    Ok(())
}

/// Check whether the current process is running with elevated Administrator privileges.
#[cfg(windows)]
pub fn is_process_elevated() -> bool {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
pub fn is_process_elevated() -> bool {
    false
}

/// Create a Windows System Restore Point via `srclient.dll!SRSetRestorePointW`.
/// Requires Administrator privileges and System Protection enabled on the OS drive.
#[cfg(windows)]
pub fn create_system_restore_point(description: &str) -> CoreResult<bool> {
    use std::ffi::c_void;
    use windows::core::{s, w};
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    if !is_process_elevated() {
        return Err(CoreError::ElevationRequired(
            "Administrator privileges required to create a System Restore Point".to_string(),
        ));
    }

    #[repr(C, packed)]
    struct RestorePointInfoW {
        dw_event_type: u32,
        dw_restore_pt_type: u32,
        ll_sequence_number: i64,
        sz_description: [u16; 256],
    }

    #[repr(C, packed)]
    struct StateMgrStatus {
        n_status: u32,
        ll_sequence_number: i64,
    }

    type SrSetRestorePointWFn =
        unsafe extern "system" fn(*const RestorePointInfoW, *mut StateMgrStatus) -> i32;

    const BEGIN_SYSTEM_CHANGE: u32 = 100;
    const APPLICATION_UNINSTALL: u32 = 1;

    unsafe {
        let module = LoadLibraryW(w!("srclient.dll"))
            .map_err(|e| CoreError::WinApi(format!("Failed to load srclient.dll: {}", e)))?;
        let proc = GetProcAddress(module, s!("SRSetRestorePointW"));
        let Some(proc_fn) = proc else {
            let _ = FreeLibrary(module);
            return Ok(false);
        };

        let sr_set: SrSetRestorePointWFn = std::mem::transmute(proc_fn as *const c_void);

        let mut desc_buf = [0u16; 256];
        for (idx, unit) in description.encode_utf16().take(255).enumerate() {
            desc_buf[idx] = unit;
        }

        let info = RestorePointInfoW {
            dw_event_type: BEGIN_SYSTEM_CHANGE,
            dw_restore_pt_type: APPLICATION_UNINSTALL,
            ll_sequence_number: 0,
            sz_description: desc_buf,
        };
        let mut status = StateMgrStatus {
            n_status: 0,
            ll_sequence_number: 0,
        };

        let ok = sr_set(&info, &mut status) != 0;
        let _ = FreeLibrary(module);
        Ok(ok)
    }
}

#[cfg(not(windows))]
pub fn create_system_restore_point(_description: &str) -> CoreResult<bool> {
    Ok(false)
}

/// List saved `.reg` backup files sorted newest first.
pub fn list_backups(backup_dir: &Path) -> Vec<BackupEntry> {
    let Ok(entries) = fs::read_dir(backup_dir) else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("reg") {
            continue;
        }
        let filename = entry.file_name().to_string_lossy().to_string();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let created_at = meta
            .modified()
            .ok()
            .map(|t| DateTime::<Utc>::from(t).to_rfc3339())
            .unwrap_or_default();
        // Parse app name from `<YYYYMMDD_HHMMSS>_<AppName>.reg`
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(&filename);
        let app_name = stem
            .splitn(3, '_')
            .nth(2)
            .unwrap_or(stem)
            .replace('_', " ");

        results.push(BackupEntry {
            id: filename.clone(),
            filename,
            path: path.to_string_lossy().to_string(),
            created_at,
            size_bytes: meta.len(),
            app_name,
        });
    }
    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    results
}

fn ensure_backup_path_contained(path: &Path, backup_dir: &Path) -> CoreResult<PathBuf> {
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("reg")) != Some(true)
    {
        return Err(CoreError::InvalidInput(
            "Only .reg backup files are permitted".to_string(),
        ));
    }
    let canon_dir = fs::canonicalize(backup_dir)
        .map_err(|_| CoreError::NotFound("Backups directory does not exist".to_string()))?;
    let canon_file = fs::canonicalize(path)
        .map_err(|_| CoreError::NotFound(format!("Backup file not found: {}", path.display())))?;
    let dir_norm = normalize_fs_path(&canon_dir);
    let file_norm = normalize_fs_path(&canon_file);
    if !file_norm.starts_with(&format!("{}\\", dir_norm)) {
        return Err(CoreError::SafetyBlocked(
            "Backup file path escapes backups directory".to_string(),
        ));
    }
    Ok(canon_file)
}

fn decode_reg_file_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        let u16s: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        String::from_utf8_lossy(&bytes[3..]).to_string()
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

/// Read and decode a `.reg` backup file safely for in-app preview.
pub fn preview_registry_backup(
    path: &Path,
    backup_dir: &Path,
    max_chars: usize,
) -> CoreResult<String> {
    let valid_path = ensure_backup_path_contained(path, backup_dir)?;
    let bytes = fs::read(&valid_path)?;
    let text = decode_reg_file_text(&bytes);
    if text.chars().count() > max_chars {
        let truncated: String = text.chars().take(max_chars).collect();
        Ok(format!(
            "{}\r\n\r\n; --- Preview truncated (open file to inspect full content) ---",
            truncated
        ))
    } else {
        Ok(text)
    }
}

/// Check whether importing a `.reg` backup requires Administrator privileges (touches HKLM/HKCR/HKU).
pub fn reg_backup_requires_elevation(path: &Path, backup_dir: &Path) -> CoreResult<bool> {
    let valid_path = ensure_backup_path_contained(path, backup_dir)?;
    let bytes = fs::read(&valid_path)?;
    let upper = decode_reg_file_text(&bytes).to_uppercase();
    Ok(upper.contains("[HKEY_LOCAL_MACHINE\\")
        || upper.contains("[-HKEY_LOCAL_MACHINE\\")
        || upper.contains("[HKEY_CLASSES_ROOT\\")
        || upper.contains("[-HKEY_CLASSES_ROOT\\")
        || upper.contains("[HKEY_USERS\\"))
}

/// Import a `.reg` backup file back into Windows Registry using `reg.exe import`.
#[cfg(windows)]
pub fn restore_registry_backup(path: &Path) -> CoreResult<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    if !path.exists() {
        return Err(CoreError::NotFound(format!(
            "Backup file not found: {}",
            path.display()
        )));
    }
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("reg")) != Some(true)
    {
        return Err(CoreError::InvalidInput(
            "Only .reg files can be restored".to_string(),
        ));
    }

    let out = std::process::Command::new("reg.exe")
        .arg("import")
        .arg(path)
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;

    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let combined = if stderr.is_empty() { stdout } else { stderr };
        if combined.to_lowercase().contains("access is denied")
            || combined.to_lowercase().contains("error accessing")
        {
            Err(CoreError::ElevationRequired(format!(
                "Administrator privileges required to restore {}",
                path.display()
            )))
        } else {
            Err(CoreError::WinApi(format!(
                "reg import failed: {}",
                combined
            )))
        }
    }
}

#[cfg(not(windows))]
pub fn restore_registry_backup(_path: &Path) -> CoreResult<()> {
    Ok(())
}

/// Safely delete a `.reg` backup file located inside `backup_dir`.
pub fn delete_backup_file(path: &Path, backup_dir: &Path) -> CoreResult<()> {
    let valid_path = ensure_backup_path_contained(path, backup_dir)?;
    fs::remove_file(valid_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_protected_paths_blocked() {
        assert!(is_protected_path(Path::new("C:\\")));
        assert!(is_protected_path(Path::new("C:\\Windows")));
        assert!(is_protected_path(Path::new("C:\\Windows\\System32\\cmd.exe")));
        assert!(is_protected_path(Path::new("C:\\Program Files")));
        assert!(is_protected_path(Path::new("C:\\Program Files (x86)")));
        assert!(is_protected_path(Path::new("C:\\ProgramData")));
        assert!(is_protected_path(Path::new("C:\\Users")));
        assert!(is_protected_path(Path::new("C:\\Program Files\\Microsoft")));
        assert!(!is_protected_path(Path::new("C:\\Program Files\\MyCustomApp")));
    }

    #[test]
    fn test_protected_registry_keys_blocked() {
        assert!(is_protected_registry_key("HKLM", ""));
        assert!(is_protected_registry_key("HKLM", "SOFTWARE"));
        assert!(is_protected_registry_key("HKLM", "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"));
        assert!(is_protected_registry_key("HKCU", "Software\\Microsoft\\Windows\\CurrentVersion\\Run"));
        assert!(is_protected_registry_key("HKLM", "SYSTEM\\CurrentControlSet\\Services"));
        assert!(!is_protected_registry_key("HKCU", "Software\\MyCustomApp"));
        assert!(!is_protected_registry_key(
            "HKLM",
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MyCustomApp"
        ));
    }

    #[test]
    fn test_generic_tokens_filtered() {
        assert!(is_generic_token("app"));
        assert!(is_generic_token("Microsoft"));
        assert!(is_generic_token("Windows"));
        assert!(is_generic_token("ab"));
        assert!(!is_generic_token("NotepadPlusPlus"));
        assert!(!is_generic_token("7-Zip"));
    }

    #[test]
    fn test_registry_backup_and_safe_delete_sandbox() {
        let dir = tempdir().unwrap();
        let test_subkey = format!("Software\\Unbind_UnitTest_{}", uuid::Uuid::new_v4());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (k, _) = hkcu.create_subkey(&test_subkey).unwrap();
        k.set_value("DisplayName", &"Sandbox Test App").unwrap();
        k.set_value("VersionCode", &42u32).unwrap();
        drop(k);

        let full_path = format!("HKCU\\{}", test_subkey);
        let item = LeftoverItem {
            id: "1".into(),
            kind: LeftoverKind::RegistryKey,
            path: full_path.clone(),
            value_name: None,
            size_bytes: 0,
            confidence: crate::models::ConfidenceLevel::High,
            reason: "Unit test".into(),
            reason_code: "unit_test".into(),
            reason_param: None,
            selected_by_default: true,
            requires_elevation: false,
        };

        let backup = export_registry_backup(&[item], "Sandbox Test", dir.path())
            .unwrap()
            .expect("backup file should be created");
        assert!(backup.exists());
        let raw = fs::read(&backup).unwrap();
        // Verify UTF-16LE BOM
        assert_eq!(&raw[0..2], &[0xFF, 0xFE]);

        // Verify preview and elevation detection
        let preview = preview_registry_backup(&backup, dir.path(), 4000).unwrap();
        assert!(preview.contains("Sandbox Test App"));
        assert!(!reg_backup_requires_elevation(&backup, dir.path()).unwrap());

        // Verify safe deletion removes our sandbox key
        delete_registry_item(&full_path, None).unwrap();
        assert!(hkcu.open_subkey(&test_subkey).is_err());

        // Verify in-app .reg restore brings the sandbox key right back!
        restore_registry_backup(&backup).unwrap();
        let restored = hkcu.open_subkey(&test_subkey).unwrap();
        let restored_name: String = restored.get_value("DisplayName").unwrap();
        assert_eq!(restored_name, "Sandbox Test App");

        // Clean up sandbox key and backup file
        delete_registry_item(&full_path, None).unwrap();
        delete_backup_file(&backup, dir.path()).unwrap();
        assert!(!backup.exists());
    }

    #[test]
    fn test_fs_safe_delete_sandbox() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("app_leftover");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("config.ini"), "hello").unwrap();

        let reboot = delete_fs_item(&target, false, false).unwrap();
        assert!(!reboot);
        assert!(!target.exists());
    }
}
