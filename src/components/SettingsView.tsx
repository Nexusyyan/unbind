import { useEffect, useState } from "react";
import {
  AppWindow,
  Check,
  ExternalLink,
  FolderOpen,
  FolderPlus,
  HardDrive,
  Loader2,
  MousePointerClick,
  RefreshCw,
  Shield,
  X,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict, Locale } from "../i18n";
import type {
  AppSettings,
  ShellIntegrationStatus,
  SystemStatus,
  TrackMode,
  UpdateCheckResult,
} from "../types";
import { BrandLogo } from "./BrandLogo";

interface SettingsViewProps {
  dict: Dict;
  locale: Locale;
  settings: AppSettings;
  status: SystemStatus | null;
  onSave: (next: AppSettings) => Promise<void>;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function SettingsView({
  dict,
  locale,
  settings,
  status,
  onSave,
  onNotify,
}: SettingsViewProps) {
  const [saving, setSaving] = useState(false);
  const [justSaved, setJustSaved] = useState(false);

  const [shellStatus, setShellStatus] = useState<ShellIntegrationStatus>({
    forceRemoveEnabled: false,
    trackInstallEnabled: false,
    registeredExe: "",
  });
  const [shellBusy, setShellBusy] = useState(false);

  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateResult, setUpdateResult] = useState<UpdateCheckResult | null>(null);

  useEffect(() => {
    void api
      .getShellIntegrationStatus()
      .then(setShellStatus)
      .catch(() => {});
  }, []);

  const updateSettings = async (patch: Partial<AppSettings>) => {
    const next: AppSettings = { ...settings, ...patch };
    setSaving(true);
    setJustSaved(false);
    try {
      await onSave(next);
      setJustSaved(true);
    } finally {
      setSaving(false);
    }
  };

  const handleToggleShell = async (forceRemove: boolean, trackInstall: boolean) => {
    setShellBusy(true);
    try {
      const updated = await api.setShellIntegration(forceRemove, trackInstall, locale);
      setShellStatus(updated);
      onNotify("success", dict.settings.shellUpdated);
    } catch (err) {
      onNotify("error", dict.settings.shellTitle, String(err));
    } finally {
      setShellBusy(false);
    }
  };

  const handleAddTrackRoot = async () => {
    const picked = await api.pickFolder();
    if (!picked) return;
    const current = settings.customTrackRoots ?? [];
    if (current.some((r) => r.toLowerCase() === picked.toLowerCase())) return;
    await updateSettings({ customTrackRoots: [...current, picked] });
  };

  const handleRemoveTrackRoot = async (root: string) => {
    const current = settings.customTrackRoots ?? [];
    await updateSettings({ customTrackRoots: current.filter((r) => r !== root) });
  };

  const handleAddPortableRoot = async () => {
    const picked = await api.pickFolder();
    if (!picked) return;
    const current = settings.portableAppRoots ?? [];
    if (current.some((r) => r.toLowerCase() === picked.toLowerCase())) return;
    await updateSettings({ portableAppRoots: [...current, picked] });
  };

  const handleRemovePortableRoot = async (root: string) => {
    const current = settings.portableAppRoots ?? [];
    await updateSettings({ portableAppRoots: current.filter((r) => r !== root) });
  };

  const handleCheckUpdates = async () => {
    setCheckingUpdate(true);
    try {
      const res = await api.checkForUpdates();
      setUpdateResult(res);
    } catch (err) {
      onNotify("error", dict.settings.checkUpdates, String(err));
    } finally {
      setCheckingUpdate(false);
    }
  };

  const themeOptions: Array<{ value: string; label: string }> = [
    { value: "system", label: dict.settings.themeSystem },
    { value: "light", label: dict.settings.themeLight },
    { value: "dark", label: dict.settings.themeDark },
  ];

  const langOptions: Array<{ value: string; label: string }> = [
    { value: "auto", label: dict.settings.langAuto },
    { value: "zh-CN", label: dict.settings.langZh },
    { value: "en", label: "English" },
  ];

  const trackRoots = settings.customTrackRoots ?? [];
  const portableRoots = settings.portableAppRoots ?? [];
  const defaultTrackMode: TrackMode = settings.defaultTrackMode ?? "hybridSnapshot";

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-y-auto bg-[var(--bg-canvas)] px-6 pt-5 pb-10">
      <div className="w-full max-w-[780px] space-y-5">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-[17px] font-medium tracking-[-0.015em] text-[var(--text-primary)]">
              {dict.settings.title}
            </h1>
            <p className="mt-0.5 text-[12.5px] text-[var(--text-secondary)]">
              {dict.settings.subtitle}
            </p>
          </div>
          <div className="flex h-8 items-center gap-1.5 text-[12px] text-[var(--text-tertiary)]">
            {saving ? (
              <>
                <Loader2 className="h-3.5 w-3.5 animate-spin" strokeWidth={1.5} />
                <span>{dict.settings.saving}</span>
              </>
            ) : justSaved ? (
              <>
                <Check className="h-3.5 w-3.5 text-[var(--success-text)]" strokeWidth={1.5} />
                <span>{dict.settings.autoSaved}</span>
              </>
            ) : null}
          </div>
        </div>

        {/* Portable Mode */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="flex items-start justify-between gap-4">
            <div className="flex items-start gap-3">
              <HardDrive
                className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <div>
                <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
                  {dict.settings.portableTitle}
                </div>
                <p className="mt-0.5 text-[12px] leading-relaxed text-[var(--text-secondary)]">
                  {dict.settings.portableDesc}
                </p>
              </div>
            </div>
            <SwitchControl
              checked={settings.portableMode}
              onChange={(v) => void updateSettings({ portableMode: v })}
              ariaLabel={dict.settings.portableTitle}
            />
          </div>

          <div className="flex items-center justify-between gap-3 rounded-[10px] bg-[var(--bg-subtle)] px-3.5 py-2.5 text-[11.5px]">
            <div className="min-w-0 truncate">
              <span className="text-[var(--text-tertiary)]">{dict.settings.dataDirLabel}: </span>
              <span className="font-mono text-[var(--text-primary)] select-text">
                {settings.dataDir}
              </span>
            </div>
            <button
              type="button"
              onClick={() => api.openPathInExplorer(settings.dataDir)}
              className="ui-btn-ghost h-6.5 shrink-0 px-2 text-[11.5px]"
            >
              <FolderOpen
                className="h-3 w-3 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              {dict.common.openDir}
            </button>
          </div>
        </section>

        {/* System Tray & Window Behavior */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div className="flex items-start gap-3">
              <AppWindow
                className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <div>
                <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
                  {dict.settings.trayTitle}
                </div>
                <p className="mt-0.5 text-[12px] text-[var(--text-secondary)]">
                  {dict.settings.trayDesc}
                </p>
              </div>
            </div>
            <button
              type="button"
              onClick={(e) => {
                e.currentTarget.blur();
                window.setTimeout(() => void api.previewTrayMenu(), 40);
              }}
              className="ui-btn-secondary h-7.5 shrink-0 px-2.5 text-[12px]"
            >
              <span>{dict.settings.previewTrayMenu}</span>
            </button>
          </div>
          <div className="divide-y divide-[var(--border-hairline)]">
            <ToggleRow
              label={dict.settings.closeToTrayLabel}
              checked={Boolean(settings.closeToTray)}
              onChange={(v) => void updateSettings({ closeToTray: v })}
            />
          </div>
        </section>

        {/* Windows Explorer Integration */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="flex items-start gap-3">
            <MousePointerClick
              className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-tertiary)]"
              strokeWidth={1.5}
            />
            <div>
              <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
                {dict.settings.shellTitle}
              </div>
              <p className="mt-0.5 text-[12px] text-[var(--text-secondary)]">
                {dict.settings.shellDesc}
              </p>
            </div>
          </div>
          <div className="divide-y divide-[var(--border-hairline)]">
            <ToggleRow
              label={dict.settings.shellForceLabel}
              checked={shellStatus.forceRemoveEnabled}
              disabled={shellBusy}
              onChange={(v) => void handleToggleShell(v, shellStatus.trackInstallEnabled)}
            />
            <ToggleRow
              label={dict.settings.shellTrackLabel}
              checked={shellStatus.trackInstallEnabled}
              disabled={shellBusy}
              onChange={(v) => void handleToggleShell(shellStatus.forceRemoveEnabled, v)}
            />
          </div>
        </section>

        {/* Safety Defaults */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="flex items-center gap-2 text-[13.5px] font-medium text-[var(--text-primary)]">
            <Shield className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
            <span>{dict.settings.safetyTitle}</span>
          </div>
          <div className="divide-y divide-[var(--border-hairline)]">
            <ToggleRow
              label={dict.wizard.useRecycleBin}
              checked={settings.useRecycleBin}
              onChange={(v) => void updateSettings({ useRecycleBin: v })}
            />
            <ToggleRow
              label={dict.wizard.backupRegistry}
              checked={settings.autoBackupRegistry}
              onChange={(v) => void updateSettings({ autoBackupRegistry: v })}
            />
            <ToggleRow
              label={dict.wizard.createRestorePoint}
              checked={settings.createRestorePoint}
              onChange={(v) => void updateSettings({ createRestorePoint: v })}
            />
          </div>
        </section>

        {/* Install Tracker Default Engine & Extra Folders */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
              {dict.settings.defaultTrackModeLabel}
            </div>
            <div className="inline-flex items-center gap-1 rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-1">
              {(
                [
                  { id: "etwRealtime" as TrackMode, label: dict.tracker.modeEtw },
                  { id: "hybridSnapshot" as TrackMode, label: dict.tracker.modeSnapshot },
                ] as const
              ).map((opt) => {
                const active = defaultTrackMode === opt.id;
                return (
                  <button
                    key={opt.id}
                    type="button"
                    onClick={() => void updateSettings({ defaultTrackMode: opt.id })}
                    className={`rounded-[7px] px-2.5 py-1 text-[12px] transition-colors duration-150 cursor-pointer ${
                      active
                        ? "bg-[var(--bg-surface)] font-medium text-[var(--text-primary)] shadow-[0_1px_2px_rgba(0,0,0,0.05)]"
                        : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                    }`}
                  >
                    {opt.label}
                  </button>
                );
              })}
            </div>
          </div>

          <div className="h-px bg-[var(--border-hairline)]" />

          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
                {dict.settings.trackRootsTitle}
              </div>
              <p className="mt-0.5 text-[12px] text-[var(--text-secondary)]">
                {dict.settings.trackRootsDesc}
              </p>
            </div>
            <button
              type="button"
              onClick={handleAddTrackRoot}
              className="ui-btn-secondary h-7.5 shrink-0 px-2.5 text-[12px]"
            >
              <FolderPlus className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
              <span>{dict.tracker.addExtraRoot}</span>
            </button>
          </div>
          {trackRoots.length === 0 ? (
            <div className="rounded-[10px] bg-[var(--bg-subtle)] px-3.5 py-2.5 text-[12px] text-[var(--text-tertiary)]">
              {dict.settings.trackRootsEmpty}
            </div>
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {trackRoots.map((root) => (
                <span
                  key={root}
                  className="inline-flex items-center gap-1.5 rounded-[8px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-2.5 py-1 font-mono text-[11.5px] text-[var(--text-primary)]"
                >
                  <span>{root}</span>
                  <button
                    type="button"
                    onClick={() => void handleRemoveTrackRoot(root)}
                    className="text-[var(--text-tertiary)] hover:text-[var(--text-primary)] cursor-pointer"
                  >
                    <X className="h-3 w-3" strokeWidth={1.5} />
                  </button>
                </span>
              ))}
            </div>
          )}
        </section>

        {/* Portable Software Library Folders */}
        <section className="ui-card space-y-3 p-5">
          <div className="flex items-start justify-between gap-3">
            <div>
              <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
                {dict.settings.portableRootsTitle}
              </div>
              <p className="mt-0.5 text-[12px] text-[var(--text-secondary)]">
                {dict.settings.portableRootsDesc}
              </p>
            </div>
            <button
              type="button"
              onClick={handleAddPortableRoot}
              className="ui-btn-secondary h-7.5 shrink-0 px-2.5 text-[12px]"
            >
              <FolderPlus className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
              <span>{dict.portable.addRoot}</span>
            </button>
          </div>
          {portableRoots.length === 0 ? (
            <div className="rounded-[10px] bg-[var(--bg-subtle)] px-3.5 py-2.5 text-[12px] text-[var(--text-tertiary)]">
              {dict.settings.portableRootsEmpty}
            </div>
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {portableRoots.map((root) => (
                <span
                  key={root}
                  className="inline-flex items-center gap-1.5 rounded-[8px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-2.5 py-1 font-mono text-[11.5px] text-[var(--text-primary)]"
                >
                  <span>{root}</span>
                  <button
                    type="button"
                    onClick={() => void handleRemovePortableRoot(root)}
                    className="text-[var(--text-tertiary)] hover:text-[var(--text-primary)] cursor-pointer"
                  >
                    <X className="h-3 w-3" strokeWidth={1.5} />
                  </button>
                </span>
              ))}
            </div>
          )}
        </section>

        {/* Display & Filtering */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
            {dict.settings.displayTitle}
          </div>
          <div className="divide-y divide-[var(--border-hairline)]">
            <ToggleRow
              label={dict.settings.showSystem}
              checked={settings.showSystemComponents}
              onChange={(v) => void updateSettings({ showSystemComponents: v })}
            />
            <ToggleRow
              label={dict.settings.showUpdates}
              checked={settings.showWindowsUpdates}
              onChange={(v) => void updateSettings({ showWindowsUpdates: v })}
            />
          </div>
        </section>

        {/* Appearance & Language */}
        <section className="ui-card space-y-4 p-5">
          <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
            {dict.settings.appearanceTitle}
          </div>
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div>
              <div className="mb-2 text-[12px] text-[var(--text-secondary)]">
                {dict.settings.themeLabel}
              </div>
              <div className="grid grid-cols-3 gap-1 rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-1">
                {themeOptions.map((opt) => {
                  const active = settings.theme === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      onClick={() => void updateSettings({ theme: opt.value })}
                      className={`rounded-[7px] px-2 py-1.5 text-[12px] transition-colors duration-150 cursor-pointer ${
                        active
                          ? "bg-[var(--bg-surface)] font-medium text-[var(--text-primary)] shadow-[0_1px_2px_rgba(0,0,0,0.05)]"
                          : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                      }`}
                    >
                      {opt.label}
                    </button>
                  );
                })}
              </div>
            </div>

            <div>
              <div className="mb-2 text-[12px] text-[var(--text-secondary)]">
                {dict.settings.langLabel}
              </div>
              <div className="grid grid-cols-3 gap-1 rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-1">
                {langOptions.map((opt) => {
                  const active = settings.language === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      onClick={() => void updateSettings({ language: opt.value })}
                      className={`rounded-[7px] px-2 py-1.5 text-[12px] transition-colors duration-150 cursor-pointer ${
                        active
                          ? "bg-[var(--bg-surface)] font-medium text-[var(--text-primary)] shadow-[0_1px_2px_rgba(0,0,0,0.05)]"
                          : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                      }`}
                    >
                      {opt.label}
                    </button>
                  );
                })}
              </div>
            </div>
          </div>
        </section>

        {/* About & Native WinHTTP Release Update Check */}
        <section className="ui-card space-y-2.5 p-5 text-[12px] text-[var(--text-secondary)]">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2 text-[13.5px] font-medium text-[var(--text-primary)]">
              <BrandLogo className="h-4 w-4 text-[var(--text-primary)]" />
              <span>{dict.settings.aboutTitle}</span>
            </div>
            <button
              type="button"
              disabled={checkingUpdate}
              onClick={handleCheckUpdates}
              className="ui-btn-secondary h-7.5 px-2.5 text-[12px]"
            >
              <RefreshCw
                className={`h-3.5 w-3.5 text-[var(--text-tertiary)] ${checkingUpdate ? "animate-spin" : ""}`}
                strokeWidth={1.5}
              />
              <span>
                {checkingUpdate ? dict.settings.checkingUpdates : dict.settings.checkUpdates}
              </span>
            </button>
          </div>

          <div className="pt-0.5">
            {dict.settings.versionLabel}{" "}
            <span className="num-tabular font-mono text-[var(--text-primary)]">
              v{status?.appVersion ?? "0.5.0"}
            </span>{" "}
            | Tauri v2 + Rust (windows-rs) + React / TypeScript
          </div>
          <div className="num-tabular text-[var(--text-tertiary)]">
            {status?.windowsVersion ?? "Windows"}
          </div>

          {updateResult && (
            <div className="mt-2 flex flex-wrap items-center justify-between gap-2 rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-3.5 py-2.5">
              <div className="num-tabular text-[12px] text-[var(--text-primary)]">
                {updateResult.hasUpdate ? (
                  <>
                    <span className="font-medium">{dict.settings.updateAvailable}:</span>{" "}
                    <span className="font-mono">v{updateResult.latestVersion}</span>
                  </>
                ) : (
                  <span className="text-[var(--success-text)]">
                    {dict.settings.upToDate} (v{updateResult.latestVersion})
                  </span>
                )}
              </div>
              {updateResult.hasUpdate && (
                <button
                  type="button"
                  onClick={() => api.openExternalUrl(updateResult.releaseUrl)}
                  className="ui-btn-primary h-7 px-2.5 text-[11.5px]"
                >
                  <ExternalLink className="h-3 w-3" strokeWidth={1.5} />
                  <span>{dict.settings.openRelease}</span>
                </button>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

function SwitchControl({
  checked,
  disabled,
  onChange,
  ariaLabel,
}: {
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
  ariaLabel?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
      className={`relative inline-flex h-[18px] w-[32px] shrink-0 items-center rounded-full transition-colors duration-150 cursor-pointer disabled:opacity-45 ${
        checked ? "bg-[var(--accent-primary)]" : "bg-[var(--bg-track)]"
      }`}
    >
      <span
        className={`inline-block h-[14px] w-[14px] transform rounded-full transition-transform duration-150 ${
          checked
            ? "translate-x-[16px] bg-[var(--accent-primary-fg)]"
            : "translate-x-[2px] bg-[var(--bg-surface)] shadow-xs"
        }`}
      />
    </button>
  );
}

function ToggleRow({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div
      onClick={(e) => {
        if (disabled) return;
        e.stopPropagation();
        onChange(!checked);
      }}
      className={`flex items-center justify-between gap-4 py-2.5 first:pt-0 last:pb-0 ${
        disabled ? "opacity-60" : "cursor-pointer"
      }`}
    >
      <span className="text-[12.5px] text-[var(--text-primary)]">{label}</span>
      <SwitchControl checked={checked} disabled={disabled} onChange={onChange} ariaLabel={label} />
    </div>
  );
}
