use crate::models::{AppKind, Arch, InstallScope, InstalledApp, PackageManagerKind};
use crate::safety::is_protected_path;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

const UNINSTALL_SUBKEY: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall";

struct ScanTarget {
    hive_name: &'static str,
    view_name: &'static str,
    scope: InstallScope,
    arch: Arch,
    is_hklm: bool,
    flags: u32,
}

/// Discover all Win32 and MSI applications from the 4 standard Windows Uninstall registry views.
pub fn discover_win32_apps() -> Vec<InstalledApp> {
    let targets = [
        ScanTarget {
            hive_name: "HKLM",
            view_name: "64",
            scope: InstallScope::Machine,
            arch: Arch::X64,
            is_hklm: true,
            flags: KEY_READ | KEY_WOW64_64KEY,
        },
        ScanTarget {
            hive_name: "HKLM",
            view_name: "32",
            scope: InstallScope::Machine,
            arch: Arch::X86,
            is_hklm: true,
            flags: KEY_READ | KEY_WOW64_32KEY,
        },
        ScanTarget {
            hive_name: "HKCU",
            view_name: "64",
            scope: InstallScope::User,
            arch: Arch::X64,
            is_hklm: false,
            flags: KEY_READ | KEY_WOW64_64KEY,
        },
        ScanTarget {
            hive_name: "HKCU",
            view_name: "32",
            scope: InstallScope::User,
            arch: Arch::X86,
            is_hklm: false,
            flags: KEY_READ | KEY_WOW64_32KEY,
        },
    ];

    let all: Vec<InstalledApp> = targets
        .par_iter()
        .flat_map(scan_single_view)
        .collect();

    // Deduplicate identical HKCU 64/32 mirrored entries on Windows 11 (HKCU Uninstall is shared).
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(all.len());
    for app in all {
        let dedup_key = format!(
            "{}|{}|{}",
            app.registry_hive,
            app.registry_key.to_lowercase(),
            app.uninstall_string.to_lowercase()
        );
        if seen.insert(dedup_key) {
            deduped.push(app);
        }
    }
    deduped
}

fn scan_single_view(target: &ScanTarget) -> Vec<InstalledApp> {
    let root = RegKey::predef(if target.is_hklm {
        HKEY_LOCAL_MACHINE
    } else {
        HKEY_CURRENT_USER
    });
    let Ok(uninstall_key) = root.open_subkey_with_flags(UNINSTALL_SUBKEY, target.flags) else {
        return Vec::new();
    };

    let subkey_names: Vec<String> = uninstall_key.enum_keys().flatten().collect();
    let mut results = Vec::with_capacity(subkey_names.len());

    for sub_name in subkey_names {
        let Ok(sub) = uninstall_key.open_subkey_with_flags(&sub_name, target.flags) else {
            continue;
        };

        let display_name = read_reg_string(&sub, "DisplayName");
        if display_name.is_empty() {
            continue;
        }

        let publisher = read_reg_string(&sub, "Publisher");
        let version = read_reg_string(&sub, "DisplayVersion");
        let raw_date = read_reg_string(&sub, "InstallDate");
        let mut install_location = clean_path_string(&read_reg_string(&sub, "InstallLocation"));
        let uninstall_string = read_reg_string(&sub, "UninstallString");
        let raw_quiet = read_reg_string(&sub, "QuietUninstallString");
        let display_icon = read_reg_string(&sub, "DisplayIcon");
        let url_info = {
            let u = read_reg_string(&sub, "URLInfoAbout");
            if u.is_empty() {
                read_reg_string(&sub, "HelpLink")
            } else {
                u
            }
        };

        let system_component = read_reg_u32(&sub, "SystemComponent") == Some(1);
        let windows_installer = read_reg_u32(&sub, "WindowsInstaller") == Some(1);
        let release_type = read_reg_string(&sub, "ReleaseType").to_lowercase();
        let parent_key = read_reg_string(&sub, "ParentKeyName");
        let is_windows_update = !parent_key.is_empty()
            || release_type.contains("update")
            || release_type.contains("hotfix")
            || sub_name.starts_with("KB")
            || display_name.starts_with("Security Update for")
            || display_name.starts_with("Update for Windows");

        // Infer install_location when registry leaves it empty.
        if install_location.is_empty() {
            install_location = infer_install_location(&display_icon, &uninstall_string);
        }

        let raw_size_kb = read_reg_u32(&sub, "EstimatedSize").unwrap_or(0) as u64;
        let estimated_size_kb = normalize_estimated_size_kb(raw_size_kb, &install_location);

        let product_code = extract_guid(&sub_name)
            .or_else(|| extract_guid(&uninstall_string))
            .unwrap_or_default();

        let kind = if windows_installer
            || (!product_code.is_empty()
                && uninstall_string.to_lowercase().contains("msiexec"))
        {
            AppKind::Msi
        } else {
            AppKind::Win32
        };

        let quiet_uninstall_string = if !raw_quiet.is_empty() {
            raw_quiet
        } else {
            synthesize_quiet_uninstall(&uninstall_string, &product_code, kind)
        };

        let install_date = normalize_install_date(&raw_date, &install_location);

        let full_reg_key = if target.hive_name == "HKLM" && target.view_name == "32" {
            format!(
                "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
                sub_name
            )
        } else {
            format!("{}\\{}\\{}", target.hive_name, UNINSTALL_SUBKEY, sub_name)
        };

        let has_uninstaller = !uninstall_string.trim().is_empty() || !quiet_uninstall_string.trim().is_empty();
        let is_valid_entry = check_entry_validity(&uninstall_string, &install_location, kind);
        let package_manager = detect_package_manager(&install_location, &uninstall_string, &full_reg_key);

        let id = format!(
            "win32:{}:{}:{}",
            target.hive_name.to_lowercase(),
            target.view_name,
            sub_name
        );

        results.push(InstalledApp {
            id,
            name: display_name,
            publisher,
            version,
            install_date,
            estimated_size_kb,
            install_location,
            uninstall_string,
            quiet_uninstall_string,
            display_icon,
            url_info_about: url_info,
            registry_key: full_reg_key,
            registry_hive: target.hive_name.to_string(),
            registry_view: target.view_name.to_string(),
            product_code,
            package_full_name: String::new(),
            package_family_name: String::new(),
            kind,
            scope: target.scope,
            arch: target.arch,
            is_system_component: system_component,
            is_windows_update,
            is_valid_entry,
            has_uninstaller,
            tracked_trace_id: None,
            package_manager,
            size_measured: false,
        });
    }

    results
}

fn read_reg_string(key: &RegKey, name: &str) -> String {
    key.get_value::<String, _>(name)
        .map(|s| s.trim().trim_end_matches('\0').to_string())
        .unwrap_or_default()
}

fn read_reg_u32(key: &RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}

pub fn expand_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(end) = input[i + 1..].find('%') {
                let var_name = &input[i + 1..i + 1 + end];
                if !var_name.is_empty() {
                    if let Ok(val) = std::env::var(var_name) {
                        out.push_str(&val);
                        i = i + 1 + end + 1;
                        continue;
                    }
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

pub fn clean_path_string(raw: &str) -> String {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let expanded = expand_env_vars(trimmed);
    expanded.trim_end_matches('\\').to_string()
}

/// Extract the leading executable path from a Windows command line string.
pub fn extract_executable_from_command(cmd: &str) -> Option<PathBuf> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split('"').next().unwrap_or("").trim().to_string()
    } else {
        // Unquoted path might still contain spaces (classic Windows registry quirk).
        let lower = trimmed.to_lowercase();
        let mut found = None;
        for ext in [".exe", ".msi", ".bat", ".cmd", ".vbs", ".msc"] {
            if let Some(idx) = lower.find(ext) {
                found = Some(trimmed[..idx + ext.len()].trim().to_string());
                break;
            }
        }
        found.unwrap_or_else(|| trimmed.split_whitespace().next().unwrap_or("").to_string())
    };

    if candidate.is_empty() {
        return None;
    }

    let expanded = expand_env_vars(&candidate);
    Some(PathBuf::from(expanded))
}

fn infer_install_location(display_icon: &str, uninstall_string: &str) -> String {
    // Prefer DisplayIcon parent dir if it points to a dedicated app folder.
    let icon_path = display_icon
        .split(',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"');

    for raw in [icon_path, uninstall_string] {
        let exe = if raw == icon_path {
            let expanded = expand_env_vars(raw);
            if expanded.is_empty() {
                None
            } else {
                Some(PathBuf::from(expanded))
            }
        } else {
            extract_executable_from_command(raw)
        };

        if let Some(path) = exe {
            let lower = path.to_string_lossy().to_lowercase();
            if lower.contains("msiexec")
                || lower.contains("\\windows\\installer\\")
                || lower.contains("\\package cache\\")
                || lower.contains("\\system32\\")
                || lower.contains("\\syswow64\\")
            {
                continue;
            }
            if let Some(parent) = path.parent() {
                if parent.exists() && !is_protected_path(parent) {
                    return parent.to_string_lossy().to_string();
                }
            }
        }
    }
    String::new()
}

pub fn extract_guid(input: &str) -> Option<String> {
    let start = input.find('{')?;
    let end = input[start..].find('}')?;
    let guid = &input[start..=start + end];
    if guid.len() == 38 && guid.chars().filter(|&c| c == '-').count() == 4 {
        Some(guid.to_uppercase())
    } else {
        None
    }
}

pub fn synthesize_quiet_uninstall(
    uninstall_string: &str,
    product_code: &str,
    kind: AppKind,
) -> String {
    if kind == AppKind::Msi && !product_code.is_empty() {
        return format!("msiexec.exe /X{} /qn /norestart", product_code);
    }

    let trimmed = uninstall_string.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let lower = trimmed.to_lowercase();
    if lower.contains("msiexec") {
        if let Some(guid) = extract_guid(trimmed) {
            return format!("msiexec.exe /X{} /qn /norestart", guid);
        }
    }

    if let Some(exe_path) = extract_executable_from_command(trimmed) {
        let filename = exe_path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_lowercase();

        // Inno Setup standard uninstaller (unins000.exe, unins001.exe, etc.)
        if filename.starts_with("unins")
            && filename.ends_with(".exe")
            && filename.as_bytes().get(5).map(|b| b.is_ascii_digit()).unwrap_or(false)
        {
            return format!(
                "\"{}\" /VERYSILENT /NORESTART /SUPPRESSMSGBOXES",
                exe_path.display()
            );
        }

        // Squirrel (Electron apps: VS Code, Discord, Slack, etc.)
        if filename == "update.exe" && lower.contains("--uninstall") {
            return format!("{} -s", trimmed);
        }

        // NSIS standard uninstaller heuristic
        if filename == "uninstall.exe" || filename == "uninst.exe" {
            return format!("\"{}\" /S", exe_path.display());
        }
    }

    String::new()
}

pub fn normalize_install_date(raw_date: &str, install_location: &str) -> String {
    let digits: String = raw_date.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() == 8 {
        let y = &digits[0..4];
        let m = &digits[4..6];
        let d = &digits[6..8];
        if (1990..=2100).contains(&y.parse::<u32>().unwrap_or(0))
            && (1..=12).contains(&m.parse::<u32>().unwrap_or(0))
            && (1..=31).contains(&d.parse::<u32>().unwrap_or(0))
        {
            return format!("{}-{}-{}", y, m, d);
        }
    }

    // Fallback to install directory creation/modification date if available.
    if !install_location.is_empty() {
        if let Ok(meta) = std::fs::metadata(install_location) {
            if let Ok(time) = meta.created().or_else(|_| meta.modified()) {
                let dt: chrono::DateTime<chrono::Local> = time.into();
                return dt.format("%Y-%m-%d").to_string();
            }
        }
    }

    raw_date.trim().to_string()
}

/// Guard against broken installers (e.g. WeChat 4.x) that write byte counts or 32-bit overflow
/// values into the `EstimatedSize` DWORD instead of kilobytes.
pub fn normalize_estimated_size_kb(raw_kb: u64, install_location: &str) -> u64 {
    const SUSPICIOUS_THRESHOLD_KB: u64 = 200 * 1024 * 1024; // 200 GB expressed in KB
    if raw_kb <= SUSPICIOUS_THRESHOLD_KB {
        return raw_kb;
    }

    // Prefer bounded real directory size when install_location points to a valid non-protected dir.
    if !install_location.is_empty() {
        let dir = Path::new(install_location);
        if dir.is_dir() && !is_protected_path(dir) {
            let measured_kb = crate::uninstaller::leftover::compute_dir_size_bounded(dir, 3000) / 1024;
            if measured_kb > 0 {
                return measured_kb;
            }
        }
    }

    // Values up to 4 GiB written in bytes convert cleanly by dividing by 1024.
    if raw_kb < 0x8000_0000 {
        raw_kb / 1024
    } else {
        0
    }
}

fn check_entry_validity(uninstall_string: &str, install_location: &str, kind: AppKind) -> bool {
    if kind == AppKind::Msi {
        return true;
    }
    if !install_location.is_empty() && Path::new(install_location).exists() {
        return true;
    }
    if let Some(exe) = extract_executable_from_command(uninstall_string) {
        let lower = exe.to_string_lossy().to_lowercase();
        if lower == "msiexec"
            || lower == "msiexec.exe"
            || lower == "rundll32"
            || lower == "rundll32.exe"
            || lower == "cmd"
            || lower == "cmd.exe"
            || lower.ends_with("\\msiexec.exe")
        {
            return true;
        }
        return exe.exists();
    }
    false
}

pub fn detect_package_manager(
    install_location: &str,
    uninstall_string: &str,
    registry_key: &str,
) -> PackageManagerKind {
    let hay = format!(
        "{}|{}|{}",
        install_location.replace('/', "\\").to_lowercase(),
        uninstall_string.replace('/', "\\").to_lowercase(),
        registry_key.replace('/', "\\").to_lowercase()
    );
    if hay.contains("\\scoop\\apps\\") {
        PackageManagerKind::Scoop
    } else if hay.contains("\\chocolatey\\lib\\") || hay.contains("choco.exe") {
        PackageManagerKind::Chocolatey
    } else if hay.contains("\\microsoft\\winget\\packages\\")
        || hay.contains("\\winget\\links\\")
    {
        PackageManagerKind::Winget
    } else {
        PackageManagerKind::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_package_manager() {
        assert_eq!(
            detect_package_manager("C:\\Users\\Me\\scoop\\apps\\ripgrep\\current", "", ""),
            PackageManagerKind::Scoop
        );
        assert_eq!(
            detect_package_manager("C:\\ProgramData\\chocolatey\\lib\\git\\tools", "", ""),
            PackageManagerKind::Chocolatey
        );
        assert_eq!(
            detect_package_manager(
                "C:\\Users\\Me\\AppData\\Local\\Microsoft\\WinGet\\Packages\\demo",
                "",
                ""
            ),
            PackageManagerKind::Winget
        );
        assert_eq!(
            detect_package_manager("C:\\Program Files\\7-Zip", "", ""),
            PackageManagerKind::None
        );
    }

    #[test]
    fn test_extract_guid_and_quiet_synthesis() {
        let cmd = "MsiExec.exe /I{12345678-1234-1234-1234-1234567890AB}";
        let guid = extract_guid(cmd).unwrap();
        assert_eq!(guid, "{12345678-1234-1234-1234-1234567890AB}");

        let quiet = synthesize_quiet_uninstall(cmd, &guid, AppKind::Msi);
        assert_eq!(
            quiet,
            "msiexec.exe /X{12345678-1234-1234-1234-1234567890AB} /qn /norestart"
        );

        let inno = synthesize_quiet_uninstall("\"C:\\Apps\\Demo\\unins000.exe\"", "", AppKind::Win32);
        assert!(inno.contains("/VERYSILENT"));

        let nsis = synthesize_quiet_uninstall("\"C:\\Apps\\Tool\\uninstall.exe\"", "", AppKind::Win32);
        assert!(nsis.ends_with("/S"));
    }

    #[test]
    fn test_normalize_install_date() {
        assert_eq!(normalize_install_date("20260315", ""), "2026-03-15");
        assert_eq!(normalize_install_date("2025/11/02", ""), "2025-11-02");
    }

    #[test]
    fn test_extract_executable_unquoted_with_spaces() {
        let exe = extract_executable_from_command("C:\\Program Files\\My App\\uninstall.exe /quiet").unwrap();
        assert_eq!(exe, PathBuf::from("C:\\Program Files\\My App\\uninstall.exe"));
    }

    #[test]
    fn test_normalize_estimated_size_bytes_bug() {
        // Normal KB value stays unchanged (500 MB)
        assert_eq!(normalize_estimated_size_kb(512_000, ""), 512_000);
        // WeChat 4.x bug: 1,511,440,925 bytes written into EstimatedSize -> ~1,476,016 KB (~1.4 GB)
        assert_eq!(normalize_estimated_size_kb(1_511_440_925, ""), 1_476_016);
        // Signed overflow bogus value -> 0
        assert_eq!(normalize_estimated_size_kb(0xFFFF_FFFE, ""), 0);
    }
}
