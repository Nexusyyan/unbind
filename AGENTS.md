# Unbind -- Repository Agent Guide

Lightweight, portable Windows software uninstaller, leftover scanner, install tracker, and startup manager built with Rust, Tauri v2, and React.

## Stack & Layout

- **Stack**: Rust + Tauri v2 + React 19 + TypeScript + Tailwind CSS v4 + `pnpm` (single root Cargo Workspace)
- **Key paths**:
  - `crates/unbind-core/` -- Core Windows engine (discovery, real-size cache, safety, leftover scanner, SCM service removal, Restart Manager unlocker, startup manager, hybrid install tracker, HKCU shell integration, WinHTTP updater, IPC)
  - `src-tauri/` -- Tauri v2 desktop host, single-instance CLI router, window bounds persistence, Named Pipe UAC elevation bridge, dual-mode `--elevated-worker`
  - `src/` -- Minimalist neutral React + TypeScript UI (`src/i18n/locales/{en,zh-CN}.json`)
  - `DESIGN.md` -- Canonical UI design system and brand specification
  - `.github/workflows/{ci,release}.yml` -- Automated Windows CI and portable GitHub Release workflow

## Verification Commands

```powershell
# 1. Pure 7-bit ASCII source check
Get-ChildItem -Recurse -File -Include *.rs,*.ts,*.tsx | Where-Object { $_.FullName -notmatch '\\(target|node_modules|dist|\.git)\\' } | ForEach-Object { if ([System.IO.File]::ReadAllBytes($_.FullName) | Where-Object { $_ -gt 127 }) { throw "Non-ASCII: $($_.FullName)" } }

# 2. Frontend typecheck, tests, and build
pnpm typecheck
pnpm test
pnpm build

# 3. Rust lint and full workspace tests
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 4. Standalone portable Release build
pnpm tauri build --no-bundle
```

## Project-Specific Constraints

- Keep all `*.rs`, `*.ts`, `*.tsx`, `*.ps1`, and `*.yml` files 100% pure 7-bit ASCII English; isolate Simplified Chinese UI strings exclusively in `src/i18n/locales/zh-CN.json`.
- Follow `DESIGN.md` strictly: pure neutral surfaces (`#FFFFFF` / `#F9F9FA`), zero cool-blue accents, `strokeWidth={1.5}` Lucide icons, `.num-tabular` on all numeric fields, hover-reveal destructive buttons, and custom monochrome `.ui-checkbox` controls.
- Keep UI non-elevated (preserves Explorer drag-and-drop and avoids WebView2 elevated profile issues); route privileged operations through the PID- and token-verified Named Pipe `--elevated-worker`.
- Never allow `--elevated-worker` to initialize Tauri or single-instance locks (`main.rs` must short-circuit worker mode first).
- Close any running `unbind.exe` before rebuilding `target/release/unbind.exe`.
