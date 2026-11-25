use crate::models::{CoreError, CoreResult, UpdateCheckResult};

const GITHUB_HOST: &str = "api.github.com";
const GITHUB_RELEASES_PATH: &str = "/repos/Nexusyyan/unbind/releases/latest";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// Parse a SemVer string like "v0.4.0" or "0.4.1-beta.1" into `(major, minor, patch)`.
pub fn parse_semver_tuple(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let core = trimmed.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    let patch = parts.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

/// Returns true if `latest` is strictly greater than `current`.
pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver_tuple(current), parse_semver_tuple(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

/// Check the latest published GitHub Release via Windows built-in WinHTTP (zero TLS/HTTP crate bloat,
/// respects Windows system proxy and TLS 1.3 natively).
#[cfg(windows)]
pub fn check_github_release(current_version: &str) -> CoreResult<UpdateCheckResult> {
    let body = winhttp_get_json(GITHUB_HOST, GITHUB_RELEASES_PATH, current_version)?;
    let parsed: serde_json::Value = serde_json::from_str(&body)?;

    let tag = parsed
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or(current_version)
        .trim()
        .to_string();
    let latest_clean = tag.trim_start_matches(['v', 'V']).to_string();
    let release_url = parsed
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://github.com/Nexusyyan/unbind/releases/latest")
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
    let notes_preview: String = raw_notes.chars().take(420).collect();

    let has_update = is_newer_version(current_version, &latest_clean);
    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: latest_clean,
        has_update,
        release_url,
        published_at,
        notes_preview,
    })
}

#[cfg(not(windows))]
pub fn check_github_release(current_version: &str) -> CoreResult<UpdateCheckResult> {
    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        latest_version: current_version.to_string(),
        has_update: false,
        release_url: "https://github.com/Nexusyyan/unbind/releases/latest".to_string(),
        published_at: String::new(),
        notes_preview: String::new(),
    })
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
fn winhttp_get_json(host: &str, path: &str, app_version: &str) -> CoreResult<String> {
    use std::ffi::c_void;
    use windows::core::PCWSTR;
    use windows::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
        WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
        WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    let ua = to_wide_null(&format!("Unbind/{}", app_version));
    let host_w = to_wide_null(host);
    let path_w = to_wide_null(path);
    let method_w = to_wide_null("GET");
    let headers_w = to_wide_null("Accept: application/vnd.github+json\r\n");

    unsafe {
        let session = WinHttpOpen(
            PCWSTR(ua.as_ptr()),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        );
        if session.is_null() {
            return Err(CoreError::WinApi("WinHttpOpen failed".to_string()));
        }

        let _ = WinHttpSetTimeouts(session, 8000, 8000, 10000, 10000);

        let connect = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), 443, 0);
        if connect.is_null() {
            let _ = WinHttpCloseHandle(session);
            return Err(CoreError::WinApi("WinHttpConnect failed".to_string()));
        }

        let request = WinHttpOpenRequest(
            connect,
            PCWSTR(method_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        );
        if request.is_null() {
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err(CoreError::WinApi("WinHttpOpenRequest failed".to_string()));
        }

        let _ = WinHttpAddRequestHeaders(
            request,
            &headers_w[..headers_w.len().saturating_sub(1)],
            WINHTTP_ADDREQ_FLAG_ADD,
        );

        let send_ok = WinHttpSendRequest(request, None, None, 0, 0, 0).is_ok();
        let recv_ok = send_ok && WinHttpReceiveResponse(request, std::ptr::null_mut()).is_ok();
        if !recv_ok {
            let _ = WinHttpCloseHandle(request);
            let _ = WinHttpCloseHandle(connect);
            let _ = WinHttpCloseHandle(session);
            return Err(CoreError::WinApi(
                "Network request to GitHub API failed or timed out".to_string(),
            ));
        }

        let mut status_code: u32 = 0;
        let mut status_len = std::mem::size_of::<u32>() as u32;
        let _ = WinHttpQueryHeaders(
            request,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status_code as *mut u32 as *mut c_void),
            &mut status_len,
            std::ptr::null_mut(),
        );

        let mut out = Vec::new();
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(request, &mut avail).is_err() || avail == 0 {
                break;
            }
            let to_read = (avail as usize).min(MAX_RESPONSE_BYTES.saturating_sub(out.len()));
            if to_read == 0 {
                break;
            }
            let mut buf = vec![0u8; to_read];
            let mut read: u32 = 0;
            if WinHttpReadData(
                request,
                buf.as_mut_ptr() as *mut c_void,
                to_read as u32,
                &mut read,
            )
            .is_err()
                || read == 0
            {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() >= MAX_RESPONSE_BYTES {
                break;
            }
        }

        let _ = WinHttpCloseHandle(request);
        let _ = WinHttpCloseHandle(connect);
        let _ = WinHttpCloseHandle(session);

        if status_code >= 400 {
            return Err(CoreError::WinApi(format!(
                "GitHub API returned HTTP {}",
                status_code
            )));
        }

        String::from_utf8(out).map_err(|e| CoreError::WinApi(format!("Invalid UTF-8: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_comparison() {
        assert!(is_newer_version("0.3.0", "v0.4.0"));
        assert!(is_newer_version("0.4.0", "0.4.1"));
        assert!(is_newer_version("0.4.0", "1.0.0"));
        assert!(!is_newer_version("0.4.0", "v0.4.0"));
        assert!(!is_newer_version("0.4.0", "0.3.9"));
        assert!(!is_newer_version("0.4.0", "invalid"));
    }
}
