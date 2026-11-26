import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Activity,
  AppWindow,
  Hammer,
  Layers,
  Monitor,
  Moon,
  Power,
  Settings,
  Sun,
  Wrench,
} from "lucide-react";
import { api, isTauriRuntime } from "../api/client";
import { getDict, resolveLocale } from "../i18n";
import type { AppSettings, TraySnapshot } from "../types";
import { BrandLogo } from "./BrandLogo";

const DEFAULT_SNAPSHOT: TraySnapshot = {
  appVersion: "0.5.1",
  isElevated: false,
  workerConnected: false,
  portableMode: false,
  mainWindowVisible: true,
  installedCount: 0,
  activeTrackCount: 0,
  language: "auto",
  resolvedLocale: "en",
  theme: "system",
  closeToTray: false,
};

export function TrayMenuApp() {
  const [snapshot, setSnapshot] = useState<TraySnapshot>(DEFAULT_SNAPSHOT);
  const [openSeq, setOpenSeq] = useState(0);
  const shellRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  const locale = useMemo(() => resolveLocale(snapshot.language), [snapshot.language]);
  const dict = useMemo(() => getDict(locale), [locale]);

  // Keep Dark / Light / System theme synced on <html>
  useEffect(() => {
    const apply = () => {
      const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      const isDark =
        snapshot.theme === "dark" || (snapshot.theme === "system" && prefersDark);
      document.documentElement.classList.toggle("dark", isDark);
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [snapshot.theme]);

  // Initial snapshot load + live Rust event sync
  useEffect(() => {
    void api.getTraySnapshot().then(setSnapshot).catch(() => {});

    if (!isTauriRuntime()) return;
    const unsubs: Array<() => void> = [];

    void listen<TraySnapshot>("tray://state", (event) => {
      if (event.payload) {
        setSnapshot(event.payload);
      }
    }).then((u) => unsubs.push(u));

    void listen<AppSettings>("settings://changed", (event) => {
      const cfg = event.payload;
      if (!cfg) return;
      setSnapshot((prev) => ({
        ...prev,
        language: cfg.language,
        theme: cfg.theme,
        portableMode: cfg.portableMode,
        closeToTray: Boolean(cfg.closeToTray),
      }));
    }).then((u) => unsubs.push(u));

    void listen<number>("tray://open", () => {
      setOpenSeq((n) => n + 1);
      if (document.activeElement instanceof HTMLElement) {
        document.activeElement.blur();
      }
    }).then((u) => unsubs.push(u));

    return () => {
      unsubs.forEach((fn) => fn());
    };
  }, []);

  // Measure natural DOM height and keep frameless window sized tightly with zero scrollbars
  useEffect(() => {
    const el = shellRef.current;
    if (!el || !isTauriRuntime()) return;

    let lastHeight = 0;
    const reportSize = () => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h > 100 && h !== lastHeight) {
        lastHeight = h;
        void api.resizeTrayMenu(h).catch(() => {});
      }
    };

    reportSize();
    const ro = new ResizeObserver(reportSize);
    ro.observe(el);
    return () => ro.disconnect();
  }, [locale, snapshot.activeTrackCount, snapshot.mainWindowVisible]);

  // Full keyboard navigation inside tray menu (ArrowUp / ArrowDown / Home / End / Escape)
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        void api.hideTrayMenu();
        return;
      }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) {
        return;
      }
      const root = menuRef.current;
      if (!root) return;
      const items = Array.from(
        root.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]'),
      );
      if (items.length === 0) return;

      e.preventDefault();
      const idx = items.indexOf(document.activeElement as HTMLButtonElement);
      let nextIdx = 0;
      if (e.key === "ArrowDown") {
        nextIdx = idx < 0 ? 0 : (idx + 1) % items.length;
      } else if (e.key === "ArrowUp") {
        nextIdx = idx <= 0 ? items.length - 1 : idx - 1;
      } else if (e.key === "Home") {
        nextIdx = 0;
      } else if (e.key === "End") {
        nextIdx = items.length - 1;
      }
      items[nextIdx]?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const runAction = useCallback((action: string) => {
    if (action.startsWith("setLanguage:")) {
      const lang = action.slice("setLanguage:".length);
      setSnapshot((prev) => ({ ...prev, language: lang, resolvedLocale: lang }));
    } else if (action.startsWith("setTheme:")) {
      const theme = action.slice("setTheme:".length);
      setSnapshot((prev) => ({ ...prev, theme }));
    }
    void api.executeTrayAction(action).catch(() => {});
  }, []);

  const isPriv = snapshot.isElevated || snapshot.workerConnected;
  const trackingLabel = dict.tray.trackingBadge.replace(
    "{count}",
    String(snapshot.activeTrackCount),
  );

  return (
    <div
      ref={shellRef}
      onContextMenu={(e) => e.preventDefault()}
      className="tray-shell select-none"
    >
      <div
        key={openSeq}
        ref={menuRef}
        role="menu"
        aria-label={dict.appTitle}
        className="tray-popover"
      >
        {/* Brand & live status header */}
        <div className="tray-header">
          <div className="flex min-w-0 items-center gap-2">
            <span className="flex h-5 w-5 shrink-0 items-center justify-center text-[var(--text-primary)]">
              <BrandLogo className="h-[16px] w-[16px]" />
            </span>
            <div className="min-w-0">
              <div className="flex items-center gap-1.5">
                <span className="truncate text-[13px] font-medium tracking-[-0.01em] text-[var(--text-primary)]">
                  {dict.appTitle}
                </span>
                {snapshot.portableMode && (
                  <span className="rounded-full bg-[var(--bg-hover)] px-1.5 py-[1px] text-[10px] font-medium text-[var(--text-secondary)]">
                    {dict.status.portable}
                  </span>
                )}
              </div>
              {!snapshot.mainWindowVisible && (
                <div className="text-[10.5px] leading-tight text-[var(--text-tertiary)]">
                  {dict.tray.hiddenHint}
                </div>
              )}
            </div>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            {snapshot.activeTrackCount > 0 ? (
              <span className="num-tabular inline-flex items-center gap-1 rounded-full border border-[var(--success-border)] bg-[var(--success-bg)] px-2 py-0.5 text-[10.5px] font-medium text-[var(--success-text)]">
                <span className="h-1.5 w-1.5 rounded-full bg-[var(--success-text)] animate-pulse" />
                {trackingLabel}
              </span>
            ) : (
              <>
                {isPriv && (
                  <span
                    title={dict.status.elevated}
                    className="h-1.5 w-1.5 rounded-full bg-[var(--success-text)]"
                  />
                )}
                <span className="num-tabular text-[11px] text-[var(--text-tertiary)]">
                  v{snapshot.appVersion}
                </span>
              </>
            )}
          </div>
        </div>

        {/* Primary window action */}
        <div className="tray-group">
          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("showMain")}
            className="tray-item"
          >
            <span className="flex min-w-0 items-center gap-2">
              <AppWindow className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.openMain}</span>
            </span>
          </button>
        </div>

        <div className="tray-divider" />

        {/* Quick-jump navigation actions */}
        <div className="tray-group">
          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("nav:apps")}
            className="tray-item"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Layers className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.apps}</span>
            </span>
            {snapshot.installedCount > 0 && (
              <span className="num-tabular text-[11px] font-normal text-[var(--text-tertiary)]">
                {snapshot.installedCount}
              </span>
            )}
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("action:forceRemove")}
            className="tray-item"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Hammer className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.forceRemove}</span>
            </span>
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("nav:tracker")}
            className="tray-item"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Activity className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.tracker}</span>
            </span>
            {snapshot.activeTrackCount > 0 && (
              <span className="flex items-center gap-1">
                <span className="h-1.5 w-1.5 rounded-full bg-[var(--success-text)] animate-pulse" />
                <span className="num-tabular text-[11px] font-normal text-[var(--success-text)]">
                  {snapshot.activeTrackCount}
                </span>
              </span>
            )}
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("nav:tools")}
            className="tray-item"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Wrench className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.tools}</span>
            </span>
          </button>

          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("nav:settings")}
            className="tray-item"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Settings className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.settings}</span>
            </span>
          </button>
        </div>

        <div className="tray-divider" />

        {/* Instant synchronized bilingual & theme controls */}
        <div className="tray-controls-section">
          <div className="tray-control-row">
            <span className="text-[11.5px] text-[var(--text-secondary)]">
              {dict.tray.language}
            </span>
            <div className="tray-segment-track" role="group" aria-label={dict.tray.language}>
              <button
                type="button"
                data-active={locale === "en"}
                aria-pressed={locale === "en"}
                onClick={() => runAction("setLanguage:en")}
                className="tray-segment-btn"
              >
                {dict.tray.langEn}
              </button>
              <button
                type="button"
                data-active={locale === "zh-CN"}
                aria-pressed={locale === "zh-CN"}
                onClick={() => runAction("setLanguage:zh-CN")}
                className="tray-segment-btn"
              >
                {dict.tray.langZh}
              </button>
            </div>
          </div>

          <div className="tray-control-row">
            <span className="text-[11.5px] text-[var(--text-secondary)]">
              {dict.tray.theme}
            </span>
            <div className="tray-segment-track" role="group" aria-label={dict.tray.theme}>
              <button
                type="button"
                title={dict.settings.themeSystem}
                aria-label={dict.settings.themeSystem}
                data-active={snapshot.theme === "system"}
                aria-pressed={snapshot.theme === "system"}
                onClick={() => runAction("setTheme:system")}
                className="tray-segment-btn w-[26px] px-0"
              >
                <Monitor className="h-3 w-3" strokeWidth={1.5} />
              </button>
              <button
                type="button"
                title={dict.settings.themeLight}
                aria-label={dict.settings.themeLight}
                data-active={snapshot.theme === "light"}
                aria-pressed={snapshot.theme === "light"}
                onClick={() => runAction("setTheme:light")}
                className="tray-segment-btn w-[26px] px-0"
              >
                <Sun className="h-3 w-3" strokeWidth={1.5} />
              </button>
              <button
                type="button"
                title={dict.settings.themeDark}
                aria-label={dict.settings.themeDark}
                data-active={snapshot.theme === "dark"}
                aria-pressed={snapshot.theme === "dark"}
                onClick={() => runAction("setTheme:dark")}
                className="tray-segment-btn w-[26px] px-0"
              >
                <Moon className="h-3 w-3" strokeWidth={1.5} />
              </button>
            </div>
          </div>
        </div>

        <div className="tray-divider" />

        {/* Hover-reveal destructive Quit action */}
        <div className="tray-group">
          <button
            type="button"
            role="menuitem"
            onClick={() => runAction("quit")}
            className="tray-item tray-item-danger"
          >
            <span className="flex min-w-0 items-center gap-2">
              <Power className="tray-item-icon" strokeWidth={1.5} />
              <span className="truncate">{dict.tray.quit}</span>
            </span>
          </button>
        </div>
      </div>
    </div>
  );
}
