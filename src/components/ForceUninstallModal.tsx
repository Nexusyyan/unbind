import { useState } from "react";
import { FileSearch, FolderOpen, Hammer, Loader2, Lock, Skull, X } from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { InstalledApp, LeftoverItem, LockingProcessInfo } from "../types";

interface ForceUninstallModalProps {
  dict: Dict;
  initialPath?: string;
  onClose: () => void;
  onProceedToReview: (targetApp: InstalledApp, leftovers: LeftoverItem[]) => void;
}

export function ForceUninstallModal({
  dict,
  initialPath = "",
  onClose,
  onProceedToReview,
}: ForceUninstallModalProps) {
  const [targetPath, setTargetPath] = useState(initialPath);
  const [scanning, setScanning] = useState(false);
  const [checkingLocks, setCheckingLocks] = useState(false);
  const [locks, setLocks] = useState<LockingProcessInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handlePickFile = async () => {
    const picked = await api.pickFile([
      { name: "Executable or Shortcut", extensions: ["exe", "lnk", "msi"] },
    ]);
    if (picked) {
      setTargetPath(picked);
      setLocks(null);
    }
  };

  const handlePickFolder = async () => {
    const picked = await api.pickFolder();
    if (picked) {
      setTargetPath(picked);
      setLocks(null);
    }
  };

  const handleCheckLocks = async () => {
    if (!targetPath.trim()) return;
    setCheckingLocks(true);
    setError(null);
    try {
      const list = await api.checkFileLocks([targetPath.trim()]);
      setLocks(list);
    } catch (err) {
      setError(String(err));
    } finally {
      setCheckingLocks(false);
    }
  };

  const handleKillLocks = async () => {
    if (!locks || locks.length === 0) return;
    try {
      await api.terminateLockingProcesses(locks.map((l) => l.pid));
      setLocks([]);
    } catch (err) {
      setError(String(err));
    }
  };

  const handleScan = async () => {
    if (!targetPath.trim()) return;
    setScanning(true);
    setError(null);
    try {
      const [syntheticApp, leftovers] = await api.scanForceTarget(targetPath.trim());
      onProceedToReview(syntheticApp, leftovers);
    } catch (err) {
      setError(String(err));
      setScanning(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--backdrop-scrim)] p-4 backdrop-blur-[2px]">
      <div className="ui-modal w-full max-w-[560px] overflow-hidden">
        <div className="flex items-center justify-between border-b border-[var(--border-hairline)] px-6 py-4">
          <div className="flex items-center gap-2">
            <Hammer className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
            <h2 className="text-[15px] font-medium text-[var(--text-primary)]">
              {dict.force.title}
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="ui-btn-ghost h-7 w-7 rounded-full p-0"
          >
            <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          </button>
        </div>

        <div className="space-y-4 px-6 py-5">
          <p className="text-[12.5px] leading-relaxed text-[var(--text-secondary)]">
            {dict.force.desc}
          </p>

          <div className="space-y-2">
            <label className="text-[11.5px] font-medium text-[var(--text-tertiary)]">
              {dict.force.targetLabel}
            </label>
            <input
              type="text"
              value={targetPath}
              onChange={(e) => setTargetPath(e.target.value)}
              placeholder="C:\Program Files\StubbornApp\app.exe"
              className="ui-input h-9 w-full px-3 font-mono text-[12px]"
            />
            <div className="flex flex-wrap gap-1.5 pt-0.5">
              <button
                type="button"
                onClick={handlePickFile}
                className="ui-btn-secondary h-8 text-[12px]"
              >
                <FileSearch
                  className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
                {dict.force.pickFile}
              </button>
              <button
                type="button"
                onClick={handlePickFolder}
                className="ui-btn-secondary h-8 text-[12px]"
              >
                <FolderOpen
                  className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
                {dict.force.pickFolder}
              </button>
              <button
                type="button"
                disabled={!targetPath.trim() || checkingLocks}
                onClick={handleCheckLocks}
                className="ui-btn-secondary h-8 text-[12px]"
              >
                {checkingLocks ? (
                  <Loader2
                    className="h-3.5 w-3.5 animate-spin text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                ) : (
                  <Lock className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
                )}
                {dict.wizard.checkLocks}
              </button>
            </div>
          </div>

          {error && (
            <div className="rounded-[12px] border border-[var(--danger-border)] bg-[var(--danger-bg-hover)] p-3 text-[12px] text-[var(--danger-text)]">
              {error}
            </div>
          )}

          {locks !== null && (
            <div className="rounded-[12px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-3.5 text-[12px]">
              {locks.length === 0 ? (
                <div className="text-[var(--success-text)]">{dict.force.noLocks}</div>
              ) : (
                <div className="space-y-2.5">
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-[var(--warning-text)]">
                      {dict.wizard.locksDetected}
                    </span>
                    <button
                      type="button"
                      onClick={handleKillLocks}
                      className="ui-btn-danger-subtle h-7 px-2.5 text-[11.5px]"
                    >
                      <Skull className="h-3 w-3" strokeWidth={1.5} />
                      {dict.wizard.killAndRetry}
                    </button>
                  </div>
                  <div className="flex flex-wrap gap-1.5">
                    {locks.map((l) => (
                      <span
                        key={l.pid}
                        className="num-tabular rounded-[6px] border border-[var(--border-hairline)] bg-[var(--bg-surface)] px-2 py-0.5 font-mono text-[11px] text-[var(--text-primary)]"
                      >
                        {l.name} (PID {l.pid})
                      </span>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-6 py-3.5">
          <button type="button" onClick={onClose} className="ui-btn-ghost h-8">
            {dict.wizard.cancel}
          </button>
          <button
            type="button"
            disabled={!targetPath.trim() || scanning}
            onClick={handleScan}
            className="ui-btn-primary h-8"
          >
            {scanning ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" strokeWidth={1.5} />
            ) : (
              <Hammer className="h-3.5 w-3.5" strokeWidth={1.5} />
            )}
            {dict.force.scanTarget}
          </button>
        </div>
      </div>
    </div>
  );
}
