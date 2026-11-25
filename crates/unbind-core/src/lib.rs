pub mod discovery;
pub mod export;
pub mod ipc;
pub mod models;
pub mod portable;
pub mod safety;
pub mod startup;
pub mod tracker;
pub mod uninstaller;
pub mod unlocker;

/// Read a friendly Windows version string (e.g. "Windows 11 Pro (Build 26200)") from registry.
pub fn get_windows_version_string() -> String {
    use winreg::enums::*;
    use winreg::RegKey;

    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(cv) = hklm.open_subkey_with_flags(
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
        KEY_READ,
    ) else {
        return "Windows".to_string();
    };

    let product: String = cv.get_value("ProductName").unwrap_or_else(|_| "Windows".into());
    let build: String = cv.get_value("CurrentBuildNumber").unwrap_or_default();
    let display_ver: String = cv.get_value("DisplayVersion").unwrap_or_default();
    let build_num: u32 = build.parse().unwrap_or(0);

    // Microsoft keeps "Windows 10" in ProductName even on Windows 11 (build >= 22000).
    let adjusted_product = if build_num >= 22000 && product.contains("Windows 10") {
        product.replace("Windows 10", "Windows 11")
    } else {
        product
    };

    if !display_ver.is_empty() && !build.is_empty() {
        format!("{} {} (Build {})", adjusted_product, display_ver, build)
    } else if !build.is_empty() {
        format!("{} (Build {})", adjusted_product, build)
    } else {
        adjusted_product
    }
}
