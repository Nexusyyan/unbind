use crate::models::{AppKind, InstalledApp};
use crate::safety::is_protected_path;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

const MAX_FILES_PER_APP: usize = 25_000;
const BUDGET_MS_PER_APP: u64 = 1500;
const BATCH_CHUNK_SIZE: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeCacheEntry {
    pub version: String,
    pub dir_mtime_secs: u64,
    pub size_kb: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeCache {
    pub entries: HashMap<String, SizeCacheEntry>,
}

impl SizeCache {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = fs::write(path, json);
        }
    }
}

/// Resolve the `%LOCALAPPDATA%\Packages\<PackageFamilyName>` data folder for a UWP/Store app.
pub fn uwp_package_data_dir(package_family_name: &str) -> Option<PathBuf> {
    let pfn = package_family_name.trim();
    if pfn.is_empty() {
        return None;
    }
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let p = PathBuf::from(local).join("Packages").join(pfn);
    if p.is_dir() {
        Some(p)
    } else {
        None
    }
}

fn dir_mtime_secs(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn app_primary_mtime(app: &InstalledApp) -> u64 {
    let mut best = 0u64;
    if !app.install_location.trim().is_empty() {
        best = best.max(dir_mtime_secs(Path::new(&app.install_location)));
    }
    if app.kind == AppKind::StoreApp {
        if let Some(data_dir) = uwp_package_data_dir(&app.package_family_name) {
            best = best.max(dir_mtime_secs(&data_dir));
        }
    }
    best
}

fn is_reparse_point(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

/// Safely measure directory bytes while skipping reparse points/symlinks and enforcing
/// file count and time budgets so large folders never stall discovery.
pub fn measure_dir_bytes_safe(dir: &Path, max_files: usize, budget_ms: u64) -> u64 {
    if !dir.exists() {
        return 0;
    }
    if dir.is_file() {
        return fs::metadata(dir).map(|m| m.len()).unwrap_or(0);
    }

    let deadline = Instant::now() + Duration::from_millis(budget_ms);
    let mut total = 0u64;
    let mut count = 0usize;
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        if Instant::now() >= deadline || count >= max_files {
            break;
        }
        let Ok(read_dir) = fs::read_dir(&current) else {
            continue;
        };
        for entry in read_dir.flatten() {
            if count >= max_files {
                break;
            }
            let Ok(meta) = entry.metadata() else {
                continue;
            };
            if is_reparse_point(&meta) {
                continue;
            }
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total = total.saturating_add(meta.len());
                count += 1;
            }
        }
    }
    total
}

/// Measure real disk footprint in KB for an application (Win32 install dir or StoreApp package + user data).
pub fn measure_app_size_kb(app: &InstalledApp) -> Option<(u64, u64)> {
    let mut total_bytes = 0u64;
    let mut measured_any = false;

    if !app.install_location.trim().is_empty() {
        let dir = Path::new(&app.install_location);
        let allowed = if app.kind == AppKind::StoreApp {
            dir.is_dir()
        } else {
            dir.is_dir() && !is_protected_path(dir)
        };
        if allowed {
            let bytes = measure_dir_bytes_safe(dir, MAX_FILES_PER_APP, BUDGET_MS_PER_APP);
            if bytes > 0 {
                total_bytes = total_bytes.saturating_add(bytes);
                measured_any = true;
            }
        }
    }

    if app.kind == AppKind::StoreApp {
        if let Some(data_dir) = uwp_package_data_dir(&app.package_family_name) {
            let bytes = measure_dir_bytes_safe(&data_dir, MAX_FILES_PER_APP / 2, BUDGET_MS_PER_APP / 2);
            if bytes > 0 {
                total_bytes = total_bytes.saturating_add(bytes);
                measured_any = true;
            }
        }
    }

    if measured_any && total_bytes > 0 {
        let kb = total_bytes.div_ceil(1024);
        Some((kb, app_primary_mtime(app)))
    } else {
        None
    }
}

/// Apply warm-cache sizes immediately during initial app discovery so repeated launches have
/// instant real size values without UI flicker.
pub fn apply_cached_sizes(apps: &mut [InstalledApp], cache_path: &Path) {
    let cache = SizeCache::load(cache_path);
    if cache.entries.is_empty() {
        return;
    }

    for app in apps.iter_mut() {
        if app.estimated_size_kb > 0 && app.kind != AppKind::StoreApp {
            continue;
        }
        if let Some(entry) = cache.entries.get(&app.id) {
            if entry.version == app.version && entry.size_kb > 0 {
                let cur_mtime = app_primary_mtime(app);
                if cur_mtime == 0 || cur_mtime == entry.dir_mtime_secs {
                    app.estimated_size_kb = entry.size_kb;
                    app.size_measured = true;
                }
            }
        }
    }
}

/// Compute missing sizes in parallel batches, streaming each batch via `on_batch` and persisting
/// the updated disk cache upon completion.
pub fn resolve_missing_sizes_batched<C, F>(
    apps: &[InstalledApp],
    cache_path: &Path,
    should_cancel: C,
    mut on_batch: F,
) where
    C: Fn() -> bool + Sync,
    F: FnMut(HashMap<String, u64>),
{
    let mut cache = SizeCache::load(cache_path);
    let targets: Vec<&InstalledApp> = apps
        .iter()
        .filter(|a| {
            let missing = a.estimated_size_kb == 0;
            let has_path = !a.install_location.trim().is_empty()
                || (a.kind == AppKind::StoreApp && !a.package_family_name.trim().is_empty());
            missing && has_path
        })
        .collect();

    if targets.is_empty() {
        return;
    }

    let mut dirty = false;
    for chunk in targets.chunks(BATCH_CHUNK_SIZE) {
        if should_cancel() {
            break;
        }

        let measured: Vec<(String, String, u64, u64)> = chunk
            .par_iter()
            .filter_map(|app| {
                if should_cancel() {
                    return None;
                }
                let (kb, mtime) = measure_app_size_kb(app)?;
                Some((app.id.clone(), app.version.clone(), kb, mtime))
            })
            .collect();

        if measured.is_empty() {
            continue;
        }

        let mut batch_map = HashMap::with_capacity(measured.len());
        for (id, version, kb, mtime) in measured {
            batch_map.insert(id.clone(), kb);
            cache.entries.insert(
                id,
                SizeCacheEntry {
                    version,
                    dir_mtime_secs: mtime,
                    size_kb: kb,
                },
            );
            dirty = true;
        }

        on_batch(batch_map);
    }

    if dirty {
        cache.save(cache_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Arch, InstallScope, PackageManagerKind};
    use tempfile::tempdir;

    #[test]
    fn test_measure_dir_bytes_and_cache_roundtrip() {
        let dir = tempdir().unwrap();
        let app_dir = dir.path().join("MyApp");
        fs::create_dir_all(app_dir.join("bin")).unwrap();
        fs::write(app_dir.join("bin").join("app.exe"), vec![0u8; 4096]).unwrap();
        fs::write(app_dir.join("data.bin"), vec![1u8; 2048]).unwrap();

        let bytes = measure_dir_bytes_safe(&app_dir, 1000, 1000);
        assert_eq!(bytes, 6144);

        let cache_file = dir.path().join("size-cache.json");
        let mut apps = vec![InstalledApp {
            id: "win32:hkcu:64:MyApp".into(),
            name: "MyApp".into(),
            publisher: "Acme".into(),
            version: "1.0.0".into(),
            install_date: "2026-11-25".into(),
            estimated_size_kb: 0,
            install_location: app_dir.to_string_lossy().to_string(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            display_icon: String::new(),
            url_info_about: String::new(),
            registry_key: String::new(),
            registry_hive: "HKCU".into(),
            registry_view: "64".into(),
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
        }];

        let mut streamed = HashMap::new();
        resolve_missing_sizes_batched(&apps, &cache_file, || false, |batch| {
            streamed.extend(batch);
        });
        assert_eq!(streamed.get("win32:hkcu:64:MyApp").copied(), Some(6));
        assert!(cache_file.exists());

        // Warm-start cache hit
        apply_cached_sizes(&mut apps, &cache_file);
        assert_eq!(apps[0].estimated_size_kb, 6);
        assert!(apps[0].size_measured);

        // Version change invalidates cache entry
        apps[0].version = "2.0.0".into();
        apps[0].estimated_size_kb = 0;
        apps[0].size_measured = false;
        apply_cached_sizes(&mut apps, &cache_file);
        assert_eq!(apps[0].estimated_size_kb, 0);
        assert!(!apps[0].size_measured);
    }
}
