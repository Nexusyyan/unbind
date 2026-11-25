import { useEffect } from "react";
import { Activity, Hammer, X } from "lucide-react";
import type { Dict } from "../i18n";

interface DropActionModalProps {
  dict: Dict;
  path: string;
  onForceRemove: () => void;
  onTrackInstall: () => void;
  onClose: () => void;
}

export function DropActionModal({
  dict,
  path,
  onForceRemove,
  onTrackInstall,
  onClose,
}: DropActionModalProps) {
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
        className="ui-modal w-full max-w-[480px] overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-[var(--border-hairline)] px-5 py-4">
          <h2 className="text-[14.5px] font-medium text-[var(--text-primary)]">
            {dict.drop.title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={dict.common.close}
            className="ui-btn-ghost h-7 w-7 rounded-full p-0"
          >
            <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          </button>
        </div>

        <div className="space-y-4 px-5 py-4">
          <p className="text-[12.5px] text-[var(--text-secondary)]">{dict.drop.desc}</p>
          <div className="rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-3 py-2 font-mono text-[11.5px] break-all text-[var(--text-primary)]">
            {path}
          </div>

          <div className="grid grid-cols-1 gap-2.5">
            <button
              type="button"
              onClick={onTrackInstall}
              className="group flex items-start gap-3 rounded-[12px] border border-[var(--border-hairline)] bg-[var(--bg-surface)] p-3.5 text-left transition-colors hover:border-[var(--border-strong)] hover:bg-[var(--bg-hover)] cursor-pointer"
            >
              <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-[9px] bg-[var(--bg-subtle)] text-[var(--text-primary)]">
                <Activity className="h-4 w-4" strokeWidth={1.5} />
              </span>
              <div>
                <div className="text-[13px] font-medium text-[var(--text-primary)]">
                  {dict.drop.trackTitle}
                </div>
                <div className="mt-0.5 text-[12px] text-[var(--text-secondary)]">
                  {dict.drop.trackDesc}
                </div>
              </div>
            </button>

            <button
              type="button"
              onClick={onForceRemove}
              className="group flex items-start gap-3 rounded-[12px] border border-[var(--border-hairline)] bg-[var(--bg-surface)] p-3.5 text-left transition-colors hover:border-[var(--border-strong)] hover:bg-[var(--bg-hover)] cursor-pointer"
            >
              <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-[9px] bg-[var(--bg-subtle)] text-[var(--text-primary)]">
                <Hammer className="h-4 w-4" strokeWidth={1.5} />
              </span>
              <div>
                <div className="text-[13px] font-medium text-[var(--text-primary)]">
                  {dict.drop.forceTitle}
                </div>
                <div className="mt-0.5 text-[12px] text-[var(--text-secondary)]">
                  {dict.drop.forceDesc}
                </div>
              </div>
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
