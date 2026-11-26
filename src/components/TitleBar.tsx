import { useEffect, useState } from "react";
import {
  ArrowUpCircle,
  HardDrive,
  Languages,
  Loader2,
  Maximize2,
  Minimize2,
  Minus,
  Monitor,
  Moon,
  RotateCw,
  Shield,
  Sun,
  X,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict, Locale } from "../i18n";
import type { SystemStatus, UpdateCheckResult, UpdateProgressEvent } from "../types";
import { BrandLogo } from "./BrandLogo";

interface TitleBarProps {
  dict: Dict;
  locale: Locale;
  theme: string;
  status: SystemStatus | null;
  updateCheck: UpdateCheckResult | null;
  updateProgress: UpdateProgressEvent;
  onElevate: () => void;
  onToggleLocale: () => void;
  onCycleTheme: () => void;
  onOpenUpdates: () => void;
  onRestartToUpdate: () => void;
}

export function TitleBar({
  dict,
  locale,
  theme,
  status,
  updateCheck,
  updateProgress,
  onElevate,
  onToggleLocale,
  onCycleTheme,
  onOpenUpdates,
  onRestartToUpdate,
}: TitleBarProps) {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const syncMax = () => {
      api.windowControls.isMaximized().then(setMaximized).catch(() => {});
    };
    syncMax();

    let unlisten: (() => void) | undefined;
    void api.windowControls.onResized(syncMax).then((fn) => {
      unlisten = fn;
    });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const isPriv = Boolean(status?.isElevated || status?.workerConnected);
  const themeLabel =
    theme === "dark"
      ? dict.settings.themeDark
      : theme === "light"
        ? dict.settings.themeLight
        : dict.settings.themeSystem;
  const updateStage = updateProgress.stage;
  const showUpdatePill =
    updateStage === "downloading" ||
    updateStage === "verifying" ||
    updateStage === "ready" ||
    updateStage === "installing" ||
    Boolean(updateCheck?.hasUpdate);
  const updateVersion = updateProgress.version || updateCheck?.latestVersion || "";
  const updatePercent = Math.round(updateProgress.percent);

  return (
    <header
      data-tauri-drag-region
      className="flex h-10 shrink-0 items-center justify-between bg-[var(--bg-chrome)] pr-1.5 pl-4 select-none"
    >
      <div data-tauri-drag-region className="flex items-center gap-2">
        <span className="flex h-5 w-5 items-center justify-center text-[var(--text-primary)]">
          <BrandLogo className="h-[17px] w-[17px]" />
        </span>
        <span
          data-tauri-drag-region
          className="text-[13px] font-medium tracking-[-0.01em] text-[var(--text-primary)]"
        >
          {dict.appTitle}
        </span>
        {status?.portableMode && (
          <span className="inline-flex items-center gap-1 rounded-full bg-[var(--bg-hover)] px-2 py-0.5 text-[10.5px] font-medium text-[var(--text-secondary)]">
            <HardDrive className="h-2.5 w-2.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
            {dict.status.portable}
          </span>
        )}
      </div>

      <div className="flex items-center gap-0.5">
        {showUpdatePill && (
          updateStage === "ready" ? (
            <button
              type="button"
              onClick={onRestartToUpdate}
              title={dict.settings.updateReadyHint}
              className="mr-1 inline-flex h-6.5 items-center gap-1.5 rounded-full bg-[var(--accent-primary)] px-2.5 text-[11px] font-medium text-[var(--accent-primary-fg)] transition-colors duration-150 hover:bg-[var(--accent-primary-hover)] cursor-pointer"
            >
              <RotateCw className="h-3 w-3" strokeWidth={1.5} />
              <span>{dict.settings.titlebarUpdateReady}</span>
            </button>
          ) : updateStage === "installing" ? (
            <span className="mr-1 inline-flex h-6.5 items-center gap-1.5 rounded-full bg-[var(--bg-hover)] px-2.5 text-[11px] font-medium text-[var(--text-secondary)]">
              <Loader2 className="h-3 w-3 animate-spin" strokeWidth={1.5} />
              <span>{dict.settings.restartingToUpdate}</span>
            </span>
          ) : updateStage === "downloading" || updateStage === "verifying" ? (
            <button
              type="button"
              onClick={onOpenUpdates}
              className="mr-1 inline-flex h-6.5 items-center gap-1.5 rounded-full border border-[var(--border-hairline)] bg-[var(--bg-surface)] px-2.5 text-[11px] font-medium text-[var(--text-primary)] transition-colors duration-150 hover:bg-[var(--bg-hover)] cursor-pointer"
            >
              <Loader2 className="h-3 w-3 animate-spin text-[var(--text-tertiary)]" strokeWidth={1.5} />
              <span className="num-tabular">
                {updateStage === "verifying"
                  ? dict.settings.titlebarUpdating.replace("{percent}", "99")
                  : dict.settings.titlebarUpdating.replace("{percent}", String(updatePercent))}
              </span>
            </button>
          ) : (
            <button
              type="button"
              onClick={onOpenUpdates}
              className="mr-1 inline-flex h-6.5 items-center gap-1.5 rounded-full border border-[var(--border-hairline)] bg-[var(--bg-surface)] px-2.5 text-[11px] font-medium text-[var(--text-primary)] transition-colors duration-150 hover:bg-[var(--bg-hover)] cursor-pointer"
            >
              <ArrowUpCircle className="h-3 w-3 text-[var(--text-secondary)]" strokeWidth={1.5} />
              <span className="num-tabular">
                {dict.settings.titlebarUpdateNew.replace("{version}", updateVersion)}
              </span>
            </button>
          )
        )}

        <button
          type="button"
          onClick={onElevate}
          title={dict.status.elevateTooltip}
          className="ui-btn-ghost mr-0.5 h-7 rounded-full px-2.5 text-[11.5px]"
        >
          {isPriv ? (
            <span className="h-1.5 w-1.5 rounded-full bg-[var(--success-text)]" />
          ) : (
            <Shield className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          )}
          <span className={isPriv ? "text-[var(--text-primary)]" : "text-[var(--text-secondary)]"}>
            {isPriv ? dict.status.elevated : dict.status.standard}
          </span>
        </button>

        <button
          type="button"
          onClick={onToggleLocale}
          title={dict.settings.langLabel}
          className="ui-btn-ghost h-7 px-2 text-[11.5px]"
        >
          <Languages className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          <span className="num-tabular">{locale === "zh-CN" ? "ZH" : "EN"}</span>
        </button>

        <button
          type="button"
          onClick={onCycleTheme}
          title={`${dict.settings.themeLabel}: ${themeLabel}`}
          aria-label={`${dict.settings.themeLabel}: ${themeLabel}`}
          className="ui-btn-ghost h-7 w-7 rounded-full p-0"
        >
          {theme === "dark" ? (
            <Moon className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          ) : theme === "light" ? (
            <Sun className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          ) : (
            <Monitor className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          )}
        </button>

        <div className="mx-1.5 h-3.5 w-px bg-[var(--border-hairline)]" />

        <button
          type="button"
          onClick={() => api.windowControls.minimize()}
          aria-label="Minimize"
          className="inline-flex h-7 w-8 items-center justify-center rounded-[8px] text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] cursor-pointer"
        >
          <Minus className="h-3.5 w-3.5" strokeWidth={1.5} />
        </button>
        <button
          type="button"
          onClick={async () => {
            await api.windowControls.toggleMaximize();
            setMaximized(await api.windowControls.isMaximized());
          }}
          aria-label="Maximize"
          className="inline-flex h-7 w-8 items-center justify-center rounded-[8px] text-[var(--text-secondary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] cursor-pointer"
        >
          {maximized ? (
            <Minimize2 className="h-3.5 w-3.5" strokeWidth={1.5} />
          ) : (
            <Maximize2 className="h-3.5 w-3.5" strokeWidth={1.5} />
          )}
        </button>
        <button
          type="button"
          onClick={() => api.windowControls.close()}
          aria-label="Close"
          className="inline-flex h-7 w-8 items-center justify-center rounded-[8px] text-[var(--text-secondary)] transition-colors hover:bg-[var(--danger-bg-hover)] hover:text-[var(--danger-text)] cursor-pointer"
        >
          <X className="h-3.5 w-3.5" strokeWidth={1.5} />
        </button>
      </div>
    </header>
  );
}
