import { useEffect } from "react";
import { ArchiveRestore, Copy, FolderOpen, X } from "lucide-react";
import type { Dict } from "../i18n";
import type { BackupEntry } from "../types";

interface BackupPreviewModalProps {
  dict: Dict;
  backup: BackupEntry;
  content: string;
  onCopy: () => void;
  onOpenFolder: () => void;
  onRestore: () => void;
  onClose: () => void;
}

export function BackupPreviewModal({
  dict,
  backup,
  content,
  onCopy,
  onOpenFolder,
  onRestore,
  onClose,
}: BackupPreviewModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--backdrop-scrim)] p-4 backdrop-blur-[2px]"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        className="ui-modal flex max-h-[82vh] w-full max-w-[740px] flex-col overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3 border-b border-[var(--border-hairline)] px-6 py-4">
          <div className="min-w-0">
            <h2 className="text-[15px] font-medium text-[var(--text-primary)]">
              {dict.backups.previewTitle} - {backup.appName}
            </h2>
            <div className="mt-0.5 truncate font-mono text-[11.5px] text-[var(--text-secondary)]">
              {backup.filename}
            </div>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label={dict.common.close}
            className="ui-btn-ghost h-7 w-7 rounded-full p-0"
          >
            <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          </button>
        </div>

        <div className="min-h-[240px] flex-1 overflow-auto bg-[var(--bg-subtle)] p-4">
          <pre className="m-0 font-mono text-[11.5px] leading-relaxed whitespace-pre-wrap break-all text-[var(--text-primary)] select-text">
            {content}
          </pre>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-[var(--border-hairline)] bg-[var(--bg-surface)] px-6 py-3.5">
          <div className="flex items-center gap-1">
            <button type="button" onClick={onCopy} className="ui-btn-ghost h-8 px-2.5 text-[12px]">
              <Copy className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
              <span>{dict.common.copy}</span>
            </button>
            <button
              type="button"
              onClick={onOpenFolder}
              className="ui-btn-ghost h-8 px-2.5 text-[12px]"
            >
              <FolderOpen className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
              <span>{dict.common.openDir}</span>
            </button>
          </div>
          <div className="flex items-center gap-2">
            <button type="button" onClick={onClose} className="ui-btn-ghost h-8">
              {dict.common.close}
            </button>
            <button type="button" onClick={onRestore} className="ui-btn-primary h-8">
              <ArchiveRestore className="h-3.5 w-3.5" strokeWidth={1.5} />
              <span>{dict.common.restore}</span>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
