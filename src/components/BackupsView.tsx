import { useMemo, useState } from "react";
import {
  ArchiveRestore,
  CheckCircle2,
  Eye,
  FolderOpen,
  History,
  Loader2,
  RefreshCw,
  Search,
  ShieldPlus,
  Trash2,
  X,
  XCircle,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { BackupEntry, OperationLogEntry } from "../types";
import { formatBytes, formatDateTime } from "../utils/format";
import { BackupPreviewModal } from "./BackupPreviewModal";
import { ConfirmDialog } from "./ConfirmDialog";

interface BackupsViewProps {
  dict: Dict;
  backups: BackupEntry[];
  logs: OperationLogEntry[];
  onRefresh: () => void;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

type LogStatusFilter = "all" | "success" | "failed";

export function BackupsView({
  dict,
  backups,
  logs,
  onRefresh,
  onNotify,
}: BackupsViewProps) {
  const [subTab, setSubTab] = useState<"backups" | "logs">("backups");
  const [creatingRp, setCreatingRp] = useState(false);

  const [previewState, setPreviewState] = useState<{
    backup: BackupEntry;
    content: string;
  } | null>(null);
  const [pendingRestore, setPendingRestore] = useState<BackupEntry | null>(null);
  const [pendingDeleteBackup, setPendingDeleteBackup] = useState<BackupEntry | null>(null);
  const [confirmClearLogs, setConfirmClearLogs] = useState(false);
  const [busyAction, setBusyAction] = useState(false);

  const [logQuery, setLogQuery] = useState("");
  const [logFilter, setLogFilter] = useState<LogStatusFilter>("all");

  const filteredLogs = useMemo(() => {
    const q = logQuery.trim().toLowerCase();
    return logs.filter((entry) => {
      if (logFilter === "success" && !entry.success) return false;
      if (logFilter === "failed" && entry.success) return false;
      if (q) {
        const hay = `${entry.action} ${entry.target} ${entry.summary}`.toLowerCase();
        return hay.includes(q);
      }
      return true;
    });
  }, [logs, logQuery, logFilter]);

  const handleCreateRestorePoint = async () => {
    setCreatingRp(true);
    try {
      const ok = await api.createRestorePoint("Unbind Manual Checkpoint");
      if (ok) {
        onNotify("success", dict.backups.createRestorePoint, dict.backups.restorePointCreated);
      } else {
        onNotify("warning", dict.backups.createRestorePoint, dict.backups.restorePointDisabled);
      }
      onRefresh();
    } catch (err) {
      onNotify("error", dict.backups.createRestorePoint, String(err));
    } finally {
      setCreatingRp(false);
    }
  };

  const handlePreview = async (backup: BackupEntry) => {
    try {
      const content = await api.previewRegistryBackup(backup.path);
      setPreviewState({ backup, content });
    } catch (err) {
      onNotify("error", dict.backups.previewTitle, String(err));
    }
  };

  const handleConfirmRestore = async () => {
    if (!pendingRestore) return;
    setBusyAction(true);
    try {
      await api.restoreRegistryBackup(pendingRestore.path);
      onNotify("success", dict.backups.restoredToast, pendingRestore.filename);
      setPendingRestore(null);
      setPreviewState(null);
      onRefresh();
    } catch (err) {
      onNotify("error", dict.backups.restoreConfirmTitle, String(err));
    } finally {
      setBusyAction(false);
    }
  };

  const handleConfirmDeleteBackup = async () => {
    if (!pendingDeleteBackup) return;
    setBusyAction(true);
    try {
      await api.deleteRegistryBackup(pendingDeleteBackup.path);
      onNotify("success", dict.backups.deletedBackupToast, pendingDeleteBackup.filename);
      setPendingDeleteBackup(null);
      onRefresh();
    } catch (err) {
      onNotify("error", dict.backups.deleteBackupTitle, String(err));
    } finally {
      setBusyAction(false);
    }
  };

  const handleConfirmClearLogs = async () => {
    setBusyAction(true);
    try {
      await api.clearOperationLogs();
      onNotify("success", dict.backups.logsClearedToast);
      setConfirmClearLogs(false);
      onRefresh();
    } catch (err) {
      onNotify("error", dict.backups.clearLogsTitle, String(err));
    } finally {
      setBusyAction(false);
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
        <div className="flex items-center justify-between gap-2 border-b border-[var(--border-hairline)] pb-3">
          <div className="flex items-center gap-1">
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

          {subTab === "logs" && logs.length > 0 && (
            <button
              type="button"
              onClick={() => setConfirmClearLogs(true)}
              className="ui-btn-danger-ghost h-7.5 px-2.5 text-[12px]"
            >
              <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
              <span>{dict.backups.clearLogs}</span>
            </button>
          )}
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
                  <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100">
                    <button
                      type="button"
                      onClick={() => handlePreview(b)}
                      className="ui-btn-ghost h-7.5 px-2 text-[12px]"
                    >
                      <Eye className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
                      <span>{dict.common.preview}</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => setPendingRestore(b)}
                      className="ui-btn-ghost h-7.5 px-2 text-[12px]"
                    >
                      <ArchiveRestore
                        className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                        strokeWidth={1.5}
                      />
                      <span>{dict.common.restore}</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => api.openPathInExplorer(b.path)}
                      title={dict.common.openDir}
                      aria-label={dict.common.openDir}
                      className="ui-btn-ghost h-7.5 w-7.5 p-0"
                    >
                      <FolderOpen
                        className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                        strokeWidth={1.5}
                      />
                    </button>
                    <button
                      type="button"
                      onClick={() => setPendingDeleteBackup(b)}
                      title={dict.common.delete}
                      aria-label={dict.common.delete}
                      className="ui-btn-danger-ghost h-7.5 w-7.5 p-0"
                    >
                      <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )
        ) : (
          <div className="space-y-3">
            {logs.length > 0 && (
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="ui-search-float relative flex h-8.5 min-w-[220px] max-w-md flex-1 items-center">
                  <Search
                    className="pointer-events-none absolute left-3 h-3.5 w-3.5 text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                  <input
                    type="text"
                    value={logQuery}
                    onChange={(e) => setLogQuery(e.target.value)}
                    placeholder={dict.backups.searchLogsPlaceholder}
                    className="h-full w-full bg-transparent pr-7 pl-8.5 text-[12.5px] text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] outline-none"
                  />
                  {logQuery && (
                    <button
                      type="button"
                      onClick={() => setLogQuery("")}
                      className="absolute right-2 flex h-4.5 w-4.5 items-center justify-center rounded-full text-[var(--text-tertiary)] hover:bg-[var(--bg-hover)] cursor-pointer"
                    >
                      <X className="h-3 w-3" strokeWidth={1.5} />
                    </button>
                  )}
                </div>

                <div className="flex items-center gap-1">
                  {(
                    [
                      ["all", dict.backups.logFilterAll],
                      ["success", dict.backups.logFilterSuccess],
                      ["failed", dict.backups.logFilterFailed],
                    ] as const
                  ).map(([id, label]) => (
                    <button
                      key={id}
                      type="button"
                      onClick={() => setLogFilter(id)}
                      className={`rounded-[8px] px-2.5 py-1 text-[12px] transition-colors cursor-pointer ${
                        logFilter === id
                          ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                          : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)]"
                      }`}
                    >
                      {label}
                    </button>
                  ))}
                </div>
              </div>
            )}

            {filteredLogs.length === 0 ? (
              <div className="rounded-[14px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] py-14 text-center text-[12.5px] text-[var(--text-secondary)]">
                {dict.backups.emptyLogs}
              </div>
            ) : (
              <div className="divide-y divide-[var(--border-hairline)]">
                {filteredLogs.map((entry, idx) => (
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
        )}
      </div>

      {previewState && (
        <BackupPreviewModal
          dict={dict}
          backup={previewState.backup}
          content={previewState.content}
          onClose={() => setPreviewState(null)}
          onCopy={async () => {
            try {
              await navigator.clipboard.writeText(previewState.content);
              onNotify("success", dict.common.copied);
            } catch {
              // Ignore
            }
          }}
          onOpenFolder={() => api.openPathInExplorer(previewState.backup.path)}
          onRestore={() => setPendingRestore(previewState.backup)}
        />
      )}

      {pendingRestore && (
        <ConfirmDialog
          title={dict.backups.restoreConfirmTitle}
          description={dict.backups.restoreConfirmDesc}
          detail={pendingRestore.filename}
          confirmLabel={dict.common.restore}
          cancelLabel={dict.common.cancel}
          busy={busyAction}
          onCancel={() => setPendingRestore(null)}
          onConfirm={() => void handleConfirmRestore()}
        />
      )}

      {pendingDeleteBackup && (
        <ConfirmDialog
          title={dict.backups.deleteBackupTitle}
          description={dict.backups.deleteBackupDesc}
          detail={pendingDeleteBackup.filename}
          confirmLabel={dict.common.delete}
          cancelLabel={dict.common.cancel}
          danger
          busy={busyAction}
          onCancel={() => setPendingDeleteBackup(null)}
          onConfirm={() => void handleConfirmDeleteBackup()}
        />
      )}

      {confirmClearLogs && (
        <ConfirmDialog
          title={dict.backups.clearLogsTitle}
          description={dict.backups.clearLogsDesc}
          confirmLabel={dict.backups.clearLogs}
          cancelLabel={dict.common.cancel}
          danger
          busy={busyAction}
          onCancel={() => setConfirmClearLogs(false)}
          onConfirm={() => void handleConfirmClearLogs()}
        />
      )}
    </div>
  );
}
