use crate::models::{
    CoreError, CoreResult, InstalledApp, WingetCheckResult, WingetUpdateInfo, WingetUpgradeResult,
};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Check whether `winget.exe` is available and query upgradable packages on demand.
/// Never runs automatically on cold start.
pub fn check_winget_updates(installed_apps: &[InstalledApp]) -> WingetCheckResult {
    let winget_version = match detect_winget_version() {
        Some(v) => v,
        None => {
            return WingetCheckResult {
                available: false,
                winget_version: String::new(),
                updates: Vec::new(),
                error: Some("winget CLI is not installed or unavailable on PATH".to_string()),
            };
        }
    };

    let mut cmd = Command::new("winget");
    cmd.args([
        "upgrade",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return WingetCheckResult {
                available: true,
                winget_version,
                updates: Vec::new(),
                error: Some(format!("Failed running winget upgrade: {}", e)),
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut updates = parse_winget_upgrade_output(&stdout);
    match_updates_to_apps(&mut updates, installed_apps);

    WingetCheckResult {
        available: true,
        winget_version,
        updates,
        error: None,
    }
}

fn detect_winget_version() -> Option<String> {
    let mut cmd = Command::new("winget");
    cmd.arg("--version");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Locale-independent parser for `winget upgrade` output.
///
/// On CJK Windows locales (e.g. Simplified Chinese), Winget computes column alignment using
/// double-width terminal cells rather than byte offsets or Rust char counts. Parsing columns
/// from right to left avoids all byte-slice misalignments while preserving multi-word names.
pub fn parse_winget_upgrade_output(stdout: &str) -> Vec<WingetUpdateInfo> {
    let mut updates = Vec::new();
    let mut in_table = false;

    for raw_line in stdout.lines() {
        // Winget prints progress spinner frames separated by '\r' on the first line.
        let line = raw_line
            .rsplit('\r')
            .next()
            .unwrap_or(raw_line)
            .trim_end();

        let trimmed = line.trim();
        if !in_table {
            if is_table_separator(trimmed) {
                in_table = true;
            }
            continue;
        }

        if trimmed.is_empty() {
            // Secondary tables (e.g. packages requiring explicit targeting / pinned) start after
            // a blank line and have their own dashed separator; stop at the primary upgrade table.
            break;
        }

        // Stop on localized summary line (e.g. "27 upgrades available." or CJK equivalent).
        if is_summary_line(trimmed) {
            break;
        }

        if let Some(item) = parse_upgrade_row(trimmed) {
            updates.push(item);
        }
    }

    updates
}

fn is_table_separator(trimmed: &str) -> bool {
    trimmed.len() >= 20 && trimmed.bytes().all(|b| b == b'-')
}

fn is_summary_line(trimmed: &str) -> bool {
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.len() < 2 {
        return true;
    }
    // A real upgrade row always ends with either a known source or a version token AND has a
    // dotted package ID. Summary lines like "27 upgrades available." have no dotted package ID.
    !tokens.iter().any(|t| looks_like_package_id(t))
}

fn looks_like_package_id(token: &str) -> bool {
    if token.len() < 3 || token.starts_with('.') || token.ends_with('.') {
        return false;
    }
    // Winget IDs are Publisher.Package (e.g. Git.Git, Obsidian.Obsidian) and occasionally MS Store
    // 12-char alphanumeric IDs (e.g. 9NKSQGP7F2NH) when source == msstore.
    let has_dot = token.contains('.');
    let valid_chars = token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'));
    has_dot && valid_chars && token.bytes().any(|b| b.is_ascii_alphabetic())
}

fn parse_upgrade_row(line: &str) -> Option<WingetUpdateInfo> {
    // Tokenize with byte offsets so we can slice the exact left-hand `name` cleanly.
    let tokens: Vec<(usize, usize, &str)> = tokenize_with_spans(line);
    if tokens.len() < 4 {
        return None;
    }

    let mut end = tokens.len();

    // 1. Optional trailing source column (e.g. "winget" or "msstore").
    let source = if matches!(
        tokens[end - 1].2.to_ascii_lowercase().as_str(),
        "winget" | "msstore"
    ) {
        end -= 1;
        tokens[end].2.to_string()
    } else {
        "winget".to_string()
    };

    if end < 3 {
        return None;
    }

    // 2. Available version (rightmost remaining token).
    end -= 1;
    let available_version = tokens[end].2.to_string();

    if end < 2 {
        return None;
    }

    // 3. Current installed version (handles "< 1.2.3" where '<' is a separate whitespace token).
    end -= 1;
    let mut current_version = tokens[end].2.to_string();
    if end >= 2 && tokens[end - 1].2 == "<" {
        end -= 1;
        current_version = format!("< {}", current_version);
    }

    if end < 1 {
        return None;
    }

    // 4. Package ID (rightmost remaining token).
    end -= 1;
    let package_id = tokens[end].2.to_string();
    if !looks_like_package_id(&package_id) && !is_msstore_id(&package_id, &source) {
        return None;
    }

    if end == 0 {
        return None;
    }

    // 5. Name is the entire prefix before the package_id token, trimmed.
    let id_start = tokens[end].0;
    let name = line[..id_start].trim().to_string();
    if name.is_empty() {
        return None;
    }

    Some(WingetUpdateInfo {
        name,
        package_id,
        current_version,
        available_version,
        source,
        matched_app_id: None,
    })
}

fn is_msstore_id(token: &str, source: &str) -> bool {
    source.eq_ignore_ascii_case("msstore")
        && token.len() >= 10
        && token.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn tokenize_with_spans(s: &str) -> Vec<(usize, usize, &str)> {
    let mut out = Vec::new();
    let mut in_tok = false;
    let mut start = 0;
    for (idx, ch) in s.char_indices() {
        if ch.is_whitespace() {
            if in_tok {
                out.push((start, idx, &s[start..idx]));
                in_tok = false;
            }
        } else if !in_tok {
            start = idx;
            in_tok = true;
        }
    }
    if in_tok {
        out.push((start, s.len(), &s[start..]));
    }
    out
}

/// Match discovered Winget updates against Unbind's `InstalledApp` list.
pub fn match_updates_to_apps(updates: &mut [WingetUpdateInfo], apps: &[InstalledApp]) {
    for upd in updates.iter_mut() {
        let upd_name_norm = normalize_app_name(&upd.name);
        let id_parts: Vec<String> = upd
            .package_id
            .split('.')
            .map(normalize_app_name)
            .filter(|p| p.len() >= 2)
            .collect();
        let pkg_tail = id_parts.last().cloned().unwrap_or_default();
        let upd_cur_clean = clean_ver(&upd.current_version);

        let mut best: Option<(i32, &str)> = None;

        for app in apps {
            let app_norm = normalize_app_name(&app.name);
            if app_norm.is_empty() {
                continue;
            }
            let mut score = 0;

            if app_norm == upd_name_norm {
                score += 100;
            } else if !upd_name_norm.is_empty()
                && (app_norm.starts_with(&upd_name_norm) || upd_name_norm.starts_with(&app_norm))
                && (app_norm.len().min(upd_name_norm.len()) >= 3)
            {
                score += 70;
            } else if !pkg_tail.is_empty() && pkg_tail.len() >= 4 && app_norm == pkg_tail {
                score += 65;
            } else {
                continue;
            }

            // Bonus when installed version matches Winget's detected current version.
            let app_ver_clean = clean_ver(&app.version);
            if !upd_cur_clean.is_empty()
                && !app_ver_clean.is_empty()
                && (app_ver_clean == upd_cur_clean
                    || app_ver_clean.starts_with(&upd_cur_clean)
                    || upd_cur_clean.starts_with(&app_ver_clean))
            {
                score += 30;
            }

            // Prefer non-Store Win32/MSI entries when source is winget.
            if upd.source.eq_ignore_ascii_case("winget")
                && app.kind != crate::models::AppKind::StoreApp
            {
                score += 10;
            }

            if best.map(|(s, _)| score > s).unwrap_or(true) {
                best = Some((score, app.id.as_str()));
            }
        }

        if let Some((score, app_id)) = best {
            if score >= 65 {
                upd.matched_app_id = Some(app_id.to_string());
            }
        }
    }
}

fn normalize_app_name(raw: &str) -> String {
    // Strip trailing architecture or version markers like "(64-bit)" / "(x64)".
    let lower = raw.to_lowercase();
    let cleaned = lower
        .replace("(64-bit)", "")
        .replace("(32-bit)", "")
        .replace("(x64)", "")
        .replace("(x86)", "")
        .replace("64-bit", "")
        .replace("32-bit", "");
    cleaned
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn clean_ver(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('<')
        .trim()
        .trim_start_matches('v')
        .trim_start_matches('V')
        .to_string()
}

/// Upgrade a single package via `winget upgrade --id <package_id> --exact`.
pub fn run_winget_upgrade(package_id: &str, app_name: &str) -> CoreResult<WingetUpgradeResult> {
    let clean_id = package_id.trim();
    if clean_id.is_empty()
        || !clean_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'))
    {
        return Err(CoreError::InvalidInput(format!(
            "Invalid Winget package ID: '{}'",
            package_id
        )));
    }

    let mut cmd = Command::new("winget");
    cmd.args([
        "upgrade",
        "--id",
        clean_id,
        "--exact",
        "--accept-source-agreements",
        "--accept-package-agreements",
        "--disable-interactivity",
    ]);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd.output()?;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let success = output.status.success();

    let last_line = stdout
        .lines()
        .chain(stderr.lines())
        .map(|l| l.rsplit('\r').next().unwrap_or(l).trim())
        .rfind(|l| !l.is_empty() && !l.chars().all(|c| matches!(c, '-' | '\\' | '|' | '/' | ' ')))
        .unwrap_or(if success {
            "Upgrade completed successfully"
        } else {
            "Winget upgrade exited with non-zero status"
        })
        .to_string();

    Ok(WingetUpgradeResult {
        package_id: clean_id.to_string(),
        app_name: app_name.to_string(),
        success,
        exit_code,
        message: last_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AppKind, Arch, InstallScope, PackageManagerKind};

    fn dummy_app(id: &str, name: &str, version: &str) -> InstalledApp {
        InstalledApp {
            id: id.to_string(),
            name: name.to_string(),
            publisher: String::new(),
            version: version.to_string(),
            install_date: String::new(),
            estimated_size_kb: 0,
            install_location: String::new(),
            uninstall_string: String::new(),
            quiet_uninstall_string: String::new(),
            display_icon: String::new(),
            url_info_about: String::new(),
            registry_key: String::new(),
            registry_hive: "HKLM".to_string(),
            registry_view: "64".to_string(),
            product_code: String::new(),
            package_full_name: String::new(),
            package_family_name: String::new(),
            kind: AppKind::Win32,
            scope: InstallScope::Machine,
            arch: Arch::X64,
            is_system_component: false,
            is_windows_update: false,
            is_valid_entry: true,
            has_uninstaller: true,
            tracked_trace_id: None,
            package_manager: PackageManagerKind::None,
            size_measured: false,
        }
    }

    #[test]
    fn test_parse_winget_output_cjk_and_english_and_less_than_version() {
        let sample = "\r-\r\\\r\
Name                       Id                       Version       Available     Source\n\
-----------------------------------------------------------------------------------------\n\
Adobe Acrobat (64-bit)     Adobe.Acrobat.Pro        23.008.20533  26.003.22110  winget\n\
Git                        Git.Git                  2.50.1        2.56.1        winget\n\
Obsidian                   Obsidian.Obsidian        1.12.7        1.15.1        winget\n\
Some Legacy App            Vendor.LegacyApp         < 1.0.0       1.2.0         winget\n\
Microsoft Visual Studio Code Microsoft.VisualStudioCode 1.95.0    1.98.2        winget\n\
5 upgrades available.\n";

        let mut parsed = parse_winget_upgrade_output(sample);
        assert_eq!(parsed.len(), 5);
        assert_eq!(parsed[0].name, "Adobe Acrobat (64-bit)");
        assert_eq!(parsed[0].package_id, "Adobe.Acrobat.Pro");
        assert_eq!(parsed[0].current_version, "23.008.20533");
        assert_eq!(parsed[0].available_version, "26.003.22110");
        assert_eq!(parsed[0].source, "winget");

        assert_eq!(parsed[3].name, "Some Legacy App");
        assert_eq!(parsed[3].current_version, "< 1.0.0");
        assert_eq!(parsed[3].available_version, "1.2.0");

        let apps = vec![
            dummy_app("id-git", "Git", "2.50.1"),
            dummy_app("id-obs", "Obsidian", "1.12.7"),
        ];
        match_updates_to_apps(&mut parsed, &apps);
        assert_eq!(parsed[1].matched_app_id.as_deref(), Some("id-git"));
        assert_eq!(parsed[2].matched_app_id.as_deref(), Some("id-obs"));
        assert_eq!(parsed[0].matched_app_id, None);

        // Verify CJK double-width localized header & summary line via pure-ASCII \u escapes.
        let cjk_sample = "\u{540d}\u{79f0}            ID              \u{7248}\u{672c}   \u{53ef}\u{7528}   \u{6e90}\n\
------------------------------------------------------------------\n\
\u{5fae}\u{4fe1} WeChat      Tencent.WeChat  3.9.10 3.9.12 winget\n\
1 \u{5347}\u{7ea7}\u{53ef}\u{7528}\u{3002}\n";
        let cjk_parsed = parse_winget_upgrade_output(cjk_sample);
        assert_eq!(cjk_parsed.len(), 1);
        assert_eq!(cjk_parsed[0].package_id, "Tencent.WeChat");
        assert_eq!(cjk_parsed[0].current_version, "3.9.10");
        assert_eq!(cjk_parsed[0].available_version, "3.9.12");
    }
}
