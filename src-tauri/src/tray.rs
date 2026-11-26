use crate::commands::AppState;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use unbind_core::safety::is_process_elevated;

pub const TRAY_ID: &str = "unbind-tray";
pub const TRAY_WINDOW_LABEL: &str = "tray-menu";
pub const TRAY_LOGICAL_WIDTH: f64 = 264.0;
pub const TRAY_DEFAULT_LOGICAL_HEIGHT: f64 = 388.0;
const TRAY_MIN_LOGICAL_HEIGHT: f64 = 240.0;
const TRAY_MAX_LOGICAL_HEIGHT: f64 = 520.0;
const BLUR_TOGGLE_DEBOUNCE_MS: u64 = 240;
const SHOW_BLUR_GRACE_MS: u64 = 350;

const EN_LOCALE_JSON: &str = include_str!("../../src/i18n/locales/en.json");
const ZH_LOCALE_JSON: &str = include_str!("../../src/i18n/locales/zh-CN.json");

pub struct TrayRuntimeState {
    last_blur_ms: AtomicU64,
    last_show_ms: AtomicU64,
    logical_height_px: AtomicU32,
    exiting: AtomicBool,
}

impl Default for TrayRuntimeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TrayRuntimeState {
    pub fn new() -> Self {
        Self {
            last_blur_ms: AtomicU64::new(0),
            last_show_ms: AtomicU64::new(0),
            logical_height_px: AtomicU32::new(TRAY_DEFAULT_LOGICAL_HEIGHT as u32),
            exiting: AtomicBool::new(false),
        }
    }

    pub fn record_show(&self) {
        self.last_show_ms.store(now_epoch_ms(), Ordering::SeqCst);
    }

    pub fn was_shown_recently(&self) -> bool {
        let prev = self.last_show_ms.load(Ordering::SeqCst);
        if prev == 0 {
            return false;
        }
        now_epoch_ms().saturating_sub(prev) < SHOW_BLUR_GRACE_MS
    }

    pub fn record_blur(&self) {
        self.last_blur_ms.store(now_epoch_ms(), Ordering::SeqCst);
    }

    pub fn was_blurred_recently(&self) -> bool {
        let prev = self.last_blur_ms.load(Ordering::SeqCst);
        if prev == 0 {
            return false;
        }
        now_epoch_ms().saturating_sub(prev) < BLUR_TOGGLE_DEBOUNCE_MS
    }

    pub fn logical_height(&self) -> f64 {
        let h = self.logical_height_px.load(Ordering::SeqCst) as f64;
        h.clamp(TRAY_MIN_LOGICAL_HEIGHT, TRAY_MAX_LOGICAL_HEIGHT)
    }

    pub fn set_logical_height(&self, height: f64) -> f64 {
        let clamped = height.clamp(TRAY_MIN_LOGICAL_HEIGHT, TRAY_MAX_LOGICAL_HEIGHT).ceil();
        self.logical_height_px.store(clamped as u32, Ordering::SeqCst);
        clamped
    }

    pub fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    pub fn mark_exiting(&self) {
        self.exiting.store(true, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraySnapshot {
    pub app_version: String,
    pub is_elevated: bool,
    pub worker_connected: bool,
    pub portable_mode: bool,
    pub main_window_visible: bool,
    pub installed_count: usize,
    pub active_track_count: usize,
    pub language: String,
    pub resolved_locale: String,
    pub theme: String,
    pub close_to_tray: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayNavigatePayload {
    pub target: String,
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "windows")]
fn system_prefers_chinese() -> bool {
    use windows::Win32::Globalization::GetUserDefaultUILanguage;
    let lang_id = unsafe { GetUserDefaultUILanguage() };
    // Primary language ID 0x04 corresponds to LANG_CHINESE.
    (lang_id & 0x03ff) == 0x04
}

#[cfg(not(target_os = "windows"))]
fn system_prefers_chinese() -> bool {
    false
}

pub fn resolve_effective_locale(setting: &str) -> &'static str {
    match setting {
        "zh-CN" => "zh-CN",
        "en" => "en",
        _ => {
            if system_prefers_chinese() {
                "zh-CN"
            } else {
                "en"
            }
        }
    }
}

pub fn format_tray_tooltip(locale: &str, active_track_count: usize) -> String {
    let raw = if locale == "zh-CN" {
        ZH_LOCALE_JSON
    } else {
        EN_LOCALE_JSON
    };
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(raw) {
        if active_track_count > 0 {
            if let Some(tpl) = val
                .get("tray")
                .and_then(|t| t.get("tooltipTracking"))
                .and_then(|v| v.as_str())
            {
                return tpl.replace("{count}", &active_track_count.to_string());
            }
        }
        if let Some(tip) = val
            .get("tray")
            .and_then(|t| t.get("tooltip"))
            .and_then(|v| v.as_str())
        {
            return tip.to_string();
        }
    }
    "Unbind".to_string()
}

pub fn build_tray_snapshot(app: &AppHandle, state: &AppState) -> TraySnapshot {
    let storage = state.current_storage();
    let settings = storage.load_settings();
    let resolved = resolve_effective_locale(&settings.language).to_string();
    let installed_count = state
        .apps_cache
        .lock()
        .map(|list| {
            list.iter()
                .filter(|a| {
                    (settings.show_system_components || !a.is_system_component)
                        && (settings.show_windows_updates || !a.is_windows_update)
                })
                .count()
        })
        .unwrap_or(0);
    let active_track_count = state.tracker.active_sessions().len();
    let main_window_visible = app
        .get_webview_window("main")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);

    TraySnapshot {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        is_elevated: is_process_elevated(),
        worker_connected: state.elevation.is_worker_connected(),
        portable_mode: storage.portable_mode,
        main_window_visible,
        installed_count,
        active_track_count,
        language: settings.language,
        resolved_locale: resolved,
        theme: settings.theme,
        close_to_tray: settings.close_to_tray,
    }
}

pub fn update_tray_tooltip(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let settings = state.current_storage().load_settings();
    let locale = resolve_effective_locale(&settings.language);
    let active_tracks = state.tracker.active_sessions().len();
    let tip = format_tray_tooltip(locale, active_tracks);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tip));
    }
    let snapshot = build_tray_snapshot(app, &state);
    let _ = app.emit_to(TRAY_WINDOW_LABEL, "tray://state", &snapshot);
}

/// Pure positioning geometry for the custom tray popup window.
/// Clamps the popup inside the monitor work area so it never clips off-screen
/// or underneath the Windows taskbar, regardless of taskbar edge or DPI scale.
#[allow(clippy::too_many_arguments)]
pub fn compute_tray_popup_position(
    click_x: i32,
    click_y: i32,
    win_w: i32,
    win_h: i32,
    wa_x: i32,
    wa_y: i32,
    wa_w: u32,
    wa_h: u32,
    scale: f64,
) -> (i32, i32) {
    let wa_right = wa_x + wa_w as i32;
    let wa_bottom = wa_y + wa_h as i32;
    let edge_gap = (2.0 * scale).round() as i32;

    let max_x = (wa_right - win_w).max(wa_x);
    let x = (click_x - win_w / 2).clamp(wa_x, max_x);

    let max_y = (wa_bottom - win_h).max(wa_y);
    let raw_y = if click_y >= wa_bottom {
        // Bottom taskbar: sit directly above the work area bottom edge.
        wa_bottom - win_h - edge_gap
    } else if click_y <= wa_y {
        // Top taskbar: sit directly below the work area top edge.
        wa_y + edge_gap
    } else if click_y > wa_y + (wa_h as i32) / 2 {
        // Lower half (e.g. Windows 11 hidden-tray overflow flyout).
        click_y - win_h - edge_gap
    } else {
        // Upper half.
        click_y + edge_gap
    };
    let y = raw_y.clamp(wa_y, max_y);

    (x, y)
}

fn ensure_tray_window(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    if let Some(win) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        return Ok(win);
    }

    let initial_height = app
        .try_state::<AppState>()
        .map(|s| s.tray.logical_height())
        .unwrap_or(TRAY_DEFAULT_LOGICAL_HEIGHT);

    let win = WebviewWindowBuilder::new(
        app,
        TRAY_WINDOW_LABEL,
        WebviewUrl::App("index.html#tray".into()),
    )
    .title("Unbind Tray")
    .inner_size(TRAY_LOGICAL_WIDTH, initial_height)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible(false)
    .build()?;

    let app_handle = app.clone();
    win.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            if let Some(state) = app_handle.try_state::<AppState>() {
                if state.tray.was_shown_recently() {
                    return;
                }
                state.tray.record_blur();
            }
            if let Some(w) = app_handle.get_webview_window(TRAY_WINDOW_LABEL) {
                let _ = w.hide();
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            let exiting = app_handle
                .try_state::<AppState>()
                .map(|s| s.tray.is_exiting())
                .unwrap_or(false);
            if !exiting {
                api.prevent_close();
                if let Some(w) = app_handle.get_webview_window(TRAY_WINDOW_LABEL) {
                    let _ = w.hide();
                }
            }
        }
        _ => {}
    });

    Ok(win)
}

#[cfg(target_os = "windows")]
fn activate_tray_foreground(win: &WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };
    if let Ok(raw) = win.hwnd() {
        let hwnd = HWND(raw.0);
        unsafe {
            let fg = GetForegroundWindow();
            let fg_tid = GetWindowThreadProcessId(fg, None);
            let cur_tid = GetCurrentThreadId();
            let attached = fg_tid != 0
                && fg_tid != cur_tid
                && windows::Win32::System::Threading::AttachThreadInput(fg_tid, cur_tid, true)
                    .as_bool();
            let _ = SetForegroundWindow(hwnd);
            if attached {
                let _ =
                    windows::Win32::System::Threading::AttachThreadInput(fg_tid, cur_tid, false);
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn activate_tray_foreground(_win: &WebviewWindow) {}

pub fn hide_tray_menu(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        let _ = win.hide();
    }
}

pub fn show_main_window(app: &AppHandle) {
    hide_tray_menu(app);
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

pub fn show_tray_menu_at(app: &AppHandle, anchor: Option<(i32, i32)>) {
    let Ok(win) = ensure_tray_window(app) else {
        return;
    };

    let logical_h = app
        .try_state::<AppState>()
        .map(|s| s.tray.logical_height())
        .unwrap_or(TRAY_DEFAULT_LOGICAL_HEIGHT);
    let _ = win.set_size(LogicalSize::new(TRAY_LOGICAL_WIDTH, logical_h));

    // Find the monitor holding the click anchor (or primary monitor for preview).
    let target_monitor = anchor
        .and_then(|(cx, cy)| {
            win.available_monitors().ok().and_then(|list| {
                list.into_iter().find(|m| {
                    let mp = m.position();
                    let ms = m.size();
                    cx >= mp.x
                        && cx < mp.x + ms.width as i32
                        && cy >= mp.y
                        && cy < mp.y + ms.height as i32
                })
            })
        })
        .or_else(|| win.primary_monitor().ok().flatten())
        .or_else(|| win.current_monitor().ok().flatten());

    if let Some(monitor) = target_monitor {
        let scale = monitor.scale_factor();
        let wa = monitor.work_area();
        let win_w = (TRAY_LOGICAL_WIDTH * scale).round() as i32;
        let win_h = (logical_h * scale).round() as i32;

        let (click_x, click_y) = match anchor {
            Some((cx, cy)) => (cx, cy),
            None => (
                wa.position.x + wa.size.width as i32 - (12.0 * scale).round() as i32,
                wa.position.y + wa.size.height as i32 + (4.0 * scale).round() as i32,
            ),
        };

        let (pos_x, pos_y) = compute_tray_popup_position(
            click_x,
            click_y,
            win_w,
            win_h,
            wa.position.x,
            wa.position.y,
            wa.size.width,
            wa.size.height,
            scale,
        );
        let _ = win.set_position(PhysicalPosition::new(pos_x, pos_y));
    }

    if let Some(state) = app.try_state::<AppState>() {
        state.tray.record_show();
        let snapshot = build_tray_snapshot(app, &state);
        let _ = app.emit_to(TRAY_WINDOW_LABEL, "tray://state", &snapshot);
    }
    let _ = app.emit_to(TRAY_WINDOW_LABEL, "tray://open", now_epoch_ms());

    let _ = win.show();
    activate_tray_foreground(&win);
    let _ = win.set_focus();
}

pub fn toggle_tray_menu_at(app: &AppHandle, click_x: i32, click_y: i32) {
    if let Some(state) = app.try_state::<AppState>() {
        if state.tray.was_blurred_recently() {
            hide_tray_menu(app);
            return;
        }
    }
    if let Some(win) = app.get_webview_window(TRAY_WINDOW_LABEL) {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
            return;
        }
    }
    show_tray_menu_at(app, Some((click_x, click_y)));
}

pub fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let app_handle = app.handle();

    // Pre-create hidden tray popup window so first right-click opens instantaneously.
    let _ = ensure_tray_window(app_handle)?;

    let initial_tooltip = app_handle
        .try_state::<AppState>()
        .map(|state| {
            let settings = state.current_storage().load_settings();
            let locale = resolve_effective_locale(&settings.language);
            format_tray_tooltip(locale, 0)
        })
        .unwrap_or_else(|| "Unbind".to_string());

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(initial_tooltip)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    show_main_window(app);
                }
                TrayIconEvent::Click {
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    position,
                    ..
                } => {
                    toggle_tray_menu_at(
                        app,
                        position.x.round() as i32,
                        position.y.round() as i32,
                    );
                }
                _ => {}
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let _tray = builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_position_clamps_right_edge_above_bottom_taskbar() {
        // 1920x1080 screen with 48px bottom taskbar -> work area 1920x1032
        let (x, y) = compute_tray_popup_position(1880, 1055, 264, 380, 0, 0, 1920, 1032, 1.0);
        assert_eq!(x, 1920 - 264);
        assert_eq!(y, 1032 - 380 - 2);
    }

    #[test]
    fn tray_position_handles_hidpi_scale_and_top_taskbar() {
        // 2560x1440 screen @ 1.5x with 60px top taskbar -> work area y=60, h=1380
        let (x, y) = compute_tray_popup_position(2400, 30, 396, 570, 0, 60, 2560, 1380, 1.5);
        assert_eq!(x, 2560 - 396);
        assert_eq!(y, 60 + 3);
    }

    #[test]
    fn tray_position_handles_negative_multimonitor_coords() {
        // Secondary monitor to the left: x=-1920..0, work area height 1032
        let (x, y) = compute_tray_popup_position(-960, 1060, 264, 380, -1920, 0, 1920, 1032, 1.0);
        assert_eq!(x, -960 - 132);
        assert_eq!(y, 1032 - 380 - 2);
    }

    #[test]
    fn bilingual_tooltip_reads_from_locale_json() {
        let en = format_tray_tooltip("en", 0);
        let zh = format_tray_tooltip("zh-CN", 0);
        assert!(en.contains("Unbind"));
        assert!(zh.contains("Unbind"));
        assert_ne!(en, zh);

        let en_track = format_tray_tooltip("en", 2);
        let zh_track = format_tray_tooltip("zh-CN", 2);
        assert!(en_track.contains('2'));
        assert!(zh_track.contains('2'));
        assert_ne!(en_track, zh_track);
    }
}
