use crate::models::{
    CoreError, CoreResult, PendingUpdateOutcome, StagedUpdateInfo, UpdateCheckResult,
};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const GITHUB_REPO: &str = "Nexusyyan/unbind";
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/Nexusyyan/unbind/releases/latest";
pub const RELEASES_FALLBACK_URL: &str = "https://github.com/Nexusyyan/unbind/releases/latest";
pub const ALLOWED_RELEASE_URL_PREFIX: &str = "https://github.com/Nexusyyan/unbind";
pub const UPDATE_CANCELLED_CODE: &str = "UPDATE_CANCELLED";

const MAX_JSON_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_CHECKSUMS_BYTES: usize = 256 * 1024;
const MAX_EXE_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;
const MIN_PE_EXE_BYTES: u64 = 128 * 1024;
const MAX_NOTES_CHARS: usize = 4000;
const MAX_REDIRECTS: usize = 6;
const STALE_TEMP_AGE_SECS: u64 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedSemver {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

fn parse_semver(raw: &str) -> Option<ParsedSemver> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    if trimmed.is_empty() {
        return None;
    }
    let without_build = trimmed.split('+').next()?.trim();
    let (core, prerelease) = match without_build.split_once('-') {
        Some((c, pre)) => (c.trim(), Some(pre.trim().to_string()).filter(|s| !s.is_empty())),
        None => (without_build, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(ParsedSemver {
        major,
        minor,
        patch,
        prerelease,
    })
}

fn compare_prerelease(a: &str, b: &str) -> Ordering {
    let mut ia = a.split('.');
    let mut ib = b.split('.');
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(pa), Some(pb)) => {
                let ord = match (pa.parse::<u64>(), pb.parse::<u64>()) {
                    (Ok(na), Ok(nb)) => na.cmp(&nb),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => pa.cmp(pb),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

fn compare_semver(a: &ParsedSemver, b: &ParsedSemver) -> Ordering {
    let core_ord = (a.major, a.minor, a.patch).cmp(&(b.major, b.minor, b.patch));
    if core_ord != Ordering::Equal {
        return core_ord;
    }
    match (&a.prerelease, &b.prerelease) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(pa), Some(pb)) => compare_prerelease(pa, pb),
    }
}

/// Parse a SemVer string like "v0.4.0" or "0.4.1-beta.1" into `(major, minor, patch)`.
pub fn parse_semver_tuple(raw: &str) -> Option<(u64, u64, u64)> {
    parse_semver(raw).map(|v| (v.major, v.minor, v.patch))
}

/// Returns true if `latest` is strictly greater than `current` (including prerelease -> stable).
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(cur), Some(lat)) => compare_semver(&lat, &cur) == Ordering::Greater,
        _ => false,
    }
}

/// Strictly verify that a release or asset URL belongs to the official GitHub repository.
pub fn is_safe_release_url(url: &str) -> bool {
    let trimmed = url.trim();
    if trimmed == ALLOWED_RELEASE_URL_PREFIX {
        return true;
    }
    let Some(rest) = trimmed.strip_prefix(ALLOWED_RELEASE_URL_PREFIX) else {
        return false;
    };
    rest.starts_with('/')
}

/// Verify that a redirected HTTPS host is an official GitHub or GitHub release-asset CDN host.
pub fn is_allowed_download_host(host: &str) -> bool {
    let lower = host.trim().to_ascii_lowercase();
    lower == "api.github.com"
        || lower == "github.com"
        || lower == "objects.githubusercontent.com"
        || lower == "release-assets.githubusercontent.com"
        || lower.ends_with(".githubusercontent.com")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHttpsUrl {
    pub host: String,
    pub port: u16,
    pub path_and_query: String,
}

/// Parse an HTTPS URL into host, port, and path+query without external URL crates.
pub fn parse_https_url(url: &str) -> CoreResult<ParsedHttpsUrl> {
    let trimmed = url.trim();
    let Some(after_scheme) = trimmed.strip_prefix("https://") else {
        return Err(CoreError::InvalidInput(
            "Only https:// URLs are allowed for updates".to_string(),
        ));
    };

    let (authority, path_and_query) = match after_scheme.find('/') {
        Some(idx) => (&after_scheme[..idx], after_scheme[idx..].to_string()),
        None => (after_scheme, "/".to_string()),
    };

    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return Err(CoreError::InvalidInput(
            "Invalid HTTPS authority in update URL".to_string(),
        ));
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p_str)) if !h.is_empty() && p_str.chars().all(|c| c.is_ascii_digit()) => {
            let port = p_str
                .parse::<u16>()
                .map_err(|_| CoreError::InvalidInput("Invalid port in update URL".to_string()))?;
            (h.to_string(), port)
        }
        _ => (authority.to_string(), 443),
    };

    if host.is_empty() {
        return Err(CoreError::InvalidInput(
            "Missing host in update URL".to_string(),
        ));
    }

    Ok(ParsedHttpsUrl {
        host,
        port,
        path_and_query,
    })
}

/// Parse a GitHub Releases `/latest` JSON payload into a validated `UpdateCheckResult`.
pub fn parse_github_release_json(
    current_version: &str,
    body: &str,
) -> CoreResult<UpdateCheckResult> {
    let parsed: serde_json::Value = serde_json::from_str(body)?;

    let raw_tag = parsed
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or(current_version)
        .trim()
        .to_string();
    let latest_clean = raw_tag.trim_start_matches(['v', 'V']).to_string();
    let latest_tag = if raw_tag.starts_with(['v', 'V']) {
        raw_tag
    } else if !latest_clean.is_empty() {
        format!("v{}", latest_clean)
    } else {
        format!("v{}", current_version.trim_start_matches(['v', 'V']))
    };

    let release_url = parsed
        .get("html_url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|u| is_safe_release_url(u))
        .unwrap_or(RELEASES_FALLBACK_URL)
        .to_string();

    let published_at = parsed
        .get("published_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let raw_notes = parsed
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let notes_preview: String = raw_notes.chars().take(MAX_NOTES_CHARS).collect();

    let mut exe_asset_name: Option<String> = None;
    let mut exe_asset_url: Option<String> = None;
    let mut exe_asset_size: Option<u64> = None;
    let mut checksums_url: Option<String> = None;

    if let Some(assets) = parsed.get("assets").and_then(|v| v.as_array()) {
        for asset in assets {
            let Some(name) = asset.get("name").and_then(|v| v.as_str()).map(str::trim) else {
                continue;
            };
            let Some(url) = asset
                .get("browser_download_url")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|u| is_safe_release_url(u))
            else {
                continue;
            };
            let size = asset.get("size").and_then(|v| v.as_u64());
            let lower = name.to_ascii_lowercase();

            if lower == "sha256sums.txt" {
                checksums_url = Some(url.to_string());
            } else if lower.ends_with(".exe")
                && lower.contains("unbind")
                && lower.contains("windows")
                && !lower.contains("setup")
                && !lower.contains("installer")
            {
                exe_asset_name = Some(name.to_string());
                exe_asset_url = Some(url.to_string());
                exe_asset_size = size;
            }
        }
    }

    let has_update = is_newer_version(current_version, &latest_clean);
    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: latest_clean,
        latest_tag,
        has_update,
        release_url,
        published_at,
        notes_preview,
        exe_asset_name,
        exe_asset_url,
        exe_asset_size,
        checksums_url,
    })
}

/// Parse a standard `SHA256SUMS.txt` file into a map of lowercase filename -> lowercase 64-hex SHA-256.
pub fn parse_sha256sums(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, |c: char| c.is_ascii_whitespace());
        let Some(hash_raw) = parts.next() else {
            continue;
        };
        let Some(name_raw) = parts.next() else {
            continue;
        };
        let hash = hash_raw.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let cleaned_name = name_raw
            .trim()
            .trim_start_matches('*')
            .trim()
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !cleaned_name.is_empty() {
            map.insert(cleaned_name, hash);
        }
    }
    map
}

/// Compute lowercase hex SHA-256 for a file on disk using streaming reads.
pub fn compute_file_sha256(path: &Path) -> CoreResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{:02x}", byte);
    }
    Ok(hex)
}

/// Verify that a staged executable has valid Windows PE (`MZ` + `PE\0\0`) headers and sane size.
pub fn verify_pe_executable(path: &Path) -> CoreResult<u64> {
    let meta = fs::metadata(path)?;
    let size = meta.len();
    if size < MIN_PE_EXE_BYTES {
        return Err(CoreError::SafetyBlocked(format!(
            "Downloaded executable is unexpectedly small ({} bytes)",
            size
        )));
    }
    if size > MAX_EXE_DOWNLOAD_BYTES {
        return Err(CoreError::SafetyBlocked(format!(
            "Downloaded executable exceeds size limit ({} bytes)",
            size
        )));
    }

    let mut file = File::open(path)?;
    let mut dos_header = [0u8; 64];
    file.read_exact(&mut dos_header)?;
    if &dos_header[0..2] != b"MZ" {
        return Err(CoreError::SafetyBlocked(
            "Downloaded file is not a valid Windows executable (missing MZ header)".to_string(),
        ));
    }

    let pe_offset = u32::from_le_bytes([
        dos_header[60],
        dos_header[61],
        dos_header[62],
        dos_header[63],
    ]) as u64;
    if pe_offset < 64 || pe_offset + 4 > size {
        return Err(CoreError::SafetyBlocked(
            "Downloaded executable has an invalid PE header offset".to_string(),
        ));
    }

    file.seek(SeekFrom::Start(pe_offset))?;
    let mut pe_sig = [0u8; 4];
    file.read_exact(&mut pe_sig)?;
    if &pe_sig != b"PE\0\0" {
        return Err(CoreError::SafetyBlocked(
            "Downloaded file is missing the PE signature".to_string(),
        ));
    }

    Ok(size)
}

/// Base directory inside `%TEMP%` used for update staging, helper copies, plans, and outcomes.
pub fn updater_temp_root() -> PathBuf {
    std::env::temp_dir().join("unbind-updater")
}

/// File written by the `--apply-update` helper and consumed once by the relaunched app.
pub fn update_outcome_path() -> PathBuf {
    updater_temp_root().join("last-outcome.json")
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn write_update_outcome(path: &Path, outcome: &PendingUpdateOutcome) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(outcome)?;
    fs::write(path, json)?;
    Ok(())
}

/// Read and delete the one-shot update outcome file if present.
pub fn consume_update_outcome() -> Option<PendingUpdateOutcome> {
    let path = update_outcome_path();
    let content = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    serde_json::from_str::<PendingUpdateOutcome>(&content).ok()
}

/// Remove leftover `.old` backups next to `current_exe` and stale staging files in `%TEMP%`.
pub fn cleanup_stale_update_artifacts(current_exe: &Path) {
    let backup = backup_path_for_exe(current_exe);
    if backup.exists() {
        let _ = fs::remove_file(&backup);
    }
    if let Some(parent) = current_exe.parent() {
        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(".unbind-update-new-") && name.ends_with(".tmp") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    let root = updater_temp_root();
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "last-outcome.json" {
            continue;
        }
        let is_ours = name.starts_with("stage-")
            || name.starts_with("helper-")
            || name.starts_with("plan-");
        if !is_ours {
            continue;
        }
        let old_enough = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|mtime| now.duration_since(mtime).ok())
            .map(|age| age.as_secs() >= STALE_TEMP_AGE_SECS)
            .unwrap_or(true);
        if old_enough {
            if path.is_dir() {
                let _ = fs::remove_dir_all(&path);
            } else {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

pub fn backup_path_for_exe(target_exe: &Path) -> PathBuf {
    let mut os = target_exe.as_os_str().to_os_string();
    os.push(".old");
    PathBuf::from(os)
}

/// Atomically replace `target_exe` with `staged_exe` after re-verifying SHA-256 and PE headers.
/// Copies onto the target volume first so cross-drive moves (e.g. `C:\Temp` -> `D:\Portable`)
/// always finish with an atomic same-volume rename, and automatically rolls back on failure.
pub fn apply_staged_binary_swap(
    staged_exe: &Path,
    target_exe: &Path,
    expected_sha256: &str,
) -> CoreResult<()> {
    if !staged_exe.exists() {
        return Err(CoreError::NotFound(format!(
            "Staged update executable not found: {}",
            staged_exe.display()
        )));
    }
    if !target_exe.exists() {
        return Err(CoreError::NotFound(format!(
            "Target executable not found: {}",
            target_exe.display()
        )));
    }

    verify_pe_executable(staged_exe)?;
    let actual_sha256 = compute_file_sha256(staged_exe)?;
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256.trim()) {
        return Err(CoreError::SafetyBlocked(format!(
            "SHA-256 mismatch before swap (expected {}, got {})",
            expected_sha256, actual_sha256
        )));
    }

    let target_dir = target_exe.parent().ok_or_else(|| {
        CoreError::InvalidInput("Target executable has no parent directory".to_string())
    })?;
    let temp_new = target_dir.join(format!(".unbind-update-new-{}.tmp", std::process::id()));
    let backup_exe = backup_path_for_exe(target_exe);

    // Stage onto the target directory volume first so the final swap is same-volume.
    let copy_result = (|| -> CoreResult<()> {
        fs::copy(staged_exe, &temp_new)?;
        let f = File::open(&temp_new)?;
        let _ = f.sync_all();
        Ok(())
    })();
    if let Err(e) = copy_result {
        let _ = fs::remove_file(&temp_new);
        return Err(e);
    }

    // Retry renaming the old binary briefly in case AV / Shell holds a transient handle.
    let mut moved_old = false;
    let mut last_err: Option<std::io::Error> = None;
    for _ in 0..24 {
        if backup_exe.exists() {
            let _ = fs::remove_file(&backup_exe);
        }
        match fs::rename(target_exe, &backup_exe) {
            Ok(()) => {
                moved_old = true;
                break;
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }

    if !moved_old {
        let _ = fs::remove_file(&temp_new);
        return Err(CoreError::Io(last_err.unwrap_or_else(|| {
            std::io::Error::other("Failed renaming running executable for update")
        })));
    }

    // Move the verified new executable into place; roll back immediately if this fails.
    if let Err(e) = fs::rename(&temp_new, target_exe) {
        let _ = fs::rename(&backup_exe, target_exe);
        let _ = fs::remove_file(&temp_new);
        return Err(CoreError::Io(e));
    }

    Ok(())
}

/// Check the latest published GitHub Release via Windows built-in WinHTTP.
#[cfg(windows)]
pub fn check_github_release(current_version: &str) -> CoreResult<UpdateCheckResult> {
    let bytes = winhttp_get_bytes(
        LATEST_RELEASE_API_URL,
        current_version,
        true,
        MAX_JSON_RESPONSE_BYTES,
    )?;
    let body = String::from_utf8(bytes)
        .map_err(|e| CoreError::WinApi(format!("Invalid UTF-8 from GitHub API: {}", e)))?;
    parse_github_release_json(current_version, &body)
}

#[cfg(not(windows))]
pub fn check_github_release(current_version: &str) -> CoreResult<UpdateCheckResult> {
    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: current_version.to_string(),
        latest_tag: format!("v{}", current_version.trim_start_matches(['v', 'V'])),
        has_update: false,
        release_url: RELEASES_FALLBACK_URL.to_string(),
        published_at: String::new(),
        notes_preview: String::new(),
        exe_asset_name: None,
        exe_asset_url: None,
        exe_asset_size: None,
        checksums_url: None,
    })
}

/// Download the standalone release executable, verify against `SHA256SUMS.txt` and PE headers,
/// and stage it into `%TEMP%\unbind-updater\stage-<version>-<id>`.
#[cfg(windows)]
pub fn download_and_stage_update<F>(
    check: &UpdateCheckResult,
    current_version: &str,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> CoreResult<StagedUpdateInfo>
where
    F: FnMut(u64, u64, bool),
{
    let asset_name = check
        .exe_asset_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| {
            CoreError::NotFound(
                "No standalone Windows executable asset found in this release".to_string(),
            )
        })?;
    let asset_url = check
        .exe_asset_url
        .as_deref()
        .filter(|u| is_safe_release_url(u))
        .ok_or_else(|| {
            CoreError::SafetyBlocked(
                "Release asset download URL is missing or untrusted".to_string(),
            )
        })?;
    let checksums_url = check
        .checksums_url
        .as_deref()
        .filter(|u| is_safe_release_url(u))
        .ok_or_else(|| {
            CoreError::SafetyBlocked(
                "Release SHA256SUMS.txt is missing; refusing unverified binary download"
                    .to_string(),
            )
        })?;

    if cancel_flag.load(AtomicOrdering::SeqCst) {
        return Err(CoreError::WinApi(UPDATE_CANCELLED_CODE.to_string()));
    }

    // 1. Fetch and parse SHA256SUMS.txt first so we fail fast if manifest lacks our asset.
    let sums_bytes = winhttp_get_bytes(checksums_url, current_version, false, MAX_CHECKSUMS_BYTES)?;
    let sums_text = String::from_utf8(sums_bytes)
        .map_err(|e| CoreError::WinApi(format!("Invalid UTF-8 in SHA256SUMS.txt: {}", e)))?;
    let sums_map = parse_sha256sums(&sums_text);
    let expected_sha256 = sums_map
        .get(&asset_name.to_ascii_lowercase())
        .cloned()
        .ok_or_else(|| {
            CoreError::SafetyBlocked(format!(
                "SHA256SUMS.txt does not contain a checksum for {}",
                asset_name
            ))
        })?;

    if cancel_flag.load(AtomicOrdering::SeqCst) {
        return Err(CoreError::WinApi(UPDATE_CANCELLED_CODE.to_string()));
    }

    // 2. Prepare clean temporary staging directory.
    let safe_ver: String = check
        .latest_version
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect();
    let stage_dir = updater_temp_root().join(format!(
        "stage-{}-{}",
        safe_ver,
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&stage_dir)?;

    let part_path = stage_dir.join("unbind-update.part");
    let final_exe_path = stage_dir.join("unbind.exe");

    // 3. Stream download with cancellation & progress reporting.
    let expected_total = check.exe_asset_size.unwrap_or(0);
    let download_res = winhttp_download_to_file(
        asset_url,
        current_version,
        &part_path,
        expected_total,
        cancel_flag,
        |downloaded, total| on_progress(downloaded, total, false),
    );
    let downloaded_bytes = match download_res {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(e);
        }
    };

    // 4. Transition to verifying phase (SHA-256 + PE headers).
    on_progress(downloaded_bytes, downloaded_bytes.max(expected_total), true);

    if cancel_flag.load(AtomicOrdering::SeqCst) {
        let _ = fs::remove_dir_all(&stage_dir);
        return Err(CoreError::WinApi(UPDATE_CANCELLED_CODE.to_string()));
    }

    let verify_res = (|| -> CoreResult<u64> {
        let size = verify_pe_executable(&part_path)?;
        let actual = compute_file_sha256(&part_path)?;
        if !actual.eq_ignore_ascii_case(&expected_sha256) {
            return Err(CoreError::SafetyBlocked(format!(
                "SHA-256 verification failed (expected {}, got {})",
                expected_sha256, actual
            )));
        }
        fs::rename(&part_path, &final_exe_path)?;
        Ok(size)
    })();

    let file_size = match verify_res {
        Ok(size) => size,
        Err(e) => {
            let _ = fs::remove_dir_all(&stage_dir);
            return Err(e);
        }
    };

    Ok(StagedUpdateInfo {
        version: check.latest_version.clone(),
        tag: check.latest_tag.clone(),
        asset_name: asset_name.to_string(),
        sha256: expected_sha256,
        file_size,
        staged_exe_path: final_exe_path.to_string_lossy().to_string(),
        staging_dir: stage_dir.to_string_lossy().to_string(),
    })
}

#[cfg(not(windows))]
pub fn download_and_stage_update<F>(
    _check: &UpdateCheckResult,
    _current_version: &str,
    _cancel_flag: &AtomicBool,
    _on_progress: F,
) -> CoreResult<StagedUpdateInfo>
where
    F: FnMut(u64, u64, bool),
{
    Err(CoreError::WinApi("In-app updates are only supported on Windows".to_string()))
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(windows)]
fn detect_env_http_proxy() -> Option<String> {
    for key in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
        let Ok(raw) = std::env::var(key) else {
            continue;
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("socks") {
            continue;
        }
        let stripped = trimmed
            .strip_prefix("https://")
            .or_else(|| trimmed.strip_prefix("http://"))
            .unwrap_or(trimmed)
            .trim_end_matches('/');
        // Skip proxies with embedded credentials or invalid whitespace.
        if stripped.is_empty() || stripped.contains('@') || stripped.chars().any(char::is_whitespace)
        {
            continue;
        }
        return Some(stripped.to_string());
    }
    None
}

#[cfg(windows)]
#[derive(Debug, Clone)]
enum ProxyMode {
    EnvNamed(String),
    Automatic,
    Direct,
}

#[cfg(windows)]
fn build_proxy_candidates() -> Vec<ProxyMode> {
    let mut modes = Vec::with_capacity(3);
    if let Some(proxy) = detect_env_http_proxy() {
        modes.push(ProxyMode::EnvNamed(proxy));
    }
    modes.push(ProxyMode::Automatic);
    modes.push(ProxyMode::Direct);
    modes
}

#[cfg(windows)]
fn query_header_string(request: *mut std::ffi::c_void, query_flag: u32) -> Option<String> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::WinHttpQueryHeaders;

    let mut byte_len: u32 = 0;
    unsafe {
        let _ = WinHttpQueryHeaders(
            request,
            query_flag,
            PCWSTR::null(),
            None,
            &mut byte_len,
            std::ptr::null_mut(),
        );
        if byte_len < 2 {
            return None;
        }
        let mut buf = vec![0u16; (byte_len as usize).div_ceil(2)];
        if WinHttpQueryHeaders(
            request,
            query_flag,
            PCWSTR::null(),
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            &mut byte_len,
            std::ptr::null_mut(),
        )
        .is_err()
        {
            return None;
        }
        let wide_len = (byte_len as usize) / 2;
        let slice = &buf[..wide_len.min(buf.len())];
        let trimmed = slice
            .iter()
            .position(|&c| c == 0)
            .map(|i| &slice[..i])
            .unwrap_or(slice);
        Some(String::from_utf16_lossy(trimmed).trim().to_string())
    }
}

#[cfg(windows)]
fn open_winhttp_session(app_version: &str, mode: &ProxyMode) -> CoreResult<*mut std::ffi::c_void> {
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::{
        WinHttpOpen, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NAMED_PROXY,
        WINHTTP_ACCESS_TYPE_NO_PROXY,
    };

    let ua = to_wide_null(&format!("Unbind/{}", app_version));
    let session = match mode {
        ProxyMode::EnvNamed(proxy) => {
            let proxy_w = to_wide_null(proxy);
            let bypass_w = to_wide_null("<local>");
            unsafe {
                WinHttpOpen(
                    PCWSTR(ua.as_ptr()),
                    WINHTTP_ACCESS_TYPE_NAMED_PROXY,
                    PCWSTR(proxy_w.as_ptr()),
                    PCWSTR(bypass_w.as_ptr()),
                    0,
                )
            }
        }
        ProxyMode::Automatic => unsafe {
            WinHttpOpen(
                PCWSTR(ua.as_ptr()),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        },
        ProxyMode::Direct => unsafe {
            WinHttpOpen(
                PCWSTR(ua.as_ptr()),
                WINHTTP_ACCESS_TYPE_NO_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        },
    };
    if session.is_null() {
        return Err(CoreError::WinApi("WinHttpOpen failed".to_string()));
    }
    Ok(session)
}

#[cfg(windows)]
enum StreamTarget<'a> {
    Memory {
        buf: &'a mut Vec<u8>,
        max_bytes: usize,
    },
    File {
        file: &'a mut File,
        max_bytes: u64,
        fallback_total: u64,
        cancel_flag: &'a AtomicBool,
        on_progress: &'a mut dyn FnMut(u64, u64),
    },
}

#[cfg(windows)]
fn winhttp_execute_stream(
    initial_url: &str,
    app_version: &str,
    accept_github_json: bool,
    proxy_mode: &ProxyMode,
    recv_timeout_ms: i32,
    mut target: StreamTarget<'_>,
) -> CoreResult<u64> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpenRequest,
        WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ADDREQ_FLAG_ADD,
        WINHTTP_DISABLE_REDIRECTS, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_DISABLE_FEATURE,
        WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_LOCATION,
        WINHTTP_QUERY_STATUS_CODE,
    };

    let session = open_winhttp_session(app_version, proxy_mode)?;
    unsafe {
        let _ = WinHttpSetTimeouts(session, 12_000, 15_000, 15_000, recv_timeout_ms);
    }

    let mut current_url = initial_url.to_string();
    let result = (|| -> CoreResult<u64> {
        for _hop in 0..=MAX_REDIRECTS {
            let parsed = parse_https_url(&current_url)?;
            if !is_allowed_download_host(&parsed.host) {
                return Err(CoreError::SafetyBlocked(format!(
                    "Blocked redirect to untrusted host: {}",
                    parsed.host
                )));
            }

            let host_w = to_wide_null(&parsed.host);
            let path_w = to_wide_null(&parsed.path_and_query);
            let method_w = to_wide_null("GET");

            let connect = unsafe { WinHttpConnect(session, PCWSTR(host_w.as_ptr()), parsed.port, 0) };
            if connect.is_null() {
                return Err(CoreError::WinApi(format!(
                    "WinHttpConnect failed for {}",
                    parsed.host
                )));
            }

            let request = unsafe {
                WinHttpOpenRequest(
                    connect,
                    PCWSTR(method_w.as_ptr()),
                    PCWSTR(path_w.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    WINHTTP_FLAG_SECURE,
                )
            };
            if request.is_null() {
                unsafe {
                    let _ = WinHttpCloseHandle(connect);
                }
                return Err(CoreError::WinApi("WinHttpOpenRequest failed".to_string()));
            }

            // Handle redirects explicitly so every hop is validated against our HTTPS host allowlist.
            let disable_bytes = WINHTTP_DISABLE_REDIRECTS.to_ne_bytes();
            unsafe {
                let _ = WinHttpSetOption(
                    Some(request),
                    WINHTTP_OPTION_DISABLE_FEATURE,
                    Some(&disable_bytes),
                );
            }

            if accept_github_json && parsed.host.eq_ignore_ascii_case("api.github.com") {
                let headers_w = to_wide_null("Accept: application/vnd.github+json\r\n");
                unsafe {
                    let _ = WinHttpAddRequestHeaders(
                        request,
                        &headers_w[..headers_w.len().saturating_sub(1)],
                        WINHTTP_ADDREQ_FLAG_ADD,
                    );
                }
            }

            let recv_ok = unsafe {
                WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok()
                    && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok()
            };
            if !recv_ok {
                unsafe {
                    let _ = WinHttpCloseHandle(request);
                    let _ = WinHttpCloseHandle(connect);
                }
                return Err(CoreError::WinApi(format!(
                    "Network request to {} failed or timed out",
                    parsed.host
                )));
            }

            let mut status_code: u32 = 0;
            let mut status_len = std::mem::size_of::<u32>() as u32;
            unsafe {
                let _ = WinHttpQueryHeaders(
                    request,
                    WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                    PCWSTR::null(),
                    Some(&mut status_code as *mut u32 as *mut c_void),
                    &mut status_len,
                    std::ptr::null_mut(),
                );
            }

            if matches!(status_code, 301 | 302 | 303 | 307 | 308) {
                let location = query_header_string(request, WINHTTP_QUERY_LOCATION);
                unsafe {
                    let _ = WinHttpCloseHandle(request);
                    let _ = WinHttpCloseHandle(connect);
                }
                let Some(next_loc) = location.filter(|s| !s.is_empty()) else {
                    return Err(CoreError::WinApi(format!(
                        "HTTP {} redirect missing Location header",
                        status_code
                    )));
                };
                if next_loc.starts_with('/') {
                    current_url = format!("https://{}{}", parsed.host, next_loc);
                } else {
                    current_url = next_loc;
                }
                continue;
            }

            if status_code >= 400 {
                unsafe {
                    let _ = WinHttpCloseHandle(request);
                    let _ = WinHttpCloseHandle(connect);
                }
                return Err(CoreError::WinApi(format!(
                    "GitHub returned HTTP {}",
                    status_code
                )));
            }

            let content_length = query_header_string(request, WINHTTP_QUERY_CONTENT_LENGTH)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            let read_res = (|| -> CoreResult<u64> {
                let mut total_read: u64 = 0;
                let mut last_emit = Instant::now();
                let mut chunk = vec![0u8; 64 * 1024];

                if let StreamTarget::File {
                    fallback_total,
                    on_progress,
                    ..
                } = &mut target
                {
                    let total_hint = if content_length > 0 {
                        content_length
                    } else {
                        *fallback_total
                    };
                    on_progress(0, total_hint);
                }

                loop {
                    if let StreamTarget::File { cancel_flag, .. } = &target {
                        if cancel_flag.load(AtomicOrdering::SeqCst) {
                            return Err(CoreError::WinApi(UPDATE_CANCELLED_CODE.to_string()));
                        }
                    }

                    let mut avail: u32 = 0;
                    if unsafe { WinHttpQueryDataAvailable(request, &mut avail) }.is_err() {
                        return Err(CoreError::WinApi(
                            "Connection interrupted while reading update stream".to_string(),
                        ));
                    }
                    if avail == 0 {
                        break;
                    }

                    let to_read = (avail as usize).min(chunk.len());
                    let mut got: u32 = 0;
                    if unsafe {
                        WinHttpReadData(
                            request,
                            chunk.as_mut_ptr() as *mut c_void,
                            to_read as u32,
                            &mut got,
                        )
                    }
                    .is_err()
                    {
                        return Err(CoreError::WinApi(
                            "Read failed while downloading update stream".to_string(),
                        ));
                    }
                    if got == 0 {
                        break;
                    }

                    let n = got as usize;
                    match &mut target {
                        StreamTarget::Memory { buf, max_bytes } => {
                            if buf.len().saturating_add(n) > *max_bytes {
                                return Err(CoreError::SafetyBlocked(
                                    "Remote response exceeded maximum buffer limit".to_string(),
                                ));
                            }
                            buf.extend_from_slice(&chunk[..n]);
                            total_read += n as u64;
                        }
                        StreamTarget::File {
                            file,
                            max_bytes,
                            fallback_total,
                            on_progress,
                            ..
                        } => {
                            if total_read.saturating_add(n as u64) > *max_bytes {
                                return Err(CoreError::SafetyBlocked(
                                    "Downloaded update exceeded maximum file size limit"
                                        .to_string(),
                                ));
                            }
                            file.write_all(&chunk[..n])?;
                            total_read += n as u64;

                            let total_hint = if content_length > 0 {
                                content_length
                            } else {
                                *fallback_total
                            };
                            if last_emit.elapsed() >= Duration::from_millis(80)
                                || (total_hint > 0 && total_read >= total_hint)
                            {
                                on_progress(total_read, total_hint);
                                last_emit = Instant::now();
                            }
                        }
                    }
                }

                if let StreamTarget::File {
                    file,
                    fallback_total,
                    on_progress,
                    ..
                } = &mut target
                {
                    file.flush()?;
                    let total_hint = if content_length > 0 {
                        content_length
                    } else {
                        (*fallback_total).max(total_read)
                    };
                    on_progress(total_read, total_hint);
                }

                Ok(total_read)
            })();

            unsafe {
                let _ = WinHttpCloseHandle(request);
                let _ = WinHttpCloseHandle(connect);
            }
            return read_res;
        }

        Err(CoreError::WinApi(
            "Too many HTTP redirects while fetching update".to_string(),
        ))
    })();

    unsafe {
        let _ = WinHttpCloseHandle(session);
    }
    result
}

#[cfg(windows)]
fn should_retry_with_next_proxy(err: &CoreError) -> bool {
    match err {
        CoreError::WinApi(msg) => {
            !msg.contains(UPDATE_CANCELLED_CODE) && !msg.starts_with("GitHub returned HTTP ")
        }
        _ => false,
    }
}

#[cfg(windows)]
fn winhttp_get_bytes(
    url: &str,
    app_version: &str,
    accept_github_json: bool,
    max_bytes: usize,
) -> CoreResult<Vec<u8>> {
    let mut last_err: Option<CoreError> = None;
    for mode in build_proxy_candidates() {
        let mut buf = Vec::new();
        match winhttp_execute_stream(
            url,
            app_version,
            accept_github_json,
            &mode,
            15_000,
            StreamTarget::Memory {
                buf: &mut buf,
                max_bytes,
            },
        ) {
            Ok(_) => return Ok(buf),
            Err(e) => {
                let retry = should_retry_with_next_proxy(&e);
                last_err = Some(e);
                if !retry {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| CoreError::WinApi("Network request failed".to_string())))
}

#[cfg(windows)]
fn winhttp_download_to_file<F>(
    url: &str,
    app_version: &str,
    dest_path: &Path,
    expected_total: u64,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> CoreResult<u64>
where
    F: FnMut(u64, u64),
{
    let mut last_err: Option<CoreError> = None;
    for mode in build_proxy_candidates() {
        if cancel_flag.load(AtomicOrdering::SeqCst) {
            return Err(CoreError::WinApi(UPDATE_CANCELLED_CODE.to_string()));
        }
        let mut file = File::create(dest_path)?;
        match winhttp_execute_stream(
            url,
            app_version,
            false,
            &mode,
            45_000,
            StreamTarget::File {
                file: &mut file,
                max_bytes: MAX_EXE_DOWNLOAD_BYTES,
                fallback_total: expected_total,
                cancel_flag,
                on_progress: &mut on_progress,
            },
        ) {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                drop(file);
                let _ = fs::remove_file(dest_path);
                let retry = should_retry_with_next_proxy(&e);
                last_err = Some(e);
                if !retry {
                    break;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| CoreError::WinApi("Download failed".to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_synthetic_pe(path: &Path, fill_byte: u8) -> String {
        let mut buf = vec![fill_byte; (MIN_PE_EXE_BYTES as usize) + 1024];
        buf[0] = b'M';
        buf[1] = b'Z';
        let pe_offset: u32 = 0x80;
        buf[60..64].copy_from_slice(&pe_offset.to_le_bytes());
        buf[0x80..0x84].copy_from_slice(b"PE\0\0");
        fs::write(path, &buf).unwrap();
        compute_file_sha256(path).unwrap()
    }

    #[test]
    fn test_semver_comparison_including_prerelease() {
        assert!(is_newer_version("0.3.0", "v0.4.0"));
        assert!(is_newer_version("0.4.0", "0.4.1"));
        assert!(is_newer_version("0.4.0", "1.0.0"));
        assert!(is_newer_version("0.6.0-beta.1", "0.6.0"));
        assert!(is_newer_version("0.6.0-beta.1", "0.6.0-beta.2"));
        assert!(!is_newer_version("0.6.0", "0.6.0-rc.1"));
        assert!(!is_newer_version("0.4.0", "v0.4.0"));
        assert!(!is_newer_version("0.4.0", "0.3.9"));
        assert!(!is_newer_version("0.4.0", "invalid"));
    }

    #[test]
    fn test_release_url_and_host_allowlist() {
        assert!(is_safe_release_url(
            "https://github.com/Nexusyyan/unbind/releases/tag/v0.6.0"
        ));
        assert!(is_safe_release_url(
            "https://github.com/Nexusyyan/unbind/releases/download/v0.6.0/unbind-v0.6.0-x86_64-windows.exe"
        ));
        assert!(!is_safe_release_url(
            "https://github.com/Nexusyyan/unbind-evil/releases/tag/v0.6.0"
        ));
        assert!(!is_safe_release_url("https://evil.example.com/unbind.exe"));
        assert!(!is_safe_release_url("http://github.com/Nexusyyan/unbind"));

        assert!(is_allowed_download_host("api.github.com"));
        assert!(is_allowed_download_host("github.com"));
        assert!(is_allowed_download_host("objects.githubusercontent.com"));
        assert!(is_allowed_download_host("release-assets.githubusercontent.com"));
        assert!(!is_allowed_download_host("evil-githubusercontent.com"));
        assert!(!is_allowed_download_host("example.com"));
    }

    #[test]
    fn test_parse_github_release_json_extracts_verified_assets() {
        let json = r####"{
            "tag_name": "v0.6.0",
            "html_url": "https://github.com/Nexusyyan/unbind/releases/tag/v0.6.0",
            "published_at": "2026-11-26T08:00:00Z",
            "body": "### Highlights\n- In-app self-updater",
            "assets": [
                {
                    "name": "unbind-v0.6.0-x86_64-windows-portable.zip",
                    "browser_download_url": "https://github.com/Nexusyyan/unbind/releases/download/v0.6.0/unbind-v0.6.0-x86_64-windows-portable.zip",
                    "size": 3100000
                },
                {
                    "name": "unbind-v0.6.0-x86_64-windows.exe",
                    "browser_download_url": "https://github.com/Nexusyyan/unbind/releases/download/v0.6.0/unbind-v0.6.0-x86_64-windows.exe",
                    "size": 7200000
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": "https://github.com/Nexusyyan/unbind/releases/download/v0.6.0/SHA256SUMS.txt",
                    "size": 256
                }
            ]
        }"####;

        let res = parse_github_release_json("0.5.0", json).unwrap();
        assert!(res.has_update);
        assert_eq!(res.latest_version, "0.6.0");
        assert_eq!(res.latest_tag, "v0.6.0");
        assert_eq!(
            res.exe_asset_name.as_deref(),
            Some("unbind-v0.6.0-x86_64-windows.exe")
        );
        assert_eq!(res.exe_asset_size, Some(7_200_000));
        assert!(res.checksums_url.is_some());
    }

    #[test]
    fn test_parse_github_release_json_rejects_untrusted_urls() {
        let json = r#"{
            "tag_name": "v0.6.0",
            "html_url": "https://phishing.example.com/unbind",
            "assets": [
                {
                    "name": "unbind-v0.6.0-x86_64-windows.exe",
                    "browser_download_url": "https://phishing.example.com/unbind.exe",
                    "size": 5000000
                },
                {
                    "name": "SHA256SUMS.txt",
                    "browser_download_url": "https://phishing.example.com/SHA256SUMS.txt",
                    "size": 128
                }
            ]
        }"#;

        let res = parse_github_release_json("0.5.0", json).unwrap();
        assert_eq!(res.release_url, RELEASES_FALLBACK_URL);
        assert!(res.exe_asset_url.is_none());
        assert!(res.checksums_url.is_none());
    }

    #[test]
    fn test_parse_sha256sums() {
        let hash_zip = "a".repeat(64);
        let hash_exe = "b".repeat(64);
        let content = format!(
            "# comment\n{}  unbind-v0.6.0-x86_64-windows-portable.zip\n{} *dist/unbind-v0.6.0-x86_64-windows.exe\ninvalid-line\n",
            hash_zip, hash_exe
        );
        let parsed = parse_sha256sums(&content);
        assert_eq!(
            parsed.get("unbind-v0.6.0-x86_64-windows-portable.zip"),
            Some(&hash_zip)
        );
        assert_eq!(
            parsed.get("unbind-v0.6.0-x86_64-windows.exe"),
            Some(&hash_exe)
        );
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn test_pe_validation_and_atomic_swap() {
        let tmp = tempdir().unwrap();
        let target_exe = tmp.path().join("unbind.exe");
        let staged_exe = tmp.path().join("staged-unbind.exe");
        let bad_exe = tmp.path().join("not-pe.exe");

        let old_hash = write_synthetic_pe(&target_exe, 0x11);
        let new_hash = write_synthetic_pe(&staged_exe, 0x22);
        fs::write(&bad_exe, vec![0x00; (MIN_PE_EXE_BYTES as usize) + 64]).unwrap();

        assert!(verify_pe_executable(&bad_exe).is_err());

        // Mismatched checksum must be rejected and leave target untouched.
        assert!(apply_staged_binary_swap(&staged_exe, &target_exe, &"0".repeat(64)).is_err());
        assert_eq!(compute_file_sha256(&target_exe).unwrap(), old_hash);

        // Valid swap replaces target and preserves backup .old file.
        apply_staged_binary_swap(&staged_exe, &target_exe, &new_hash).unwrap();
        assert_eq!(compute_file_sha256(&target_exe).unwrap(), new_hash);
        let backup = backup_path_for_exe(&target_exe);
        assert!(backup.exists());
        assert_eq!(compute_file_sha256(&backup).unwrap(), old_hash);

        cleanup_stale_update_artifacts(&target_exe);
        assert!(!backup.exists());
    }
}
