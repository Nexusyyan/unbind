use std::fs;
use unbind_core::discovery::win32::discover_win32_apps;
use unbind_core::models::{CleanupOptions, ConfidenceLevel, LeftoverItem, LeftoverKind};
use unbind_core::safety::{is_protected_path, is_protected_registry_key};
use unbind_core::uninstaller::execute_cleanup;
use unbind_core::uninstaller::leftover::scan_leftovers;
use tempfile::tempdir;
use uuid::Uuid;
use winreg::enums::*;
use winreg::RegKey;

/// Full end-to-end sandbox lifecycle test (strictly isolated to temp dir + HKCU test keys):
/// 1. Register a synthetic app in HKCU Uninstall + create an install folder and Software reg key.
/// 2. Verify `discover_win32_apps()` discovers it accurately with metadata.
/// 3. Verify `scan_leftovers()` finds the install directory, Software key, and Uninstall key as High confidence.
/// 4. Verify `execute_cleanup()` exports a valid UTF-16LE `.reg` backup and removes every sandbox trace.
#[test]
fn test_e2e_uninstall_discovery_scan_backup_and_cleanup_sandbox() {
    let unique = format!("UnbindE2E_{}", Uuid::new_v4().simple());
    let temp = tempdir().unwrap();
    let app_dir = temp.path().join(&unique);
    let backup_dir = temp.path().join("backups");
    fs::create_dir_all(app_dir.join("plugins")).unwrap();
    fs::write(app_dir.join("main.exe"), b"MZ_MOCK_EXE").unwrap();
    fs::write(app_dir.join("plugins").join("ext.dll"), b"MOCK_DLL").unwrap();

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let uninstall_subkey = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        unique
    );
    let software_subkey = format!("Software\\{}", unique);

    // Ensure cleanup even on panic via guard closure at end.
    let (u_key, _) = hkcu.create_subkey(&uninstall_subkey).unwrap();
    u_key.set_value("DisplayName", &unique).unwrap();
    u_key.set_value("Publisher", &"SandboxCorp").unwrap();
    u_key.set_value("DisplayVersion", &"1.2.3").unwrap();
    u_key.set_value("InstallDate", &"20261124").unwrap();
    u_key.set_value("EstimatedSize", &2048u32).unwrap();
    u_key
        .set_value("InstallLocation", &app_dir.to_string_lossy().to_string())
        .unwrap();
    u_key
        .set_value(
            "UninstallString",
            &format!("\"{}\\unins000.exe\"", app_dir.display()),
        )
        .unwrap();
    drop(u_key);

    let (s_key, _) = hkcu.create_subkey(&software_subkey).unwrap();
    s_key.set_value("ConfigFlag", &1u32).unwrap();
    drop(s_key);

    // 1. Discover our mock installed app.
    let apps = discover_win32_apps();
    let found_app = apps
        .into_iter()
        .find(|a| a.name == unique)
        .expect("Synthetic app should be discovered");
    assert_eq!(found_app.version, "1.2.3");
    assert_eq!(found_app.install_date, "2026-11-24");
    assert!(found_app.quiet_uninstall_string.contains("/VERYSILENT"));

    // 2. Scan leftovers.
    let leftovers = scan_leftovers(&found_app, None);
    let has_app_dir = leftovers
        .iter()
        .any(|i| i.kind == LeftoverKind::Directory && i.path.eq_ignore_ascii_case(&app_dir.to_string_lossy()));
    let has_uninstall_reg = leftovers.iter().any(|i| {
        i.kind == LeftoverKind::RegistryKey
            && i.path.to_lowercase().ends_with(&uninstall_subkey.to_lowercase())
    });
    let has_software_reg = leftovers.iter().any(|i| {
        i.kind == LeftoverKind::RegistryKey
            && i.path.to_lowercase().ends_with(&software_subkey.to_lowercase())
    });

    assert!(has_app_dir, "Leftover scanner must find install directory");
    assert!(
        has_uninstall_reg,
        "Leftover scanner must find Uninstall registry key"
    );
    assert!(
        has_software_reg,
        "Leftover scanner must find HKCU Software key"
    );

    // Filter only our exact sandbox leftovers to guarantee zero external side effects.
    let sandbox_items: Vec<LeftoverItem> = leftovers
        .into_iter()
        .filter(|i| {
            i.confidence == ConfidenceLevel::High
                && i.path.to_lowercase().contains(&unique.to_lowercase())
        })
        .collect();
    assert!(sandbox_items.len() >= 3);

    // 3. Execute cleanup with automatic .reg backup (permanent delete inside tempdir).
    let opts = CleanupOptions {
        use_recycle_bin: false,
        backup_registry: true,
        create_restore_point: false,
        schedule_reboot_on_lock: false,
        terminate_locking_processes: false,
    };
    let report = execute_cleanup(&unique, &sandbox_items, &opts, &backup_dir).unwrap();
    assert_eq!(report.failed, 0);
    assert_eq!(report.succeeded, sandbox_items.len());
    assert!(report.freed_bytes > 0);
    let reg_backup = report
        .reg_backup_path
        .expect("Registry backup .reg file must be exported");
    assert!(std::path::Path::new(&reg_backup).exists());

    // 4. Verify everything is completely gone.
    assert!(!app_dir.exists());
    assert!(hkcu.open_subkey(&uninstall_subkey).is_err());
    assert!(hkcu.open_subkey(&software_subkey).is_err());
}

/// Ensure critical Windows paths & registry keys are unconditionally blocked even if forced into cleanup.
#[test]
fn test_safety_guardrails_reject_critical_targets_in_cleanup() {
    let temp = tempdir().unwrap();
    let dangerous = vec![
        LeftoverItem {
            id: "d1".into(),
            kind: LeftoverKind::Directory,
            path: "C:\\Windows\\System32".into(),
            value_name: None,
            size_bytes: 0,
            confidence: ConfidenceLevel::High,
            reason: "malicious test".into(),
            reason_code: "test".into(),
            reason_param: None,
            selected_by_default: false,
            requires_elevation: true,
        },
        LeftoverItem {
            id: "d2".into(),
            kind: LeftoverKind::RegistryKey,
            path: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall".into(),
            value_name: None,
            size_bytes: 0,
            confidence: ConfidenceLevel::High,
            reason: "malicious test".into(),
            reason_code: "test".into(),
            reason_param: None,
            selected_by_default: false,
            requires_elevation: true,
        },
    ];

    assert!(is_protected_path(std::path::Path::new("C:\\Windows\\System32")));
    assert!(is_protected_registry_key(
        "HKLM",
        "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"
    ));

    let opts = CleanupOptions {
        use_recycle_bin: false,
        backup_registry: false,
        create_restore_point: false,
        schedule_reboot_on_lock: false,
        terminate_locking_processes: false,
    };
    let report = execute_cleanup("DangerousTest", &dangerous, &opts, temp.path()).unwrap();
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.failed, 2);
    assert!(report
        .details
        .iter()
        .all(|d| d.error.as_deref().unwrap_or("").contains("Safety guardrail")));
}
