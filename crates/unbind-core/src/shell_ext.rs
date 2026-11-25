use crate::models::{CoreError, CoreResult, ShellIntegrationStatus};
use std::path::Path;
use winreg::enums::*;
use winreg::RegKey;

const FORCE_FILE_KEY: &str = "Software\\Classes\\*\\shell\\UnbindForceRemove";
const FORCE_DIR_KEY: &str = "Software\\Classes\\Directory\\shell\\UnbindForceRemove";
const TRACK_EXE_KEY: &str = "Software\\Classes\\exefile\\shell\\UnbindTrackInstall";
const TRACK_MSI_KEY: &str = "Software\\Classes\\Msi.Package\\shell\\UnbindTrackInstall";

/// Inspect current HKCU Explorer context menu registration state (requires zero UAC).
pub fn get_shell_integration_status() -> ShellIntegrationStatus {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let force_file = hkcu.open_subkey_with_flags(FORCE_FILE_KEY, KEY_READ).is_ok();
    let force_dir = hkcu.open_subkey_with_flags(FORCE_DIR_KEY, KEY_READ).is_ok();
    let track_exe = hkcu.open_subkey_with_flags(TRACK_EXE_KEY, KEY_READ).is_ok();

    let registered_exe = hkcu
        .open_subkey_with_flags(format!("{}\\command", FORCE_FILE_KEY), KEY_READ)
        .or_else(|_| hkcu.open_subkey_with_flags(format!("{}\\command", TRACK_EXE_KEY), KEY_READ))
        .ok()
        .and_then(|k| k.get_value::<String, _>("").ok())
        .and_then(|cmd| crate::discovery::win32::extract_executable_from_command(&cmd))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    ShellIntegrationStatus {
        force_remove_enabled: force_file && force_dir,
        track_install_enabled: track_exe,
        registered_exe,
    }
}

/// Enable or disable HKCU Windows Explorer right-click menu entries for Unbind.
/// Writes strictly under `HKCU\Software\Classes`, requiring no Administrator elevation.
pub fn set_shell_integration(
    force_remove: bool,
    track_install: bool,
    locale: &str,
) -> CoreResult<ShellIntegrationStatus> {
    let exe_path = std::env::current_exe().map_err(CoreError::Io)?;
    set_shell_integration_with_exe(&exe_path, force_remove, track_install, locale)?;
    Ok(get_shell_integration_status())
}

pub fn format_shell_command(exe_path: &Path, flag: &str) -> String {
    format!("\"{}\" {} \"%1\"", exe_path.display(), flag)
}

fn set_shell_integration_with_exe(
    exe_path: &Path,
    force_remove: bool,
    track_install: bool,
    locale: &str,
) -> CoreResult<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let is_zh = locale.to_lowercase().starts_with("zh");

    let force_label = if is_zh {
        "\u{4F7F}\u{7528} Unbind \u{5F3A}\u{5236}\u{5378}\u{8F7D}"
    } else {
        "Force Remove with Unbind"
    };
    let track_label = if is_zh {
        "\u{4F7F}\u{7528} Unbind \u{8FFD}\u{8E2A}\u{5B89}\u{88C5}"
    } else {
        "Track Install with Unbind"
    };

    let icon_val = format!("\"{}\",0", exe_path.display());

    if force_remove {
        let cmd = format_shell_command(exe_path, "--force-remove");
        register_verb(&hkcu, FORCE_FILE_KEY, force_label, &icon_val, &cmd)?;
        register_verb(&hkcu, FORCE_DIR_KEY, force_label, &icon_val, &cmd)?;
    } else {
        let _ = hkcu.delete_subkey_all(FORCE_FILE_KEY);
        let _ = hkcu.delete_subkey_all(FORCE_DIR_KEY);
    }

    if track_install {
        let cmd = format_shell_command(exe_path, "--track-install");
        register_verb(&hkcu, TRACK_EXE_KEY, track_label, &icon_val, &cmd)?;
        register_verb(&hkcu, TRACK_MSI_KEY, track_label, &icon_val, &cmd)?;
    } else {
        let _ = hkcu.delete_subkey_all(TRACK_EXE_KEY);
        let _ = hkcu.delete_subkey_all(TRACK_MSI_KEY);
    }

    Ok(())
}

fn register_verb(
    hkcu: &RegKey,
    verb_subkey: &str,
    label: &str,
    icon_val: &str,
    command: &str,
) -> CoreResult<()> {
    let (key, _) = hkcu.create_subkey(verb_subkey)?;
    key.set_value("", &label)?;
    key.set_value("Icon", &icon_val)?;
    let (cmd_key, _) = key.create_subkey("command")?;
    cmd_key.set_value("", &command)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_shell_command() {
        let cmd = format_shell_command(Path::new("C:\\Tools\\unbind.exe"), "--force-remove");
        assert_eq!(cmd, "\"C:\\Tools\\unbind.exe\" --force-remove \"%1\"");
    }

    #[test]
    fn test_register_verb_sandbox() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let sandbox_key = format!(
            "Software\\Unbind_ShellExtTest_{}",
            uuid::Uuid::new_v4().simple()
        );
        register_verb(
            &hkcu,
            &sandbox_key,
            "Force Remove with Unbind",
            "\"C:\\Tools\\unbind.exe\",0",
            "\"C:\\Tools\\unbind.exe\" --force-remove \"%1\"",
        )
        .unwrap();

        let opened = hkcu.open_subkey_with_flags(&sandbox_key, KEY_READ).unwrap();
        let label: String = opened.get_value("").unwrap();
        assert_eq!(label, "Force Remove with Unbind");
        let cmd_sub = opened.open_subkey_with_flags("command", KEY_READ).unwrap();
        let cmd: String = cmd_sub.get_value("").unwrap();
        assert!(cmd.contains("--force-remove"));

        hkcu.delete_subkey_all(&sandbox_key).unwrap();
    }
}
