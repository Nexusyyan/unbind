import { AlertCircle, AlertTriangle, CheckCircle2, Info, X } from "lucide-react";
import type { ToastMessage } from "../types";

interface ToastContainerProps {
  toasts: ToastMessage[];
  onDismiss: (id: string) => void;
}

export function ToastContainer({ toasts, onDismiss }: ToastContainerProps) {
  if (toasts.length === 0) return null;
  return (
    <div className="fixed right-4 bottom-4 z-50 flex w-80 flex-col gap-2">
      {toasts.map((t) => {
        const Icon =
          t.kind === "success"
            ? CheckCircle2
            : t.kind === "error"
              ? AlertCircle
              : t.kind === "warning"
                ? AlertTriangle
                : Info;
        const color =
          t.kind === "success"
            ? "text-[var(--success-text)]"
            : t.kind === "error"
              ? "text-[var(--danger-text)]"
              : t.kind === "warning"
                ? "text-[var(--warning-text)]"
                : "text-[var(--text-primary)]";
        return (
          <div key={t.id} className="ui-popover flex items-start gap-2.5 p-3.5">
            <Icon className={`mt-0.5 h-4 w-4 shrink-0 ${color}`} strokeWidth={1.5} />
            <div className="min-w-0 flex-1">
              <div className="text-[12.5px] font-medium text-[var(--text-primary)]">{t.title}</div>
              {t.detail && (
                <div className="mt-0.5 break-words text-[11.5px] text-[var(--text-secondary)]">
                  {t.detail}
                </div>
              )}
            </div>
            <button
              type="button"
              onClick={() => onDismiss(t.id)}
              aria-label="Dismiss"
              className="ui-btn-ghost h-6 w-6 rounded-full p-0"
            >
              <X className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
