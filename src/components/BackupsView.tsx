import { useState } from "react";
import {
  ArchiveRestore,
  CheckCircle2,
  FolderOpen,
  History,
  Loader2,
  RefreshCw,
  ShieldPlus,
  XCircle,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { BackupEntry, OperationLogEntry } from "../types";
import { formatBytes, formatDateTime } from "../utils/format";

interface BackupsViewProps {
  dict: Dict;
  backups: BackupEntry[];
  logs: OperationLogEntry[];
  onRefresh: () => void;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function BackupsView({
  dict,
  backups,
  logs,
  onRefresh,
  onNotify,
}: BackupsViewProps) {
  const [subTab, setSubTab] = useState<"backups" | "logs">("backups");
  const [creatingRp, setCreatingRp] = useState(false);

  const handleCreateRestorePoint = async () => {
    setCreatingRp(true);
    try {
      const ok = await api.createRestorePoint("Unbind Manual Checkpoint");
      if (ok) {
        onNotify("success", dict.backups.createRestorePoint, "Restore point created");
      } else {
        onNotify(
          "warning",
          dict.backups.createRestorePoint,
          "System Protection may be disabled on the OS drive.",
        );
      }
      onRefresh();
    } catch (err) {
      onNotify("error", dict.backups.createRestorePoint, String(err));
    } finally {
      setCreatingRp(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-y-auto bg-[var(--bg-canvas)] px-6 pt-5 pb-10">
      <div className="w-full max-w-[960px] space-y-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h1 className="text-[17px] font-medium tracking-[-0.015em] text-[var(--text-primary)]">
              {dict.backups.title}
            </h1>
            <p className="mt-0.5 text-[12.5px] text-[var(--text-secondary)]">
              {dict.backups.subtitle}
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-0.5">
            <button
              type="button"
              disabled={creatingRp}
              onClick={handleCreateRestorePoint}
              className="ui-btn-ghost h-8 px-2.5"
            >
              {creatingRp ? (
                <Loader2
                  className="h-3.5 w-3.5 animate-spin text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
              ) : (
                <ShieldPlus
                  className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
              )}
              <span>{dict.backups.createRestorePoint}</span>
            </button>
            <button
              type="button"
              onClick={() => api.openBackupsFolder()}
              className="ui-btn-ghost h-8 px-2.5"
            >
              <FolderOpen
                className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <span>{dict.backups.openFolder}</span>
            </button>
            <button
              type="button"
              onClick={onRefresh}
              aria-label={dict.toolbar.refresh}
              className="ui-btn-ghost h-8 w-8 p-0"
            >
              <RefreshCw
                className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
            </button>
          </div>
        </div>

        {/* Minimal Flat Sub-Tabs */}
        <div className="flex items-center gap-1 border-b border-[var(--border-hairline)] pb-3">
          <button
            type="button"
            onClick={() => setSubTab("backups")}
            className={`inline-flex items-center gap-2 rounded-[8px] px-3 py-1.5 text-[12.5px] transition-colors duration-150 cursor-pointer ${
              subTab === "backups"
                ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            }`}
          >
            <ArchiveRestore
              className={`h-3.5 w-3.5 ${
                subTab === "backups"
                  ? "text-[var(--text-primary)]"
                  : "text-[var(--text-tertiary)]"
              }`}
              strokeWidth={1.5}
            />
            <span>{dict.backups.tabBackups}</span>
            <span className="num-tabular text-[11.5px] text-[var(--text-tertiary)]">
              {backups.length}
            </span>
          </button>

          <button
            type="button"
            onClick={() => setSubTab("logs")}
            className={`inline-flex items-center gap-2 rounded-[8px] px-3 py-1.5 text-[12.5px] transition-colors duration-150 cursor-pointer ${
              subTab === "logs"
                ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
            }`}
          >
            <History
              className={`h-3.5 w-3.5 ${
                subTab === "logs"
                  ? "text-[var(--text-primary)]"
                  : "text-[var(--text-tertiary)]"
              }`}
              strokeWidth={1.5}
            />
            <span>{dict.backups.tabLogs}</span>
            <span className="num-tabular text-[11.5px] text-[var(--text-tertiary)]">
              {logs.length}
            </span>
          </button>
        </div>

        {subTab === "backups" ? (
          backups.length === 0 ? (
            <div className="rounded-[14px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] py-14 text-center text-[12.5px] text-[var(--text-secondary)]">
              {dict.backups.emptyBackups}
            </div>
          ) : (
            <div className="divide-y divide-[var(--border-hairline)]">
              {backups.map((b) => (
                <div
                  key={b.id}
                  className="group flex items-center justify-between gap-3 rounded-[10px] px-3 py-3 transition-colors duration-150 hover:bg-[var(--bg-hover)]"
                >
                  <div className="min-w-0">
                    <div className="text-[13px] font-medium text-[var(--text-primary)]">
                      {b.appName}
                    </div>
                    <div className="num-tabular mt-0.5 truncate font-mono text-[11.5px] text-[var(--text-secondary)]">
                      {b.filename} | {formatBytes(b.sizeBytes)} | {formatDateTime(b.createdAt)}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => api.openPathInExplorer(b.path)}
                    className="ui-btn-ghost h-7.5 shrink-0 px-2.5 text-[12px] opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100"
                  >
                    <FolderOpen
                      className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                      strokeWidth={1.5}
                    />
                    {dict.appList.openFolder}
                  </button>
                </div>
              ))}
            </div>
          )
        ) : logs.length === 0 ? (
          <div className="rounded-[14px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] py-14 text-center text-[12.5px] text-[var(--text-secondary)]">
            {dict.backups.emptyLogs}
          </div>
        ) : (
          <div className="divide-y divide-[var(--border-hairline)]">
            {logs.map((entry, idx) => (
              <div
                key={`${entry.timestamp}-${idx}`}
                className="flex items-start justify-between gap-4 px-2 py-3 text-[12px]"
              >
                <div className="flex min-w-0 items-start gap-2.5">
                  {entry.success ? (
                    <CheckCircle2
                      className="mt-0.5 h-4 w-4 shrink-0 text-[var(--text-tertiary)]"
                      strokeWidth={1.5}
                    />
                  ) : (
                    <XCircle
                      className="mt-0.5 h-4 w-4 shrink-0 text-[var(--danger-text)]"
                      strokeWidth={1.5}
                    />
                  )}
                  <div className="min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="rounded-[6px] bg-[var(--bg-subtle)] px-1.5 py-0.5 font-mono text-[10.5px] uppercase text-[var(--text-secondary)]">
                        {entry.action}
                      </span>
                      <span className="truncate font-medium text-[var(--text-primary)]">
                        {entry.target}
                      </span>
                    </div>
                    <div className="mt-1 break-words text-[12px] text-[var(--text-secondary)]">
                      {entry.summary}
                    </div>
                  </div>
                </div>
                <span className="num-tabular shrink-0 text-[11.5px] text-[var(--text-tertiary)]">
                  {formatDateTime(entry.timestamp)}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
