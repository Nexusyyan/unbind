import { useEffect, useState } from "react";
import { FolderOpen, HardDrive, Save, Shield } from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { AppSettings, SystemStatus } from "../types";
import { BrandLogo } from "./BrandLogo";

interface SettingsViewProps {
  dict: Dict;
  settings: AppSettings;
  status: SystemStatus | null;
  onSave: (next: AppSettings) => Promise<void>;
}

export function SettingsView({ dict, settings, status, onSave }: SettingsViewProps) {
  const [draft, setDraft] = useState<AppSettings>(settings);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await onSave(draft);
    } finally {
      setSaving(false);
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
          <button
            type="button"
            disabled={saving}
            onClick={handleSave}
            className="ui-btn-primary h-8.5"
          >
            <Save className="h-3.5 w-3.5" strokeWidth={1.5} />
            <span>{dict.settings.save}</span>
          </button>
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
              checked={draft.portableMode}
              onChange={(v) => setDraft((d) => ({ ...d, portableMode: v }))}
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
              {dict.appList.openFolder}
            </button>
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
              checked={draft.useRecycleBin}
              onChange={(v) => setDraft((d) => ({ ...d, useRecycleBin: v }))}
            />
            <ToggleRow
              label={dict.wizard.backupRegistry}
              checked={draft.autoBackupRegistry}
              onChange={(v) => setDraft((d) => ({ ...d, autoBackupRegistry: v }))}
            />
            <ToggleRow
              label={dict.wizard.createRestorePoint}
              checked={draft.createRestorePoint}
              onChange={(v) => setDraft((d) => ({ ...d, createRestorePoint: v }))}
            />
          </div>
        </section>

        {/* Display & Filtering */}
        <section className="ui-card space-y-3.5 p-5">
          <div className="text-[13.5px] font-medium text-[var(--text-primary)]">
            {dict.settings.displayTitle}
          </div>
          <div className="divide-y divide-[var(--border-hairline)]">
            <ToggleRow
              label={dict.settings.showSystem}
              checked={draft.showSystemComponents}
              onChange={(v) => setDraft((d) => ({ ...d, showSystemComponents: v }))}
            />
            <ToggleRow
              label={dict.settings.showUpdates}
              checked={draft.showWindowsUpdates}
              onChange={(v) => setDraft((d) => ({ ...d, showWindowsUpdates: v }))}
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
                  const active = draft.theme === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      onClick={() => setDraft((d) => ({ ...d, theme: opt.value }))}
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
                  const active = draft.language === opt.value;
                  return (
                    <button
                      key={opt.value}
                      type="button"
                      onClick={() => setDraft((d) => ({ ...d, language: opt.value }))}
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

        {/* About */}
        <section className="ui-card space-y-1.5 p-5 text-[12px] text-[var(--text-secondary)]">
          <div className="flex items-center gap-2 text-[13.5px] font-medium text-[var(--text-primary)]">
            <BrandLogo className="h-4 w-4 text-[var(--text-primary)]" />
            <span>{dict.settings.aboutTitle}</span>
          </div>
          <div className="pt-0.5">
            Version{" "}
            <span className="num-tabular font-mono text-[var(--text-primary)]">
              {status?.appVersion ?? "0.3.0"}
            </span>{" "}
            | Tauri v2 + Rust (windows-rs) + React / TypeScript
          </div>
          <div className="num-tabular text-[var(--text-tertiary)]">
            {status?.windowsVersion ?? "Windows"}
          </div>
        </section>
      </div>
    </div>
  );
}

function SwitchControl({
  checked,
  onChange,
  ariaLabel,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  ariaLabel?: string;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
      className={`relative inline-flex h-[18px] w-[32px] shrink-0 items-center rounded-full transition-colors duration-150 cursor-pointer ${
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
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div
      onClick={(e) => {
        e.stopPropagation();
        onChange(!checked);
      }}
      className="flex items-center justify-between gap-4 py-2.5 first:pt-0 last:pb-0 cursor-pointer"
    >
      <span className="text-[12.5px] text-[var(--text-primary)]">{label}</span>
      <SwitchControl checked={checked} onChange={onChange} ariaLabel={label} />
    </div>
  );
}
