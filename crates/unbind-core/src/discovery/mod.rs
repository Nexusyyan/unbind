pub mod appx;
pub mod icon;
pub mod win32;

use crate::models::{CoreError, CoreResult, InstalledApp};
use crate::tracker::load_all_traces;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Discover both Win32/MSI and UWP/MSIX applications in parallel, enriched with Install Tracker metadata.
pub fn discover_all_apps(traces_dir: &Path) -> Vec<InstalledApp> {
    let (mut win32_apps, appx_apps) = rayon::join(win32::discover_win32_apps, appx::discover_appx_apps);
    win32_apps.extend(appx_apps);

    // Annotate apps that have matching saved Install Tracker traces.
    let traces = load_all_traces(traces_dir);
    if !traces.is_empty() {
        let trace_map: HashMap<String, String> = traces
            .into_iter()
            .map(|t| (t.app_name.to_lowercase(), t.id))
            .collect();
        for app in &mut win32_apps {
            let lower = app.name.to_lowercase();
            if let Some(tid) = trace_map.get(&lower) {
                app.tracked_trace_id = Some(tid.clone());
            } else if let Some((_, tid)) = trace_map
                .iter()
                .find(|(tname, _)| tname.len() >= 4 && (lower.contains(tname.as_str()) || tname.contains(lower.as_str())))
            {
                app.tracked_trace_id = Some(tid.clone());
            }
        }
    }

    win32_apps.sort_by_key(|a| a.name.to_lowercase());
    win32_apps
}

/// Open a folder or highlight a file in Windows Explorer.
pub fn open_in_explorer(path: &str) -> CoreResult<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(CoreError::NotFound(format!("Path does not exist: {}", path)));
    }
    if p.is_file() {
        Command::new("explorer.exe")
            .args(["/select,", path])
            .spawn()?;
    } else {
        Command::new("explorer.exe").arg(path).spawn()?;
    }
    Ok(())
}

/// Open Windows Registry Editor directly at the given registry key using the `LastKey` trick.
pub fn open_in_regedit(full_reg_path: &str) -> CoreResult<()> {
    use winreg::enums::*;
    use winreg::RegKey;

    let Some((hive, subkey)) = crate::safety::split_registry_path(full_reg_path) else {
        return Err(CoreError::InvalidInput(format!(
            "Invalid registry key: {}",
            full_reg_path
        )));
    };
    let long_hive = crate::safety::hive_to_long_name(&hive);
    let last_key = format!("Computer\\{}\\{}", long_hive, subkey);

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((regedit_key, _)) =
        hkcu.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Applets\\Regedit")
    {
        let _ = regedit_key.set_value("LastKey", &last_key);
    }

    // Launch regedit (will prompt UAC if non-elevated; ShellExecute handles elevation smoothly).
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let exe: Vec<u16> = std::ffi::OsStr::new("regedit.exe")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let args: Vec<u16> = std::ffi::OsStr::new("-m")
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(exe.as_ptr()),
                PCWSTR(args.as_ptr()),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }
    Ok(())
}

/// Open an external http(s) URL in the default system browser.
pub fn open_url(url: &str) -> CoreResult<()> {
    let trimmed = url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(CoreError::InvalidInput(
            "Only http:// and https:// URLs are allowed".to_string(),
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let wide: Vec<u16> = std::ffi::OsStr::new(trimmed)
            .encode_wide()
            .chain(Some(0))
            .collect();
        unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                PCWSTR(wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AppKind;

    #[test]
    fn test_live_discovery_and_icon_extraction() {
        let tmp = tempfile::tempdir().unwrap();
        let apps = discover_all_apps(tmp.path());
        assert!(!apps.is_empty(), "Expected at least one installed app on Windows");

        let win32_count = apps.iter().filter(|a| a.kind != AppKind::StoreApp).count();
        let appx_count = apps.iter().filter(|a| a.kind == AppKind::StoreApp).count();
        println!(
            "Discovered {} total apps ({} Win32/MSI, {} UWP/Store)",
            apps.len(),
            win32_count,
            appx_count
        );
        assert!(win32_count > 0, "Expected Win32/MSI apps");

        // Verify icon extraction works for at least one real app on the host.
        let cache = icon::IconCache::new();
        let extracted = apps
            .iter()
            .take(30)
            .filter_map(|a| cache.get_or_extract(a))
            .filter(|uri| uri.starts_with("data:image/"))
            .count();
        println!("Extracted {} real icons from first 30 apps", extracted);
        assert!(extracted > 0, "Expected at least one extracted app icon");
    }
}
