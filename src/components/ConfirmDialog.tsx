import { useEffect } from "react";
import { AlertTriangle, X } from "lucide-react";

interface ConfirmDialogProps {
  title: string;
  description: string;
  detail?: string;
  confirmLabel: string;
  cancelLabel: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

export function ConfirmDialog({
  title,
  description,
  detail,
  confirmLabel,
  cancelLabel,
  danger = false,
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (busy) return;
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
      } else if (e.key === "Enter") {
        e.preventDefault();
        onConfirm();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel, onConfirm]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--backdrop-scrim)] p-4 backdrop-blur-[2px]"
      onClick={() => {
        if (!busy) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        className="ui-modal w-full max-w-[440px] overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-start justify-between gap-3 border-b border-[var(--border-hairline)] px-5 py-4">
          <div className="flex items-center gap-2.5">
            {danger && (
              <AlertTriangle
                className="h-4 w-4 shrink-0 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
            )}
            <h2 className="text-[14.5px] font-medium text-[var(--text-primary)]">{title}</h2>
          </div>
          <button
            type="button"
            disabled={busy}
            onClick={onCancel}
            aria-label={cancelLabel}
            className="ui-btn-ghost h-7 w-7 rounded-full p-0"
          >
            <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          </button>
        </div>

        <div className="space-y-3 px-5 py-4">
          <p className="text-[12.5px] leading-relaxed text-[var(--text-secondary)]">
            {description}
          </p>
          {detail && (
            <div className="rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-3 py-2 font-mono text-[11.5px] break-all text-[var(--text-primary)]">
              {detail}
            </div>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-5 py-3">
          <button
            type="button"
            disabled={busy}
            onClick={onCancel}
            className="ui-btn-ghost h-8"
          >
            {cancelLabel}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={onConfirm}
            className={danger ? "ui-btn-danger-subtle h-8" : "ui-btn-primary h-8"}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
