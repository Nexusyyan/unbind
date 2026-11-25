use crate::models::{CoreError, CoreResult, PortableApp, ShortcutLocation};
use crate::safety::{is_protected_path, normalize_fs_path};
use crate::uninstaller::leftover::compute_dir_size_bounded;
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone)]
pub struct PeVersionInfo {
    pub product_name: String,
    pub file_description: String,
    pub company_name: String,
    pub file_version: String,
}

/// Scan user-configured portable software library roots (depth 1-2 app folders plus standalone
/// single-file `.exe` utilities in the root), extracting PE version metadata and real sizes.
pub fn scan_portable_apps(roots: &[String]) -> Vec<PortableApp> {
    let mut valid_roots: Vec<PathBuf> = Vec::new();
    let mut seen_roots: HashSet<String> = HashSet::new();

    for raw in roots {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = PathBuf::from(trimmed);
        if !p.is_dir() || is_protected_path(&p) {
            continue;
        }
        let norm = normalize_fs_path(&p);
        if seen_roots.insert(norm) {
            valid_roots.push(p);
        }
    }

    if valid_roots.is_empty() {
        return Vec::new();
    }

    // Collect candidate entries: (root_dir, target_path, is_dir).
    let mut candidates: Vec<(PathBuf, PathBuf, bool)> = Vec::new();
    for root in &valid_roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with('.') || fname.eq_ignore_ascii_case("unbind-data") {
                continue;
            }
            if ft.is_dir() {
                if !is_protected_path(&path) {
                    candidates.push((root.clone(), path, true));
                }
            } else if ft.is_file() && is_candidate_exe(&path) {
                candidates.push((root.clone(), path, false));
            }
        }
    }

    let mut apps: Vec<PortableApp> = candidates
        .into_par_iter()
        .filter_map(|(root, target, is_dir)| {
            if is_dir {
                inspect_portable_dir(&root, &target)
            } else {
                inspect_single_file_portable(&root, &target)
            }
        })
        .collect();

    // Deduplicate by canonical normalized exe path.
    let mut seen_ids: HashSet<String> = HashSet::new();
    apps.retain(|a| seen_ids.insert(a.id.clone()));
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps
}

fn inspect_portable_dir(root: &Path, dir: &Path) -> Option<PortableApp> {
    let folder_name = dir.file_name()?.to_string_lossy().trim().to_string();
    if folder_name.is_empty() {
        return None;
    }

    let primary_exe = select_primary_executable(dir, &folder_name)?;
    let pe = read_pe_version_info(&primary_exe);
    let display_name = choose_display_name(&pe, &folder_name, &primary_exe);
    let size_bytes = compute_dir_size_bounded(dir, 6000);
    let last_modified = read_modified_iso(&primary_exe)
        .or_else(|| read_modified_iso(dir))
        .unwrap_or_default();

    let id = format!("portable:{}", normalize_fs_path(&primary_exe));
    Some(PortableApp {
        id,
        name: display_name,
        publisher: pe.company_name,
        version: clean_version_string(&pe.file_version),
        exe_path: primary_exe.to_string_lossy().to_string(),
        install_dir: dir.to_string_lossy().to_string(),
        root_dir: root.to_string_lossy().to_string(),
        size_bytes,
        last_modified,
        is_single_file: false,
    })
}

fn inspect_single_file_portable(root: &Path, exe_path: &Path) -> Option<PortableApp> {
    let stem = exe_path.file_stem()?.to_string_lossy().trim().to_string();
    if stem.is_empty() {
        return None;
    }
    let pe = read_pe_version_info(exe_path);
    let display_name = choose_display_name(&pe, &stem, exe_path);
    let size_bytes = fs::metadata(exe_path).map(|m| m.len()).unwrap_or(0);
    let last_modified = read_modified_iso(exe_path).unwrap_or_default();
    let id = format!("portable:{}", normalize_fs_path(exe_path));

    Some(PortableApp {
        id,
        name: display_name,
        publisher: pe.company_name,
        version: clean_version_string(&pe.file_version),
        exe_path: exe_path.to_string_lossy().to_string(),
        install_dir: root.to_string_lossy().to_string(),
        root_dir: root.to_string_lossy().to_string(),
        size_bytes,
        last_modified,
        is_single_file: true,
    })
}

fn is_candidate_exe(path: &Path) -> bool {
    let is_exe = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("exe"))
        .unwrap_or(false);
    if !is_exe {
        return false;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    !is_helper_exe_name(&stem)
}

fn is_helper_exe_name(stem_lower: &str) -> bool {
    const BLOCKED_PREFIXES: &[&str] = &[
        "unins",
        "uninstall",
        "update",
        "updater",
        "crash",
        "createdump",
        "elevate",
        "vc_redist",
        "vcredist",
        "dotnet",
        "dxsetup",
        "setup",
        "install",
        "squirrel",
        "nacl64",
        "notification_helper",
        "chrome_pwa_launcher",
        "elevation_service",
    ];
    if BLOCKED_PREFIXES.iter().any(|p| stem_lower.starts_with(p)) {
        return true;
    }
    stem_lower.contains("crashpad")
        || stem_lower.contains("crashhandler")
        || stem_lower.contains("crashreport")
        || stem_lower.ends_with("_helper")
        || stem_lower.ends_with("-helper")
        || stem_lower.ends_with("helper")
}

/// Find the most likely primary executable inside a portable application folder (depth 1-2).
pub fn select_primary_executable(dir: &Path, folder_name: &str) -> Option<PathBuf> {
    let folder_norm = normalize_token(folder_name);
    let mut scored: Vec<(i64, PathBuf)> = Vec::new();

    for entry in walkdir::WalkDir::new(dir)
        .min_depth(1)
        .max_depth(2)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !is_candidate_exe(path) {
            continue;
        }

        let depth = entry.depth() as i64;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let stem_norm = normalize_token(&stem);
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);

        let mut score: i64 = 0;
        if depth == 1 {
            score += 40;
        } else {
            // Prefer conventional bin/app subdirectories at depth 2.
            if let Some(parent) = path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str())
            {
                let pl = parent.to_lowercase();
                if pl == "bin" || pl == "app" || pl == "program" || pl.starts_with("app-") {
                    score += 18;
                }
            }
        }

        if !folder_norm.is_empty() && !stem_norm.is_empty() {
            if stem_norm == folder_norm {
                score += 120;
            } else if folder_norm.contains(&stem_norm) || stem_norm.contains(&folder_norm) {
                score += 65;
            }
        }

        // Mild tie-breaker favoring larger primary binaries over tiny CLI shims.
        score += ((size / (64 * 1024)).min(35)) as i64;

        scored.push((score, path.to_path_buf()));
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().next().map(|(_, p)| p)
}

fn normalize_token(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn choose_display_name(pe: &PeVersionInfo, fallback: &str, exe_path: &Path) -> String {
    let candidate = pe.product_name.trim();
    if is_usable_product_name(candidate) {
        return candidate.to_string();
    }
    let desc = pe.file_description.trim();
    if is_usable_product_name(desc) && desc.len() <= 48 {
        return desc.to_string();
    }
    if !fallback.trim().is_empty() {
        return fallback.trim().to_string();
    }
    exe_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Portable App")
        .to_string()
}

fn is_usable_product_name(s: &str) -> bool {
    if s.len() < 2 || s.len() > 80 {
        return false;
    }
    let alpha_only: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    !(alpha_only == "application"
        || alpha_only == "windows"
        || alpha_only == "todo"
        || alpha_only.starts_with("todo ")
        || alpha_only.contains("microsoft windows operating system"))
}

fn clean_version_string(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Strip trailing parenthetical build tags such as "10.0.26100.9818 (WinBuild.160101.0800)".
    let base = trimmed
        .split_once(" (")
        .map(|(head, _)| head.trim())
        .unwrap_or(trimmed);
    // Many PE resources format version as "1, 2, 3, 4".
    if base.contains(',') && !base.contains('.') {
        return base
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(".");
    }
    base.to_string()
}

fn read_modified_iso(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dt: DateTime<Utc> = modified.into();
    Some(dt.format("%Y-%m-%d").to_string())
}

/// Extract PE version resource strings (`ProductName`, `FileDescription`, `CompanyName`,
/// `FileVersion`) using Windows native `GetFileVersionInfoW` + `VerQueryValueW`.
#[cfg(windows)]
pub fn read_pe_version_info(exe_path: &Path) -> PeVersionInfo {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    if !exe_path.is_file() {
        return PeVersionInfo::default();
    }

    let wide: Vec<u16> = exe_path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut handle = 0u32;
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), Some(&mut handle));
        if size == 0 {
            return PeVersionInfo::default();
        }

        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(
            PCWSTR(wide.as_ptr()),
            0,
            size,
            buf.as_mut_ptr() as *mut c_void,
        )
        .is_err()
        {
            return PeVersionInfo::default();
        }

        let mut codepages: Vec<String> = Vec::new();
        let trans_query: Vec<u16> = "\\VarFileInfo\\Translation"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut trans_ptr: *mut c_void = std::ptr::null_mut();
        let mut trans_len: u32 = 0;
        if VerQueryValueW(
            buf.as_ptr() as *const c_void,
            PCWSTR(trans_query.as_ptr()),
            &mut trans_ptr,
            &mut trans_len,
        )
        .as_bool()
            && !trans_ptr.is_null()
            && trans_len >= 4
        {
            let count = (trans_len as usize) / 4;
            let pairs = std::slice::from_raw_parts(trans_ptr as *const u16, count * 2);
            for chunk in pairs.as_chunks::<2>().0 {
                codepages.push(format!("{:04X}{:04X}", chunk[0], chunk[1]));
            }
        }

        for fallback in ["040904B0", "040904E4", "080404B0", "000004B0"] {
            if !codepages.iter().any(|c| c.eq_ignore_ascii_case(fallback)) {
                codepages.push(fallback.to_string());
            }
        }

        let query_field = |field: &str| -> String {
            for cp in &codepages {
                let sub = format!("\\StringFileInfo\\{}\\{}", cp, field);
                let sub_wide: Vec<u16> = sub.encode_utf16().chain(Some(0)).collect();
                let mut val_ptr: *mut c_void = std::ptr::null_mut();
                let mut val_len: u32 = 0;
                if VerQueryValueW(
                    buf.as_ptr() as *const c_void,
                    PCWSTR(sub_wide.as_ptr()),
                    &mut val_ptr,
                    &mut val_len,
                )
                .as_bool()
                    && !val_ptr.is_null()
                    && val_len > 0
                {
                    let slice =
                        std::slice::from_raw_parts(val_ptr as *const u16, val_len as usize);
                    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
                    let s = String::from_utf16_lossy(&slice[..end]).trim().to_string();
                    if !s.is_empty() {
                        return s;
                    }
                }
            }
            String::new()
        };

        PeVersionInfo {
            product_name: query_field("ProductName"),
            file_description: query_field("FileDescription"),
            company_name: query_field("CompanyName"),
            file_version: query_field("FileVersion"),
        }
    }
}

#[cfg(not(windows))]
pub fn read_pe_version_info(_exe_path: &Path) -> PeVersionInfo {
    PeVersionInfo::default()
}

/// Launch a portable executable with its working directory set cleanly to its containing directory.
#[cfg(windows)]
pub fn launch_portable_app(exe_path: &Path, working_dir: &Path) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if !exe_path.is_file() {
        return Err(CoreError::NotFound(format!(
            "Portable executable not found: {}",
            exe_path.display()
        )));
    }

    let dir = if working_dir.is_dir() {
        working_dir
    } else {
        exe_path.parent().unwrap_or(working_dir)
    };

    let exe_wide: Vec<u16> = exe_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let dir_wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        let res = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(exe_wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR(dir_wide.as_ptr()),
            SW_SHOWNORMAL,
        );
        if (res.0 as isize) <= 32 {
            return Err(CoreError::WinApi(format!(
                "ShellExecuteW failed launching {} (code {})",
                exe_path.display(),
                res.0 as isize
            )));
        }
    }

    Ok(())
}

#[cfg(not(windows))]
pub fn launch_portable_app(_exe_path: &Path, _working_dir: &Path) -> CoreResult<()> {
    Ok(())
}

/// Create a `.lnk` shortcut for a portable application on the Desktop or in the Start Menu.
pub fn create_portable_shortcut(
    exe_path: &Path,
    name: &str,
    location: ShortcutLocation,
) -> CoreResult<PathBuf> {
    if !exe_path.is_file() {
        return Err(CoreError::NotFound(format!(
            "Portable executable not found: {}",
            exe_path.display()
        )));
    }

    let target_dir = resolve_shortcut_dir(location)?;
    fs::create_dir_all(&target_dir)?;

    let safe_name = sanitize_shortcut_filename(name);
    let lnk_path = target_dir.join(format!("{}.lnk", safe_name));
    create_shortcut_at(exe_path, &lnk_path, name)?;
    Ok(lnk_path)
}

fn resolve_shortcut_dir(location: ShortcutLocation) -> CoreResult<PathBuf> {
    match location {
        ShortcutLocation::Desktop => {
            let profile = std::env::var("USERPROFILE").map_err(|_| {
                CoreError::NotFound("USERPROFILE environment variable not set".to_string())
            })?;
            Ok(PathBuf::from(profile).join("Desktop"))
        }
        ShortcutLocation::StartMenu => {
            let appdata = std::env::var("APPDATA").map_err(|_| {
                CoreError::NotFound("APPDATA environment variable not set".to_string())
            })?;
            Ok(PathBuf::from(appdata).join("Microsoft\\Windows\\Start Menu\\Programs"))
        }
    }
}

fn sanitize_shortcut_filename(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if c.is_control() => '_',
            _ => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == ' ');
    if trimmed.is_empty() {
        "PortableApp".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

/// Create a Windows `.lnk` file at an explicit destination path via COM `IShellLinkW`.
#[cfg(windows)]
pub fn create_shortcut_at(exe_path: &Path, lnk_path: &Path, description: &str) -> CoreResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{Interface, PCWSTR};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let work_dir = exe_path.parent().unwrap_or(Path::new("."));
    let exe_wide: Vec<u16> = exe_path.as_os_str().encode_wide().chain(Some(0)).collect();
    let dir_wide: Vec<u16> = work_dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let desc_wide: Vec<u16> = description.encode_utf16().chain(Some(0)).collect();
    let lnk_wide: Vec<u16> = lnk_path.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        let init_hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let need_uninit = init_hr.is_ok();

        let res = (|| -> CoreResult<()> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| CoreError::WinApi(format!("CoCreateInstance ShellLink failed: {}", e)))?;
            link.SetPath(PCWSTR(exe_wide.as_ptr()))
                .map_err(|e| CoreError::WinApi(format!("IShellLinkW::SetPath failed: {}", e)))?;
            link.SetWorkingDirectory(PCWSTR(dir_wide.as_ptr()))
                .map_err(|e| {
                    CoreError::WinApi(format!("IShellLinkW::SetWorkingDirectory failed: {}", e))
                })?;
            link.SetIconLocation(PCWSTR(exe_wide.as_ptr()), 0)
                .map_err(|e| {
                    CoreError::WinApi(format!("IShellLinkW::SetIconLocation failed: {}", e))
                })?;
            if !description.trim().is_empty() {
                let _ = link.SetDescription(PCWSTR(desc_wide.as_ptr()));
            }

            let persist: IPersistFile = link
                .cast()
                .map_err(|e| CoreError::WinApi(format!("Cast to IPersistFile failed: {}", e)))?;
            persist
                .Save(PCWSTR(lnk_wide.as_ptr()), true)
                .map_err(|e| CoreError::WinApi(format!("IPersistFile::Save failed: {}", e)))?;
            Ok(())
        })();

        if need_uninit {
            CoUninitialize();
        }
        res
    }
}

#[cfg(not(windows))]
pub fn create_shortcut_at(_exe_path: &Path, _lnk_path: &Path, _description: &str) -> CoreResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unlocker::resolve_shortcut_target;
    use tempfile::tempdir;

    #[test]
    fn test_select_primary_exe_prefers_folder_match_and_skips_helpers() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("MyCoolTool");
        fs::create_dir_all(&app_dir).unwrap();
        fs::write(app_dir.join("unins000.exe"), vec![0u8; 200_000]).unwrap();
        fs::write(app_dir.join("updater.exe"), vec![0u8; 200_000]).unwrap();
        fs::write(app_dir.join("MyCoolTool.exe"), vec![0u8; 50_000]).unwrap();
        fs::write(app_dir.join("cli.exe"), vec![0u8; 20_000]).unwrap();

        let picked = select_primary_executable(&app_dir, "MyCoolTool").unwrap();
        assert_eq!(
            picked.file_name().unwrap().to_string_lossy(),
            "MyCoolTool.exe"
        );
    }

    #[test]
    fn test_scan_portable_apps_and_pe_version_and_shortcut() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("Portable");
        let app_dir = root.join("NotepadCopy");
        fs::create_dir_all(&app_dir).unwrap();

        let sys_notepad = Path::new("C:\\Windows\\System32\\notepad.exe");
        let copied_exe = app_dir.join("NotepadCopy.exe");
        if sys_notepad.exists() {
            fs::copy(sys_notepad, &copied_exe).unwrap();
        } else {
            fs::write(&copied_exe, b"MZ-stub").unwrap();
        }

        let found = scan_portable_apps(&[root.to_string_lossy().to_string()]);
        assert_eq!(found.len(), 1);
        assert!(!found[0].is_single_file);
        assert!(found[0].size_bytes > 0);
        if sys_notepad.exists() {
            let pe = read_pe_version_info(&copied_exe);
            assert!(
                !pe.company_name.is_empty() || !pe.file_version.is_empty(),
                "Expected PE version metadata from Windows notepad.exe"
            );
        }

        // Verify COM .lnk creation and roundtrip resolution in tempdir.
        let lnk = dir.path().join("NotepadCopy.lnk");
        create_shortcut_at(&copied_exe, &lnk, "NotepadCopy Portable").unwrap();
        assert!(lnk.exists());
        let resolved = resolve_shortcut_target(&lnk).unwrap();
        assert_eq!(
            fs::canonicalize(&resolved).unwrap(),
            fs::canonicalize(&copied_exe).unwrap()
        );
    }

    #[test]
    fn test_sanitize_shortcut_filename() {
        assert_eq!(sanitize_shortcut_filename("App: Pro / 2.0*"), "App_ Pro _ 2.0_");
        assert_eq!(sanitize_shortcut_filename("..."), "PortableApp");
        assert!(!is_usable_product_name("Microsoft\u{00ae} Windows\u{00ae} Operating System"));
        assert!(is_usable_product_name("7-Zip File Manager"));
        assert_eq!(
            clean_version_string("10.0.26100.9818 (WinBuild.160101.0800)"),
            "10.0.26100.9818"
        );
    }
}
