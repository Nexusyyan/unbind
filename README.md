<p align="center">
  <img src="assets/social-preview.png" alt="Unbind - Precision Windows Uninstaller" width="100%" />
</p>

<h1 align="center">Unbind</h1>

<p align="center">
  <strong>Cleanly decouple software from Windows without leftover traces.</strong><br />
  A lightweight, fast, portable Windows uninstaller, leftover scanner, install tracer, and startup manager built with <strong>Rust</strong>, <strong>Tauri v2</strong>, and <strong>React</strong>.
</p>

<p align="center">
  <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-18181b?style=flat-square" />
  <img alt="Stack" src="https://img.shields.io/badge/stack-Rust%20%2B%20Tauri%20v2-52525b?style=flat-square" />
  <img alt="License" src="https://img.shields.io/badge/license-MIT-71717a?style=flat-square" />
</p>

---

## Features

- **Unified Software Discovery (Win32 / MSI / UWP Store Apps)**: Parallel discovery across 64-bit & 32-bit `HKLM` / `HKCU` Uninstall views and WinRT `PackageManager`, with native Win32/UWP icon extraction and smart filtering (All, Recent 30d, Large >100MB, Broken/Invalid entries, System Components).
- **Step-by-Step Uninstall Wizard**: Runs vendor standard or quiet uninstallers with Windows **Job Object** process-tree monitoring, followed by parallel multi-anchor leftover scanning (Registry, AppData, ProgramData, Program Files, Start Menu/Desktop shortcuts, App Paths, MuiCache, Services).
- **Safety Guardrails & Backups**: Hardcoded OS protection denylist (`Windows`, `System32`, critical hives/containers), confidence grading (`Safe` vs `Review`), automatic UTF-16LE `.reg` backup export before deletion, Recycle Bin deletion by default, optional Windows System Restore Point (`SRSetRestorePointW`), and `MoveFileExW` delete-on-reboot fallback.
- **Force Removal & Restart Manager File Unlocker**: Force-clean stubborn software or any dropped `.exe` / `.lnk` / folder; inspect locking processes via Windows Restart Manager (`RmStartSession`) and terminate them safely.
- **Batch Uninstall Queue**: Uninstall multiple applications sequentially (interactive or quiet) with a consolidated leftover cleanup review.
- **Startup Manager**: Manage Registry `Run`/`RunOnce` (64/32-bit), Startup Folders, and third-party Scheduled Tasks with non-destructive enable/disable natively synchronized with Windows Task Manager (`StartupApproved`).
- **Install Tracker**: Capture fast pre/post system snapshots across key filesystem and registry trees to save `.ubtrace.json` records for 100% clean tracked removal later.
- **Privilege-Separated Portable Single Binary**: UI stays non-elevated (preserving Explorer drag-and-drop and avoiding Windows 11 WebView2 elevated profile issues), while a dual-mode `--elevated-worker` instance connects over a PID-verified Named Pipe on demand (one UAC prompt per session).
- **Portable Mode & Export**: Drop a `portable.json` next to `unbind.exe` (or toggle in Settings) to keep all data in `./unbind-data`. Export app lists to HTML, CSV, or JSON. Bilingual UI (English / Simplified Chinese) with Light and Dark neutral themes.

## Architecture

```text
unbind/
├── assets/               # Brand & GitHub social preview assets (1280x640)
├── crates/unbind-core/   # Decoupled Windows engine library (discovery, safety, scanner, unlocker, startup, tracker, export, ipc)
├── src-tauri/            # Tauri v2 desktop host + Named Pipe UAC elevation bridge + dual-mode worker entrypoint
└── src/                  # Minimalist neutral React + TypeScript + Tailwind CSS v4 frontend
```

## Development & Build

### Prerequisites
- Windows 10/11 (WebView2 Runtime built in)
- Rust stable (`x86_64-pc-windows-msvc`) + Visual Studio Build Tools (MSVC + Windows SDK)
- Node.js 20+ & `pnpm`

### Commands

```powershell
# Install frontend dependencies
pnpm install

# Run automated tests (Rust core + E2E sandbox + Frontend Vitest)
cargo test --workspace
pnpm test

# Run Clippy & TypeScript checks
cargo clippy --workspace --all-targets -- -D warnings
pnpm typecheck

# Launch in development mode
pnpm tauri dev

# Build compact portable single-file release executable
pnpm tauri build --no-bundle
# Output: target/release/unbind.exe
```

### Portable Mode
Place an empty `portable.json` file alongside `unbind.exe` (or enable **Portable Mode** in Settings). All settings, `.reg` backups, install traces (`.ubtrace.json`), and logs are stored locally inside `unbind-data/` beside the executable.

## License

MIT
