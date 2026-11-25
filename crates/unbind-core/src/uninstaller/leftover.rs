use crate::discovery::size::uwp_package_data_dir;
use crate::models::{
    AppKind, Arch, ConfidenceLevel, InstallScope, InstallTrace, InstalledApp, LeftoverItem,
    LeftoverKind, PackageManagerKind,
};
use crate::safety::{
    is_generic_token, is_protected_path, is_protected_registry_key, normalize_fs_path,
    open_predef_hive, split_registry_path,
};
use crate::startup::enumerate_third_party_scheduled_tasks;
use crate::unlocker::resolve_shortcut_target;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone)]
pub struct SearchAnchors {
    pub app_name: String,
    pub clean_name: String,
    pub compact_name: String,
    pub publisher: String,
    pub kind: AppKind,
    pub install_dir: Option<PathBuf>,
    pub install_dir_norm: Option<String>,
    pub install_dir_leaf: Option<String>,
    pub product_code: Option<String>,
    pub registry_key: Option<String>,
    pub package_family_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchHit {
    pub confidence: ConfidenceLevel,
    pub reason: String,
    pub reason_code: &'static str,
    pub reason_param: Option<String>,
}

impl SearchAnchors {
    pub fn from_app(app: &InstalledApp) -> Self {
        let clean_name = strip_version_and_noise(&app.name);
        let compact_name: String = clean_name
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();

        let install_dir = if !app.install_location.trim().is_empty() {
            let p = PathBuf::from(&app.install_location);
            if !is_protected_path(&p) {
                Some(p)
            } else {
                None
            }
        } else {
            None
        };

        let install_dir_norm = install_dir.as_ref().map(|p| normalize_fs_path(p));
        let install_dir_leaf = install_dir
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .filter(|s| !is_generic_token(s));

        let product_code = if app.product_code.trim().is_empty() {
            None
        } else {
            Some(app.product_code.trim().to_uppercase())
        };

        let registry_key = if app.registry_key.trim().is_empty() {
            None
        } else {
            Some(app.registry_key.trim().to_string())
        };

        let package_family_name = if app.package_family_name.trim().is_empty() {
            None
        } else {
            Some(app.package_family_name.trim().to_string())
        };

        Self {
            app_name: app.name.clone(),
            clean_name,
            compact_name,
            publisher: app.publisher.trim().to_string(),
            kind: app.kind,
            install_dir,
            install_dir_norm,
            install_dir_leaf,
            product_code,
            registry_key,
            package_family_name,
        }
    }

    /// Match strength against a candidate folder or registry key name.
    pub fn match_name(&self, candidate: &str) -> Option<MatchHit> {
        let cand_trim = candidate.trim();
        if cand_trim.len() < 3 || is_generic_token(cand_trim) {
            return None;
        }
        let cand_lower = cand_trim.to_lowercase();
        let cand_compact: String = cand_lower
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();

        // 1. Exact ProductCode GUID match
        if let Some(guid) = &self.product_code {
            if cand_trim.eq_ignore_ascii_case(guid) {
                return Some(MatchHit {
                    confidence: ConfidenceLevel::High,
                    reason: "Matches MSI ProductCode".to_string(),
                    reason_code: "match_msi_guid",
                    reason_param: Some(guid.clone()),
                });
            }
        }

        // 2. Exact match with install folder leaf name
        if let Some(leaf) = &self.install_dir_leaf {
            if cand_lower == leaf.to_lowercase() {
                return Some(MatchHit {
                    confidence: ConfidenceLevel::High,
                    reason: format!("Exact match with install folder '{}'", leaf),
                    reason_code: "exact_install_folder",
                    reason_param: Some(leaf.clone()),
                });
            }
        }

        // 3. Exact match with cleaned app name (normal or compact)
        let clean_lower = self.clean_name.to_lowercase();
        if !is_generic_token(&clean_lower) {
            if cand_lower == clean_lower
                || (self.compact_name.len() >= 4 && cand_compact == self.compact_name)
            {
                return Some(MatchHit {
                    confidence: ConfidenceLevel::High,
                    reason: format!("Exact match with application name '{}'", self.clean_name),
                    reason_code: "exact_app_name",
                    reason_param: Some(self.clean_name.clone()),
                });
            }
            // Substring match requires longer anchor (>= 6 chars) to prevent false positives.
            if clean_lower.len() >= 6 && cand_lower.contains(&clean_lower) {
                return Some(MatchHit {
                    confidence: ConfidenceLevel::Medium,
                    reason: format!("Name contains '{}'", self.clean_name),
                    reason_code: "contains_app_name",
                    reason_param: Some(self.clean_name.clone()),
                });
            }
            if self.compact_name.len() >= 6 && cand_compact.contains(&self.compact_name) {
                return Some(MatchHit {
                    confidence: ConfidenceLevel::Medium,
                    reason: format!("Normalized name contains '{}'", self.compact_name),
                    reason_code: "contains_app_name",
                    reason_param: Some(self.compact_name.clone()),
                });
            }
        }

        None
    }

    pub fn publisher_matches_folder(&self, folder_name: &str) -> bool {
        if self.publisher.len() < 3 {
            return false;
        }
        let pub_clean = strip_company_suffix(&self.publisher).to_lowercase();
        let f_lower = folder_name.trim().to_lowercase();
        if pub_clean.len() < 3 || f_lower.len() < 3 {
            return false;
        }
        f_lower == pub_clean || (pub_clean.len() >= 5 && f_lower.contains(&pub_clean))
    }
}

/// Strip version suffixes and architecture noise from display names, e.g.:
/// "7-Zip 24.08 (x64 edition)" -> "7-Zip"
pub fn strip_version_and_noise(name: &str) -> String {
    let mut s = name.to_string();
    while let (Some(start), Some(end)) = (s.find('('), s.find(')')) {
        if end > start {
            s.replace_range(start..=end, " ");
        } else {
            break;
        }
    }

    let tokens: Vec<&str> = s
        .split_whitespace()
        .filter(|tok| {
            let lower = tok.to_lowercase();
            if matches!(
                lower.as_str(),
                "x64" | "x86" | "arm64" | "64-bit" | "32-bit" | "win64" | "win32" | "edition" | "setup"
            ) {
                return false;
            }
            let stripped = lower.trim_start_matches('v');
            let is_ver = !stripped.is_empty()
                && stripped.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-')
                && stripped.contains('.');
            !is_ver
        })
        .collect();

    let joined = tokens.join(" ").trim().to_string();
    if joined.is_empty() {
        name.trim().to_string()
    } else {
        joined
    }
}

fn strip_company_suffix(pub_name: &str) -> String {
    let cleaned = pub_name.replace([',', '.'], " ");
    let tokens: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|t| {
            !matches!(
                t.to_lowercase().as_str(),
                "inc" | "llc" | "ltd" | "corp" | "corporation" | "co" | "gmbh" | "limited" | "sa" | "ag" | "bv"
            )
        })
        .collect();
    tokens.join(" ")
}

#[allow(clippy::too_many_arguments)]
fn make_item(
    kind: LeftoverKind,
    path: String,
    value_name: Option<String>,
    size_bytes: u64,
    confidence: ConfidenceLevel,
    reason: String,
    reason_code: &str,
    reason_param: Option<String>,
    selected_by_default: bool,
    requires_elevation: bool,
) -> LeftoverItem {
    LeftoverItem {
        id: Uuid::new_v4().to_string(),
        kind,
        path,
        value_name,
        size_bytes,
        confidence,
        reason,
        reason_code: reason_code.to_string(),
        reason_param,
        selected_by_default,
        requires_elevation,
    }
}

/// Run a multi-threaded deep leftover scan for an installed (or recently uninstalled) app.
pub fn scan_leftovers(app: &InstalledApp, trace: Option<&InstallTrace>) -> Vec<LeftoverItem> {
    let anchors = SearchAnchors::from_app(app);

    let ((fs_items, reg_items), task_items) = rayon::join(
        || {
            rayon::join(
                || scan_filesystem_leftovers(&anchors),
                || scan_registry_leftovers(&anchors),
            )
        },
        || scan_scheduled_task_leftovers(&anchors),
    );

    let mut combined = Vec::with_capacity(fs_items.len() + reg_items.len() + task_items.len() + 16);
    combined.extend(reg_items);
    combined.extend(fs_items);
    combined.extend(task_items);

    // Merge recorded Install Tracker trace items (if any still exist on disk/registry/SCM/tasks).
    if let Some(tr) = trace {
        append_trace_leftovers(tr, &mut combined);
    }

    // Deduplicate by (kind, normalized path, value_name).
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(combined.len());
    for item in combined {
        let key = format!(
            "{:?}|{}|{}",
            item.kind,
            item.path.to_lowercase(),
            item.value_name.as_deref().unwrap_or("").to_lowercase()
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }

    // Sort High confidence first, then path alphabetically.
    deduped.sort_by(|a, b| {
        let conf_ord = |c: ConfidenceLevel| match c {
            ConfidenceLevel::High => 0,
            ConfidenceLevel::Medium => 1,
            ConfidenceLevel::Protected => 2,
        };
        conf_ord(a.confidence)
            .cmp(&conf_ord(b.confidence))
            .then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase()))
    });

    deduped
}

/// Build a synthetic InstalledApp target from any dropped file, `.lnk` shortcut, or folder path
/// for Force Removal.
pub fn build_force_target_from_path(raw_path: &Path) -> InstalledApp {
    let resolved = if raw_path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("lnk"))
        .unwrap_or(false)
    {
        resolve_shortcut_target(raw_path).unwrap_or_else(|| raw_path.to_path_buf())
    } else {
        raw_path.to_path_buf()
    };

    let (install_dir, display_name) = if resolved.is_dir() {
        let name = resolved
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown App")
            .to_string();
        (resolved.to_string_lossy().to_string(), name)
    } else {
        let stem = resolved
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown App")
            .to_string();
        let dir = resolved
            .parent()
            .filter(|p| !is_protected_path(p))
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let dir_name = Path::new(&dir)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !is_generic_token(s))
            .map(|s| s.to_string())
            .unwrap_or(stem);
        (dir, dir_name)
    };

    InstalledApp {
        id: format!("force:{}", Uuid::new_v4()),
        name: display_name,
        publisher: String::new(),
        version: String::new(),
        install_date: String::new(),
        estimated_size_kb: 0,
        install_location: install_dir,
        uninstall_string: String::new(),
        quiet_uninstall_string: String::new(),
        display_icon: resolved.to_string_lossy().to_string(),
        url_info_about: String::new(),
        registry_key: String::new(),
        registry_hive: String::new(),
        registry_view: String::new(),
        product_code: String::new(),
        package_full_name: String::new(),
        package_family_name: String::new(),
        kind: AppKind::Win32,
        scope: InstallScope::Machine,
        arch: Arch::X64,
        is_system_component: false,
        is_windows_update: false,
        is_valid_entry: true,
        has_uninstaller: false,
        tracked_trace_id: None,
        package_manager: PackageManagerKind::None,
        size_measured: false,
    }
}

fn scan_filesystem_leftovers(anchors: &SearchAnchors) -> Vec<LeftoverItem> {
    let mut items = Vec::new();

    // 1. Original install directory itself (if still present and safe).
    if let Some(dir) = &anchors.install_dir {
        if dir.exists() && !is_protected_path(dir) {
            let size = compute_dir_size_bounded(dir, 5000);
            let req_elev = path_likely_needs_elevation(dir);
            items.push(make_item(
                if dir.is_dir() {
                    LeftoverKind::Directory
                } else {
                    LeftoverKind::File
                },
                dir.to_string_lossy().to_string(),
                None,
                size,
                ConfidenceLevel::High,
                "Remaining installation directory".to_string(),
                "remaining_install_dir",
                None,
                true,
                req_elev,
            ));
        }
    }

    // 1b. UWP / Store App user data directory under %LOCALAPPDATA%\Packages\<PackageFamilyName>.
    if anchors.kind == AppKind::StoreApp {
        if let Some(pfn) = &anchors.package_family_name {
            if let Some(pkg_data) = uwp_package_data_dir(pfn) {
                if !is_protected_path(&pkg_data) {
                    let size = compute_dir_size_bounded(&pkg_data, 5000);
                    items.push(make_item(
                        LeftoverKind::Directory,
                        pkg_data.to_string_lossy().to_string(),
                        None,
                        size,
                        ConfidenceLevel::High,
                        format!("Microsoft Store app data folder ({})", pfn),
                        "uwp_package_data",
                        Some(pfn.clone()),
                        true,
                        false,
                    ));
                }
            }
        }
    }

    // 2. Standard app data & installation containers.
    let mut containers: Vec<PathBuf> = Vec::new();
    for var in ["APPDATA", "LOCALAPPDATA", "ProgramData", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(v) = std::env::var(var) {
            let p = PathBuf::from(v);
            if p.is_dir() {
                containers.push(p);
            }
        }
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        let local_low = PathBuf::from(&profile).join("AppData\\LocalLow");
        if local_low.is_dir() {
            containers.push(local_low);
        }
        let local_programs = PathBuf::from(&profile).join("AppData\\Local\\Programs");
        if local_programs.is_dir() {
            containers.push(local_programs);
        }
    }

    let scanned_containers: Vec<LeftoverItem> = containers
        .par_iter()
        .flat_map(|container| scan_fs_container(container, anchors))
        .collect();
    items.extend(scanned_containers);

    // 3. Shortcuts on Desktop & Start Menu.
    let mut shortcut_dirs = Vec::new();
    if let Ok(profile) = std::env::var("USERPROFILE") {
        shortcut_dirs.push(PathBuf::from(&profile).join("Desktop"));
        shortcut_dirs.push(
            PathBuf::from(&profile)
                .join("AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs"),
        );
    }
    if let Ok(pub_dir) = std::env::var("PUBLIC") {
        shortcut_dirs.push(PathBuf::from(pub_dir).join("Desktop"));
    }
    if let Ok(pd) = std::env::var("ProgramData") {
        shortcut_dirs
            .push(PathBuf::from(pd).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }

    for sc_dir in shortcut_dirs {
        if sc_dir.is_dir() {
            scan_shortcuts_dir(&sc_dir, anchors, &mut items);
        }
    }

    items
}

fn scan_fs_container(container: &Path, anchors: &SearchAnchors) -> Vec<LeftoverItem> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(container) else {
        return found;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() || is_protected_path(&path) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if let Some(inst_norm) = &anchors.install_dir_norm {
            if normalize_fs_path(&path) == *inst_norm {
                continue;
            }
        }

        if let Some(hit) = anchors.match_name(name) {
            let size = compute_dir_size_bounded(&path, 3000);
            found.push(make_item(
                LeftoverKind::Directory,
                path.to_string_lossy().to_string(),
                None,
                size,
                hit.confidence,
                hit.reason,
                hit.reason_code,
                hit.reason_param,
                hit.confidence == ConfidenceLevel::High,
                path_likely_needs_elevation(&path),
            ));
            continue;
        }

        // Check 2nd-level child inside a matching vendor/publisher directory.
        if anchors.publisher_matches_folder(name) {
            if let Ok(sub_entries) = fs::read_dir(&path) {
                for sub in sub_entries.flatten() {
                    let sub_path = sub.path();
                    if is_protected_path(&sub_path) {
                        continue;
                    }
                    let Some(sub_name) = sub_path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };
                    if let Some(hit) = anchors.match_name(sub_name) {
                        let size = if sub_path.is_dir() {
                            compute_dir_size_bounded(&sub_path, 3000)
                        } else {
                            sub.metadata().map(|m| m.len()).unwrap_or(0)
                        };
                        found.push(make_item(
                            if sub_path.is_dir() {
                                LeftoverKind::Directory
                            } else {
                                LeftoverKind::File
                            },
                            sub_path.to_string_lossy().to_string(),
                            None,
                            size,
                            hit.confidence,
                            format!("{} (under publisher folder)", hit.reason),
                            "publisher_subdir_match",
                            hit.reason_param,
                            hit.confidence == ConfidenceLevel::High,
                            path_likely_needs_elevation(&sub_path),
                        ));
                    }
                }
            }
        }
    }
    found
}

fn scan_shortcuts_dir(dir: &Path, anchors: &SearchAnchors, out: &mut Vec<LeftoverItem>) {
    for entry in walkdir::WalkDir::new(dir)
        .max_depth(3)
        .into_iter()
        .flatten()
    {
        let p = entry.path();
        if p == dir || is_protected_path(p) {
            continue;
        }

        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                if let Some(hit) = anchors.match_name(name) {
                    out.push(make_item(
                        LeftoverKind::Shortcut,
                        p.to_string_lossy().to_string(),
                        None,
                        compute_dir_size_bounded(p, 200),
                        hit.confidence,
                        format!("Start Menu folder: {}", hit.reason),
                        "start_menu_folder",
                        hit.reason_param,
                        hit.confidence == ConfidenceLevel::High,
                        path_likely_needs_elevation(p),
                    ));
                }
            }
            continue;
        }

        if !p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("lnk"))
            .unwrap_or(false)
        {
            continue;
        }

        let mut matched: Option<MatchHit> = None;
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
            matched = anchors.match_name(stem);
        }
        if matched.is_none() {
            if let (Some(inst_norm), Some(target)) =
                (&anchors.install_dir_norm, resolve_shortcut_target(p))
            {
                let t_norm = normalize_fs_path(&target);
                if t_norm.starts_with(inst_norm) {
                    matched = Some(MatchHit {
                        confidence: ConfidenceLevel::High,
                        reason: "Shortcut targets uninstalled application directory".to_string(),
                        reason_code: "shortcut_target_match",
                        reason_param: None,
                    });
                }
            }
        }

        if let Some(hit) = matched {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            out.push(make_item(
                LeftoverKind::Shortcut,
                p.to_string_lossy().to_string(),
                None,
                size,
                hit.confidence,
                hit.reason,
                hit.reason_code,
                hit.reason_param,
                hit.confidence == ConfidenceLevel::High,
                path_likely_needs_elevation(p),
            ));
        }
    }
}

fn scan_registry_leftovers(anchors: &SearchAnchors) -> Vec<LeftoverItem> {
    let mut items = Vec::new();

    // 1. Original Uninstall registry key if still lingering.
    if let Some(reg_path) = &anchors.registry_key {
        if let Some((hive, subkey)) = split_registry_path(reg_path) {
            if !is_protected_registry_key(&hive, &subkey) {
                if let Ok(root) = open_predef_hive(&hive) {
                    if root.open_subkey_with_flags(&subkey, KEY_READ).is_ok() {
                        items.push(make_item(
                            LeftoverKind::RegistryKey,
                            reg_path.clone(),
                            None,
                            0,
                            ConfidenceLevel::High,
                            "Remaining Uninstall registry entry".to_string(),
                            "remaining_uninstall_reg",
                            None,
                            true,
                            hive.eq_ignore_ascii_case("HKLM"),
                        ));
                    }
                }
            }
        }
    }

    // 2. Software trees (HKCU\Software, HKLM\SOFTWARE, HKLM\SOFTWARE\WOW6432Node).
    let sw_roots = [
        ("HKCU", HKEY_CURRENT_USER, "Software", KEY_READ | KEY_WOW64_64KEY, false),
        ("HKLM", HKEY_LOCAL_MACHINE, "SOFTWARE", KEY_READ | KEY_WOW64_64KEY, true),
        (
            "HKLM",
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node",
            KEY_READ | KEY_WOW64_64KEY,
            true,
        ),
    ];

    for (hive_name, hkey, base_sub, flags, needs_elev) in sw_roots {
        let root = RegKey::predef(hkey);
        let Ok(base) = root.open_subkey_with_flags(base_sub, flags) else {
            continue;
        };

        for sub_name in base.enum_keys().flatten() {
            let full_sub = format!("{}\\{}", base_sub, sub_name);
            if is_protected_registry_key(hive_name, &full_sub) {
                continue;
            }
            if let Some(hit) = anchors.match_name(&sub_name) {
                items.push(make_item(
                    LeftoverKind::RegistryKey,
                    format!("{}\\{}", hive_name, full_sub),
                    None,
                    0,
                    hit.confidence,
                    hit.reason,
                    hit.reason_code,
                    hit.reason_param,
                    hit.confidence == ConfidenceLevel::High,
                    needs_elev,
                ));
                continue;
            }

            if anchors.publisher_matches_folder(&sub_name) {
                if let Ok(vendor_key) = base.open_subkey_with_flags(&sub_name, flags) {
                    for child in vendor_key.enum_keys().flatten() {
                        let child_sub = format!("{}\\{}", full_sub, child);
                        if is_protected_registry_key(hive_name, &child_sub) {
                            continue;
                        }
                        if let Some(hit) = anchors.match_name(&child) {
                            items.push(make_item(
                                LeftoverKind::RegistryKey,
                                format!("{}\\{}", hive_name, child_sub),
                                None,
                                0,
                                hit.confidence,
                                format!("{} (under publisher key)", hit.reason),
                                "publisher_subkey_match",
                                hit.reason_param,
                                hit.confidence == ConfidenceLevel::High,
                                needs_elev,
                            ));
                        }
                    }
                }
            }
        }
    }

    // 3. Startup Run values (64/32-bit), App Paths, MuiCache, Services, and Shell Context Menus.
    if let Some(inst_norm) = &anchors.install_dir_norm {
        if inst_norm.len() >= 8 {
            scan_reg_values_referencing_path(
                "HKCU",
                HKEY_CURRENT_USER,
                "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                inst_norm,
                false,
                "Startup entry references uninstalled directory",
                "startup_run_ref",
                &mut items,
            );
            scan_reg_values_referencing_path(
                "HKLM",
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run",
                inst_norm,
                true,
                "Machine startup entry references uninstalled directory",
                "startup_run_ref",
                &mut items,
            );
            scan_reg_values_referencing_path(
                "HKLM",
                HKEY_LOCAL_MACHINE,
                "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Run",
                inst_norm,
                true,
                "32-bit machine startup entry references uninstalled directory",
                "startup_run_ref",
                &mut items,
            );
            scan_reg_values_referencing_path(
                "HKCU",
                HKEY_CURRENT_USER,
                "Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\Shell\\MuiCache",
                inst_norm,
                false,
                "MUI shell cache references uninstalled executable",
                "muicache_ref",
                &mut items,
            );
            scan_app_paths(inst_norm, &mut items);
            scan_services_referencing_path(inst_norm, &mut items);
            scan_shell_context_menus(inst_norm, &mut items);
        }
    }

    items
}

#[allow(clippy::too_many_arguments)]
fn scan_reg_values_referencing_path(
    hive_name: &str,
    hkey: winreg::HKEY,
    subkey: &str,
    inst_norm: &str,
    needs_elev: bool,
    reason: &str,
    reason_code: &str,
    out: &mut Vec<LeftoverItem>,
) {
    let root = RegKey::predef(hkey);
    let Ok(key) = root.open_subkey_with_flags(subkey, KEY_READ) else {
        return;
    };
    for (val_name, val) in key.enum_values().flatten() {
        let name_norm = val_name.replace('/', "\\").to_lowercase();
        let data_norm = val.to_string().replace('/', "\\").to_lowercase();
        if name_norm.contains(inst_norm) || data_norm.contains(inst_norm) {
            out.push(make_item(
                LeftoverKind::RegistryValue,
                format!("{}\\{}", hive_name, subkey),
                Some(val_name),
                0,
                ConfidenceLevel::High,
                reason.to_string(),
                reason_code,
                None,
                true,
                needs_elev,
            ));
        }
    }
}

fn scan_app_paths(inst_norm: &str, out: &mut Vec<LeftoverItem>) {
    let base_sub = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths";
    for (hive_name, hkey, needs_elev) in [
        ("HKCU", HKEY_CURRENT_USER, false),
        ("HKLM", HKEY_LOCAL_MACHINE, true),
    ] {
        let root = RegKey::predef(hkey);
        let Ok(key) = root.open_subkey_with_flags(base_sub, KEY_READ) else {
            continue;
        };
        for exe_key_name in key.enum_keys().flatten() {
            if let Ok(sub) = key.open_subkey_with_flags(&exe_key_name, KEY_READ) {
                let default_val: String = sub.get_value("").unwrap_or_default();
                let path_val: String = sub.get_value("Path").unwrap_or_default();
                let d_norm = default_val.replace('/', "\\").to_lowercase();
                let p_norm = path_val.replace('/', "\\").to_lowercase();
                if d_norm.contains(inst_norm) || p_norm.contains(inst_norm) {
                    out.push(make_item(
                        LeftoverKind::RegistryKey,
                        format!("{}\\{}\\{}", hive_name, base_sub, exe_key_name),
                        None,
                        0,
                        ConfidenceLevel::High,
                        "App Paths registration references uninstalled directory".to_string(),
                        "app_paths_ref",
                        None,
                        true,
                        needs_elev,
                    ));
                }
            }
        }
    }
}

fn scan_services_referencing_path(inst_norm: &str, out: &mut Vec<LeftoverItem>) {
    let base_sub = "SYSTEM\\CurrentControlSet\\Services";
    let root = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(key) = root.open_subkey_with_flags(base_sub, KEY_READ) else {
        return;
    };
    for svc_name in key.enum_keys().flatten() {
        if let Ok(sub) = key.open_subkey_with_flags(&svc_name, KEY_READ) {
            let img: String = sub.get_value("ImagePath").unwrap_or_default();
            let norm = img.replace('/', "\\").to_lowercase();
            if !norm.is_empty() && norm.contains(inst_norm) {
                out.push(make_item(
                    LeftoverKind::Service,
                    format!("HKLM\\{}\\{}", base_sub, svc_name),
                    None,
                    0,
                    ConfidenceLevel::Medium,
                    format!("Windows Service '{}' references application folder", svc_name),
                    "service_ref",
                    Some(svc_name),
                    false,
                    true,
                ));
            }
        }
    }
}

/// Scan Explorer shell context menu verbs & `Applications\<exe>` file associations under
/// HKCU\Software\Classes and HKLM\SOFTWARE\Classes referencing `inst_norm`.
fn scan_shell_context_menus(inst_norm: &str, out: &mut Vec<LeftoverItem>) {
    let shell_parents = [
        "Software\\Classes\\*\\shell",
        "Software\\Classes\\Directory\\shell",
        "Software\\Classes\\Directory\\Background\\shell",
    ];
    for (hive_name, hkey, needs_elev) in [
        ("HKCU", HKEY_CURRENT_USER, false),
        ("HKLM", HKEY_LOCAL_MACHINE, true),
    ] {
        let root = RegKey::predef(hkey);
        for parent_sub in shell_parents {
            let Ok(parent_key) = root.open_subkey_with_flags(parent_sub, KEY_READ) else {
                continue;
            };
            for verb in parent_key.enum_keys().flatten() {
                if verb.starts_with("Unbind") {
                    continue;
                }
                let Ok(verb_key) = parent_key.open_subkey_with_flags(&verb, KEY_READ) else {
                    continue;
                };
                let icon: String = verb_key.get_value("Icon").unwrap_or_default();
                let cmd: String = verb_key
                    .open_subkey_with_flags("command", KEY_READ)
                    .ok()
                    .and_then(|c| c.get_value("").ok())
                    .unwrap_or_default();

                let icon_norm = icon.replace('/', "\\").to_lowercase();
                let cmd_norm = cmd.replace('/', "\\").to_lowercase();
                if cmd_norm.contains(inst_norm) || icon_norm.contains(inst_norm) {
                    let full_sub = format!("{}\\{}", parent_sub, verb);
                    if !is_protected_registry_key(hive_name, &full_sub) {
                        out.push(make_item(
                            LeftoverKind::RegistryKey,
                            format!("{}\\{}", hive_name, full_sub),
                            None,
                            0,
                            ConfidenceLevel::High,
                            format!("Explorer context menu '{}' references application", verb),
                            "shell_context_menu",
                            Some(verb),
                            true,
                            needs_elev,
                        ));
                    }
                }
            }
        }

        // Check Software\Classes\Applications\<exe>\shell\open\command
        let apps_sub = "Software\\Classes\\Applications";
        if let Ok(apps_key) = root.open_subkey_with_flags(apps_sub, KEY_READ) {
            for exe_key in apps_key.enum_keys().flatten() {
                let cmd_path = format!("{}\\shell\\open\\command", exe_key);
                let cmd: String = apps_key
                    .open_subkey_with_flags(&cmd_path, KEY_READ)
                    .ok()
                    .and_then(|k| k.get_value("").ok())
                    .unwrap_or_default();
                if cmd.replace('/', "\\").to_lowercase().contains(inst_norm) {
                    let full_sub = format!("{}\\{}", apps_sub, exe_key);
                    if !is_protected_registry_key(hive_name, &full_sub) {
                        out.push(make_item(
                            LeftoverKind::RegistryKey,
                            format!("{}\\{}", hive_name, full_sub),
                            None,
                            0,
                            ConfidenceLevel::High,
                            format!("File association '{}' references application", exe_key),
                            "file_association_ref",
                            Some(exe_key),
                            true,
                            needs_elev,
                        ));
                    }
                }
            }
        }
    }
}

/// Scan third-party Windows Scheduled Tasks whose command targets the application directory
/// or whose task name matches the application anchor.
fn scan_scheduled_task_leftovers(anchors: &SearchAnchors) -> Vec<LeftoverItem> {
    let mut items = Vec::new();
    let tasks = enumerate_third_party_scheduled_tasks();
    for task in tasks {
        let cmd_norm = task.command.replace('/', "\\").to_lowercase();
        let exe_norm = task.executable_path.replace('/', "\\").to_lowercase();

        if let Some(inst_norm) = &anchors.install_dir_norm {
            if inst_norm.len() >= 8
                && (cmd_norm.contains(inst_norm) || exe_norm.contains(inst_norm))
            {
                items.push(make_item(
                    LeftoverKind::ScheduledTask,
                    task.task_path,
                    None,
                    0,
                    ConfidenceLevel::High,
                    format!("Scheduled task '{}' targets application directory", task.name),
                    "scheduled_task_ref",
                    Some(task.name),
                    true,
                    true,
                ));
                continue;
            }
        }

        if let Some(hit) = anchors.match_name(&task.name) {
            items.push(make_item(
                LeftoverKind::ScheduledTask,
                task.task_path,
                None,
                0,
                ConfidenceLevel::Medium,
                format!("Scheduled task name matches ({})", hit.reason),
                "scheduled_task_name",
                Some(task.name),
                false,
                true,
            ));
        }
    }
    items
}

fn append_trace_leftovers(trace: &InstallTrace, out: &mut Vec<LeftoverItem>) {
    for dir_str in &trace.added_dirs {
        let p = Path::new(dir_str);
        if p.exists() && !is_protected_path(p) {
            out.push(make_item(
                LeftoverKind::Directory,
                dir_str.clone(),
                None,
                compute_dir_size_bounded(p, 2000),
                ConfidenceLevel::High,
                "Captured by Install Tracker".to_string(),
                "tracked_install",
                None,
                true,
                path_likely_needs_elevation(p),
            ));
        }
    }
    for file_str in &trace.added_files {
        let p = Path::new(file_str);
        if p.exists() && p.is_file() && !is_protected_path(p) {
            let sz = fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            out.push(make_item(
                LeftoverKind::File,
                file_str.clone(),
                None,
                sz,
                ConfidenceLevel::High,
                "Captured by Install Tracker".to_string(),
                "tracked_install",
                None,
                true,
                path_likely_needs_elevation(p),
            ));
        }
    }
    for reg_key in &trace.added_registry_keys {
        if let Some((hive, sub)) = split_registry_path(reg_key) {
            if !is_protected_registry_key(&hive, &sub) {
                if let Ok(root) = open_predef_hive(&hive) {
                    if root.open_subkey_with_flags(&sub, KEY_READ).is_ok() {
                        out.push(make_item(
                            LeftoverKind::RegistryKey,
                            reg_key.clone(),
                            None,
                            0,
                            ConfidenceLevel::High,
                            "Captured by Install Tracker".to_string(),
                            "tracked_install",
                            None,
                            true,
                            hive.eq_ignore_ascii_case("HKLM"),
                        ));
                    }
                }
            }
        }
    }
    for reg_val in &trace.added_registry_values {
        if let Some((hive, sub)) = split_registry_path(&reg_val.key_path) {
            if let Ok(root) = open_predef_hive(&hive) {
                if let Ok(k) = root.open_subkey_with_flags(&sub, KEY_READ) {
                    if k.get_raw_value(&reg_val.value_name).is_ok() {
                        out.push(make_item(
                            LeftoverKind::RegistryValue,
                            reg_val.key_path.clone(),
                            Some(reg_val.value_name.clone()),
                            0,
                            ConfidenceLevel::High,
                            "Captured by Install Tracker".to_string(),
                            "tracked_install",
                            None,
                            true,
                            hive.eq_ignore_ascii_case("HKLM"),
                        ));
                    }
                }
            }
        }
    }
    for svc_path in &trace.added_services {
        if let Some((hive, sub)) = split_registry_path(svc_path) {
            if let Ok(root) = open_predef_hive(&hive) {
                if root.open_subkey_with_flags(&sub, KEY_READ).is_ok() {
                    out.push(make_item(
                        LeftoverKind::Service,
                        svc_path.clone(),
                        None,
                        0,
                        ConfidenceLevel::High,
                        "Service captured by Install Tracker".to_string(),
                        "tracked_service",
                        None,
                        true,
                        true,
                    ));
                }
            }
        }
    }
    for task_path in &trace.added_scheduled_tasks {
        out.push(make_item(
            LeftoverKind::ScheduledTask,
            task_path.clone(),
            None,
            0,
            ConfidenceLevel::High,
            "Scheduled task captured by Install Tracker".to_string(),
            "tracked_task",
            None,
            true,
            true,
        ));
    }
}

pub fn compute_dir_size_bounded(dir: &Path, max_files: usize) -> u64 {
    if dir.is_file() {
        return fs::metadata(dir).map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    let mut seen = 0usize;
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if entry.file_type().is_file() {
            total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            seen += 1;
            if seen >= max_files {
                break;
            }
        }
    }
    total
}

pub fn path_likely_needs_elevation(path: &Path) -> bool {
    let norm = normalize_fs_path(path);
    norm.contains("\\program files\\")
        || norm.contains("\\program files (x86)\\")
        || norm.contains("\\programdata\\")
        || norm.contains("\\users\\public\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_version_and_noise() {
        assert_eq!(strip_version_and_noise("7-Zip 24.08 (x64 edition)"), "7-Zip");
        assert_eq!(
            strip_version_and_noise("Notepad++ (64-bit x64) v8.6.9"),
            "Notepad++"
        );
        assert_eq!(
            strip_version_and_noise("Visual Studio Code (User)"),
            "Visual Studio Code"
        );
    }

    #[test]
    fn test_anchor_matching_rejects_generic_names() {
        let app = InstalledApp {
            id: "test".into(),
            name: "Obsidian 1.7.4".into(),
            publisher: "Dynalist Inc.".into(),
            version: "1.7.4".into(),
            install_date: String::new(),
            estimated_size_kb: 0,
            install_location: "C:\\Users\\Test\\AppData\\Local\\Programs\\Obsidian".into(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            display_icon: String::new(),
            url_info_about: String::new(),
            registry_key: String::new(),
            registry_hive: String::new(),
            registry_view: String::new(),
            product_code: String::new(),
            package_full_name: String::new(),
            package_family_name: String::new(),
            kind: AppKind::Win32,
            scope: InstallScope::User,
            arch: Arch::X64,
            is_system_component: false,
            is_windows_update: false,
            is_valid_entry: true,
            has_uninstaller: true,
            tracked_trace_id: None,
            package_manager: PackageManagerKind::None,
            size_measured: false,
        };
        let anchors = SearchAnchors::from_app(&app);
        let hit = anchors.match_name("Obsidian").unwrap();
        assert_eq!(hit.reason_code, "exact_install_folder");
        assert!(anchors.match_name("Microsoft").is_none());
        assert!(anchors.match_name("Windows").is_none());
        assert!(anchors.match_name("Temp").is_none());
    }
}
