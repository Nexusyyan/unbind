use crate::models::{AppKind, Arch, CoreError, CoreResult, InstallScope, InstalledApp};
use std::path::{Path, PathBuf};

/// Discover installed UWP / MSIX Microsoft Store apps for the current user via WinRT PackageManager.
#[cfg(windows)]
pub fn discover_appx_apps() -> Vec<InstalledApp> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;
    use windows::System::ProcessorArchitecture;

    let Ok(pm) = PackageManager::new() else {
        return Vec::new();
    };

    let Ok(packages) = pm.FindPackagesByUserSecurityId(&HSTRING::new()) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for pkg in packages {
        // Skip framework and resource packages (VCLibs, WinAppRuntime, language packs, etc.).
        if pkg.IsFramework().unwrap_or(false) || pkg.IsResourcePackage().unwrap_or(false) {
            continue;
        }

        let Ok(pkg_id) = pkg.Id() else {
            continue;
        };

        let full_name = pkg_id.FullName().map(|s| s.to_string()).unwrap_or_default();
        let family_name = pkg_id.FamilyName().map(|s| s.to_string()).unwrap_or_default();
        let raw_name = pkg_id.Name().map(|s| s.to_string()).unwrap_or_default();
        if full_name.is_empty() {
            continue;
        }

        let display_name = pkg
            .DisplayName()
            .map(|s| s.to_string())
            .ok()
            .filter(|s| !s.trim().is_empty() && !s.starts_with("ms-resource:"))
            .unwrap_or_else(|| prettify_package_name(&raw_name));

        let publisher = pkg
            .PublisherDisplayName()
            .map(|s| s.to_string())
            .ok()
            .filter(|s| !s.trim().is_empty() && !s.starts_with("ms-resource:"))
            .unwrap_or_else(|| {
                pkg_id
                    .Publisher()
                    .map(|s| extract_cn(&s.to_string()))
                    .unwrap_or_default()
            });

        let version = pkg_id
            .Version()
            .map(|v| format!("{}.{}.{}.{}", v.Major, v.Minor, v.Build, v.Revision))
            .unwrap_or_default();

        let arch = match pkg_id.Architecture() {
            Ok(ProcessorArchitecture::X64) => Arch::X64,
            Ok(ProcessorArchitecture::X86) => Arch::X86,
            Ok(ProcessorArchitecture::Arm64) => Arch::Arm64,
            _ => Arch::Neutral,
        };

        let install_location = pkg
            .InstalledLocation()
            .and_then(|loc| loc.Path())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let install_date = pkg
            .InstalledDate()
            .ok()
            .and_then(|dt| {
                // Windows FILETIME (100ns ticks since 1601-01-01) -> Unix seconds
                let unix_secs = (dt.UniversalTime / 10_000_000) - 11_644_473_600;
                chrono::DateTime::from_timestamp(unix_secs, 0)
                    .map(|c| c.format("%Y-%m-%d").to_string())
            })
            .unwrap_or_default();

        let logo_path = pkg
            .Logo()
            .and_then(|uri| uri.Path())
            .ok()
            .map(|s| {
                let raw = s.to_string();
                let trimmed = raw.strip_prefix('/').unwrap_or(&raw);
                trimmed.to_string()
            })
            .and_then(|p: String| resolve_appx_logo(&install_location, &p))
            .unwrap_or_default();

        let signature_kind = pkg.SignatureKind().ok();
        let is_system_sig = signature_kind
            .map(|k| k == windows::ApplicationModel::PackageSignatureKind::System)
            .unwrap_or(false);
        let is_system_component = is_system_sig
            || raw_name.starts_with("Windows.")
            || raw_name.starts_with("Microsoft.Windows.Shell")
            || raw_name.starts_with("Microsoft.Windows.StartMenu")
            || raw_name.starts_with("Microsoft.AAD.")
            || raw_name.starts_with("Microsoft.AccountsControl")
            || raw_name.starts_with("Microsoft.LockApp");

        results.push(InstalledApp {
            id: format!("appx:{}", full_name),
            name: display_name,
            publisher,
            version,
            install_date,
            estimated_size_kb: 0,
            install_location,
            uninstall_string: format!("appx://{}", full_name),
            quiet_uninstall_string: format!("appx://{}", full_name),
            display_icon: logo_path,
            url_info_about: String::new(),
            registry_key: String::new(),
            registry_hive: String::new(),
            registry_view: String::new(),
            product_code: String::new(),
            package_full_name: full_name,
            package_family_name: family_name,
            kind: AppKind::StoreApp,
            scope: InstallScope::User,
            arch,
            is_system_component,
            is_windows_update: false,
            is_valid_entry: true,
            has_uninstaller: !is_system_component,
            tracked_trace_id: None,
        });
    }

    results
}

#[cfg(not(windows))]
pub fn discover_appx_apps() -> Vec<InstalledApp> {
    Vec::new()
}

/// Remove a UWP / MSIX package via WinRT PackageManager::RemovePackageAsync.
#[cfg(windows)]
pub fn remove_appx_package(package_full_name: &str) -> CoreResult<()> {
    use windows::core::HSTRING;
    use windows::Management::Deployment::PackageManager;

    if package_full_name.trim().is_empty() {
        return Err(CoreError::InvalidInput(
            "Empty package_full_name".to_string(),
        ));
    }

    let pm = PackageManager::new()
        .map_err(|e| CoreError::WinApi(format!("PackageManager init failed: {}", e)))?;
    let op = pm
        .RemovePackageAsync(&HSTRING::from(package_full_name))
        .map_err(|e| CoreError::WinApi(format!("RemovePackageAsync failed: {}", e)))?;

    let result = op
        .get()
        .map_err(|e| CoreError::WinApi(format!("RemovePackageAsync wait failed: {}", e)))?;

    let ext = result.ExtendedErrorCode().unwrap_or_default();
    if ext.is_err() {
        let err_text = result.ErrorText().map(|s| s.to_string()).unwrap_or_default();
        return Err(CoreError::WinApi(format!(
            "Failed to remove package {}: {} ({:?})",
            package_full_name, err_text, ext
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn remove_appx_package(_package_full_name: &str) -> CoreResult<()> {
    Ok(())
}

fn prettify_package_name(raw: &str) -> String {
    let cleaned = raw
        .strip_prefix("Microsoft.")
        .or_else(|| raw.strip_prefix("Windows."))
        .unwrap_or(raw);
    cleaned.replace('.', " ")
}

fn extract_cn(publisher_dn: &str) -> String {
    for part in publisher_dn.split(',') {
        let t = part.trim();
        if let Some(cn) = t.strip_prefix("CN=") {
            return cn.trim_matches('"').to_string();
        }
    }
    publisher_dn.to_string()
}

fn resolve_appx_logo(install_dir: &str, uri_path: &str) -> Option<String> {
    let decoded = uri_path.replace("%20", " ").replace('/', "\\");
    let direct = PathBuf::from(&decoded);
    if direct.exists() {
        return Some(direct.to_string_lossy().to_string());
    }

    // Try scale variants in the same directory (e.g., Logo.scale-100.png, Logo.scale-200.png).
    let candidate = if Path::new(&decoded).is_absolute() {
        PathBuf::from(&decoded)
    } else if !install_dir.is_empty() {
        Path::new(install_dir).join(&decoded)
    } else {
        return None;
    };

    if candidate.exists() {
        return Some(candidate.to_string_lossy().to_string());
    }

    let parent = candidate.parent()?;
    let stem = candidate.file_stem()?.to_str()?.to_lowercase();
    let ext = candidate.extension()?.to_str()?.to_lowercase();

    let entries = std::fs::read_dir(parent).ok()?;
    let mut best: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let fname = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if fname.starts_with(&stem) && fname.ends_with(&format!(".{}", ext)) {
            if fname.contains("scale-200") || fname.contains("targetsize-48") {
                return Some(p.to_string_lossy().to_string());
            }
            if best.is_none() || fname.contains("scale-100") || fname.contains("scale-125") {
                best = Some(p);
            }
        }
    }
    best.map(|p| p.to_string_lossy().to_string())
}
