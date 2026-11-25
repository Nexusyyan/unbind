# Unbind Design System

## Direction

**Personality:** Precision & Restraint (Antigravity-aligned minimalist desktop utility)

**Foundation:** Pure neutral (strictly zero cool-blue/cyan cast)

**Depth:** Hairline borders (`rgba(0, 0, 0, 0.06)`) with zero default shadows; whisper diffuse shadows reserved exclusively for floating search containers, popovers, and modals.

**Signature Architecture:** Continuous `#F9F9FA` L-shaped chrome (TitleBar + Sidebar) framing an inset pure-white `#FFFFFF` workspace canvas with a `12px` top-left continuous radius and `1px` hairline edge. Native full-color application icons act as the sole natural color accents on screen.

## Brand Identity & Logo

- **Brand Name:** `Unbind` (`unbind.exe`, `unbind-core`, `com.unbind.desktop`)
- **Master Asset:** `src-tauri/icons/logo.svg` (1024x1024 Swiss grid)
- **Synced Exports:** `src-tauri/icons/{icon.ico, icon.png, 32x32.png, 64x64.png, 128x128.png, 128x128@2x.png}`, `public/favicon.svg`, `assets/social-preview.{svg,png}` (1280x640 GitHub Social Preview), and `src/components/BrandLogo.tsx` (used in `TitleBar.tsx` and `SettingsView.tsx`).
- **Tile Geometry (Desktop/Taskbar):** Continuous luminous white squircle (`#FFFFFF` to `#F5F5F7`, `rx=224` on 1024 grid, 89% optical fill) with a hairline `rgba(24, 24, 27, 0.08)` rim so it sits cleanly on both light and dark taskbars.
- **Emblem Geometry (Decouple):** Twin point-symmetric architectural U-links rotated `-45deg`, cleanly offset along the diagonal release axis (`62px` negative-space channel on 1024 grid) — deep charcoal primary link (`#27272A` -> `#141417`) and refined graphite secondary link (`#7B7B85` -> `#52525B`), expressing zero-trace software decoupling without generic trash-bin tropes.
- **In-App Mark (`BrandLogo`):** Bare `17px x 17px` monochrome SVG (`currentColor`, `strokeWidth="2.35"`, secondary link at `opacity="0.52"`) adapting seamlessly to Light and Dark themes.

## Tokens

Canonical source: `src/index.css`

### Spacing
Base: `4px`
Scale: `4, 6, 8, 12, 16, 20, 24`
Row height: `62px` (virtualized app list)
Sidebar width: `222px`
Inspector drawer width: `312px`

### Colors (Light — `:root`)
```css
--bg-chrome: #f9f9fa;
--bg-sidebar: #f9f9fa;
--bg-canvas: #ffffff;
--bg-surface: #ffffff;
--bg-subtle: #f9f9fa;
--bg-hover: #f2f2f4;
--bg-selected: #eaeaef;
--bg-track: #efeff2;

--border-hairline: rgba(0, 0, 0, 0.06);
--border-strong: rgba(0, 0, 0, 0.12);
--border-focus: rgba(24, 24, 27, 0.28);

--text-primary: #1a1a1d;      /* Never #000000 */
--text-secondary: #6e6e77;
--text-tertiary: #9a9aa3;

--accent-primary: #18181b;
--accent-primary-hover: #27272a;
--accent-primary-fg: #ffffff;
--bar-fill: #71717a;

--danger-text: #dc2626;       /* Revealed on hover or active error alerts */
--danger-bg-hover: #fef2f2;
--danger-border: rgba(220, 38, 38, 0.18);
--success-text: #15803d;
--success-bg: #f0fdf4;
--warning-text: #a16207;
--warning-bg: #fefce8;
```

### Colors (Dark — `.dark`)
```css
--bg-chrome: #111113;
--bg-sidebar: #111113;
--bg-canvas: #161618;
--bg-surface: #1c1c1f;
--bg-subtle: #1c1c1f;
--bg-hover: #232327;
--bg-selected: #2a2a2f;
--bg-track: #27272b;
--border-hairline: rgba(255, 255, 255, 0.07);
--text-primary: #f4f4f5;
--text-secondary: #a1a1aa;
--text-tertiary: #71717a;
--accent-primary: #f4f4f5;
--accent-primary-fg: #18181b;
--bar-fill: #a1a1aa;
```

### Radius
- Controls & ghost buttons: `8px` (`--radius-control`)
- Sidebar nav items & list rows: `9px`–`10px` (`--radius-nav`, `--radius-input`)
- Floating search, cards, popovers, toasts: `14px` (`--radius-card`)
- Modals & drag-drop overlay card: `16px` (`--radius-modal`)
- Pills, switches, circular icon buttons, 3px bars: `9999px` (`--radius-pill`)

### Elevation
- Default cards/panels/rows: `none`
- Floating search / popovers / toasts: `0 4px 16px rgba(0, 0, 0, 0.04)` (`--shadow-float`)
- Modals: `0 12px 36px rgba(0, 0, 0, 0.08), 0 0 0 1px rgba(0, 0, 0, 0.06)` (`--shadow-modal`)

### Typography & Icons
- Font: `"Segoe UI Variable Text", "Segoe UI", "Inter", "Microsoft YaHei UI", system-ui, sans-serif`
- Scale: `11px`–`11.5px` (headers/meta) · `12px`–`12.5px` (secondary/controls) · `13px` (body/rows) · `15px` (modal headings) · `17px` (page titles, `-0.015em` tracking)
- Weights: `400` (body/inactive), `500` (titles/names/active states — avoid heavy 700 bold)
- Numeric alignment: `.num-tabular` (`font-variant-numeric: tabular-nums`) mandatory on all versions, dates, sizes, and counts.
- Icons: Monochrome `lucide-react` icons strictly at `strokeWidth={1.5}`, default color `--text-tertiary`.

## Patterns

### Toolbar Ghost Button (`.ui-btn-ghost`)
- Height: `32px` (`h-8`) · Padding: `6px 10px` · Radius: `8px` · `12.5px / 500`
- Rest: transparent background, no border, `--text-secondary` text, `--text-tertiary` 1.5px icon
- Hover: `--bg-hover` (`#F2F2F4`) background, `--text-primary` text

### Primary Action Button (`.ui-btn-primary`)
- Height: `32px`–`34px` · Padding: `6px 14px` · Radius: `8px` · `12.5px / 500`
- Background: `--accent-primary` (`#18181B`), Text: `--accent-primary-fg` (`#FFFFFF`)

### Hover-Reveal Destructive Button (`.ui-btn-danger-ghost` / `.ui-btn-danger-subtle`)
- Rest: low-presence neutral `--text-secondary` (transparent or hairline `--bg-surface`)
- Hover: smooth `180ms` transition to `--danger-bg-hover` (`#FEF2F2`) + `--danger-text` (`#DC2626`)

### Sidebar Navigation Item
- Padding: `7px 10px` · Radius: `9px` · `13px`
- Active: `--bg-selected` (`#EAEAEF`) + `--text-primary` (`500` weight); NO left vertical indicator bar, NO colored count pills
- Counts: bare right-aligned `.num-tabular` muted text; bottom storage summary integrated as plain text without boxed cards

### Data List & Proportion Bar
- Borderless transparent table header (`11.5px / 500` `--text-tertiary`) + single `1px` `--border-hairline` bottom rule; header `pr-[30px]` paired with `[scrollbar-gutter:stable]` on scroll container
- Row hover actions (`Uninstall` / `Open Folder`) hidden at rest (`opacity-0`), fading in smoothly on `group-hover` / `group-focus-within`
- Size bar: `3px` height, `64px` width, `9999px` radius, logarithmic scale, `--bg-track` (`#EFEFF2`) track + `--bar-fill` (`#71717A`) fill

## Decisions

| Decision | Rationale | Date |
|----------|-----------|------|
| Strict zero-blue neutral palette | Aligns with Antigravity desktop craft; keeps user attention on native app icons and tabular data | 2026-11-25 |
| Custom CSS `.ui-checkbox` and charcoal switches | Eliminates Windows WebView2 native blue accent leaks across all settings and modals | 2026-11-25 |
| Inset `#FFFFFF` rounded-tl canvas inside `#F9F9FA` shell | Creates quiet structural separation without heavy chrome borders or drop shadows | 2026-11-25 |
| White squircle + Decouple monochrome mark (`Unbind`) | Replaces generic trash-can trope with a memorable GitHub-ready brand mark expressing clean software decoupling while preserving 0.1s recognition at 16px-256px | 2026-11-25 |
