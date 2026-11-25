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
- **Emblem Geometry (Decouple):** Twin point-symmetric architectural U-links rotated `-45deg`, cleanly offset along the diagonal release axis (`62px` negative-space channel on 1024 grid) -- deep charcoal primary link (`#27272A` -> `#141417`) and refined graphite secondary link (`#7B7B85` -> `#52525B`), expressing zero-trace software decoupling without generic trash-bin tropes.
- **In-App Mark (`BrandLogo`):** Bare `17px x 17px` monochrome SVG (`currentColor`, `strokeWidth="2.35"`, secondary link at `opacity="0.52"`) adapting seamlessly to Light and Dark themes.

## Color Tokens

Canonical source: `src/index.css`

### Light (`:root`)
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

### Dark (`.dark`)
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

## Geometry, Spacing & Elevation

- **Spacing Base:** `4px` (`4, 6, 8, 12, 16, 20, 24`)
- **Layout Metrics:** Virtualized app row `62px`, Sidebar `222px`, Inspector drawer `312px`
- **Radii:** Controls & ghost buttons `8px`, nav items & inputs `9px-10px`, cards/popovers/floating search `14px`, modals & drop overlay `16px`, pills/switches/bars `9999px`
- **Elevation:** Default cards/rows `none`; floating search/popovers/toasts `0 4px 16px rgba(0,0,0,0.04)`; modals `0 12px 36px rgba(0,0,0,0.08), 0 0 0 1px rgba(0,0,0,0.06)`

## Typography & Iconography

- **Font Stack:** `"Segoe UI Variable Text", "Segoe UI", "Inter", "Microsoft YaHei UI", system-ui, sans-serif`
- **Scale:** `11px-11.5px` headers/meta, `12px-12.5px` controls/secondary, `13px` body/rows, `15px` modal headings, `17px` page titles (`-0.015em` tracking)
- **Weights:** `400` body/inactive, `500` titles/names/active states (avoid heavy 700 bold)
- **Numeric Discipline:** `.num-tabular` (`font-variant-numeric: tabular-nums`) mandatory on all versions, dates, sizes, counts, and PIDs
- **Icons:** Monochrome `lucide-react` icons strictly at `strokeWidth={1.5}`, default color `--text-tertiary`

## Core Surface Patterns

- **Toolbar Ghost Button (`.ui-btn-ghost`):** `32px` height, `8px` radius, transparent at rest, `--bg-hover` on hover
- **Primary Action Button (`.ui-btn-primary`):** Deep charcoal `--accent-primary` with `--accent-primary-fg`
- **Hover-Reveal Destructive Buttons (`.ui-btn-danger-ghost` / `.ui-btn-danger-subtle`):** Neutral at rest, `180ms` ease-out wash to `--danger-bg-hover` + `--danger-text` on hover
- **Custom Checkbox (`.ui-checkbox`):** Pure monochrome charcoal check & indeterminate dash (zero OS blue accent)
- **Sortable Borderless Table Header:** Transparent header + single `1px` `--border-hairline` rule; clickable column headers highlight active field in `--text-primary` with `12px` `1.5px`-stroke sort arrow
- **Confirmation & Preview Modals (`ConfirmDialog`, `BackupPreviewModal`, `DropActionModal`):** Replace all native browser `window.confirm` dialogs; `16px` radius, `Esc` dismissible, neutral confirmation footer with hover-reveal destructive button
- **Settings Auto-Save Pattern:** Every toggle and segmented switch persists immediately without a redundant Save button, surfacing a subtle quiet `Auto-saved` checkmark in the page header
