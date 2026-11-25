# Changelog

All notable changes to **Unbind** are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-11-25

### Added
- **ETW Real-Time Kernel Install Tracker (`etw_tracker.rs`)**: Named real-time ETW session hosted inside `--elevated-worker` (zero driver signing) subscribing to Windows built-in `Microsoft-Windows-Kernel-Process`, `Microsoft-Windows-Kernel-File`, and `Microsoft-Windows-Kernel-Registry` providers with pre-bind event buffering, PID-tree isolation, NT-device-to-DOS conversion (`QueryDosDeviceW`), post-stop `Path::exists()` filtering of transient `%TEMP%` files, and snapshot fusion for Services, Scheduled Tasks, and MSI `msiserver` compatibility.
- **Portable / Green Software Library Manager (`portable_apps.rs` + `PortableAppsView`)**: Configure portable library root directories, scan depth 1-2 app folders and single-file `.exe` utilities, read PE version metadata via native `GetFileVersionInfoW` / `VerQueryValueW`, extract native icons via `IconCache`, measure real directory sizes, launch apps with proper working directories, create Desktop / Start Menu `.lnk` shortcuts via COM `IShellLinkW`, and deep-clean leftovers via `UninstallWizardModal`.
- **On-Demand Winget Update Check & 1-Click Upgrade (`winget.rs`)**: Strictly on-demand update scanning (zero cold-start impact) using a locale-independent right-to-left table parser resilient to CJK double-width terminal output, matching updates into `AppListView` with an `Updatable` filter pill, subtle version diff cues, inspector upgrade button, and batch upgrade flow guarded by `ConfirmDialog`.
- **Unified System Tools View (`ToolsView`)**: Aggregates `Startup Manager` and `Portable Apps` under clean borderless secondary tabs, keeping the minimalist sidebar strictly at 6 items.

### Changed
- Upgraded GitHub Actions workflows (`ci.yml`, `release.yml`) to Node 24 native actions (`actions/checkout@v6`, `pnpm/action-setup@v5`, `actions/setup-node@v6` with `node-version: 24`).
- Bundled `README.zh-CN.md` inside the portable Windows ZIP release artifact and added pure ASCII validation to `release.yml`.

## [0.4.0] - 2026-11-25

### Added
- Background real disk size resolver for Win32 and UWP apps with persistent mtime-aware `size-cache.json` and streaming `apps://size-batch` updates.
- Deep leftover scanner expansion covering Scheduled Tasks, Explorer shell context menus, file associations, UWP `%LOCALAPPDATA%\Packages\<PFN>`, WOW6432Node Run entries, and clean SCM service stop/delete (`Win32_System_Services`).
- Zero-UAC `HKCU` Windows Explorer context menu integration (`--force-remove`, `--track-install`) with `tauri-plugin-single-instance` window handoff.
- Portable-safe window bounds persistence in `settings.json`, in-app `.reg` backup preview and 1-click restore, running-process pre-check in the Uninstall Wizard, smart installer drag-and-drop routing, native `WinHTTP` GitHub Releases update checker, and `.ubtrace.json` import/export.

### Changed
- Migrated canonical UI design specification to root `DESIGN.md`; added bilingual `README.zh-CN.md` and `AGENTS.md`.
- Replaced all native `window.confirm` calls with bespoke `ConfirmDialog` and upgraded Settings to immediate auto-save.

## [0.3.0] - 2026-11-24

### Added
- Automated GitHub Actions Windows CI (`ci.yml`) and portable single-file GitHub Release workflow (`release.yml`) with `SHA256SUMS.txt`.
- Full brand and icon suite derived from the `Decouple` vector master (`src-tauri/icons/logo.svg`, `.ico`, multi-DPI PNGs, `favicon.svg`, and 1280x640 social preview banner).

## [0.2.0] - 2026-11-23

### Changed
- Redesigned UI around pure neutral surfaces (`#FFFFFF` / `#F9F9FA` light, `#161618` / `#111113` dark), zero cool-blue accents, `strokeWidth={1.5}` Lucide icons, `.num-tabular` numeric discipline, hover-reveal destructive buttons, and custom monochrome `.ui-checkbox` controls.

## [0.1.0] - 2026-11-22

### Added
- Initial MVP featuring Rust `unbind-core` + Tauri v2 + React 19 desktop host, dual-mode `--elevated-worker` Named Pipe privilege separation, Win32/MSI/UWP app discovery, Job Object uninstaller monitoring, leftover scanning & safe cleanup, Startup Manager, Install Tracker, and Portable Mode (`portable.json`).
