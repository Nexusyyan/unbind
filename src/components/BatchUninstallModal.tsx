import { useState } from "react";
import { AlertCircle, CheckCircle2, Clock, Loader2, Play, Trash2, X, Zap } from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { InstalledApp, LeftoverItem } from "../types";

type QueueItemStatus = "pending" | "running" | "done" | "error";

interface QueueState {
  app: InstalledApp;
  status: QueueItemStatus;
  detail?: string;
}

interface BatchUninstallModalProps {
  dict: Dict;
  apps: InstalledApp[];
  initialQuiet: boolean;
  onClose: (didModify: boolean) => void;
  onReviewCombinedLeftovers: (label: string, leftovers: LeftoverItem[]) => void;
}

export function BatchUninstallModal({
  dict,
  apps,
  initialQuiet,
  onClose,
  onReviewCombinedLeftovers,
}: BatchUninstallModalProps) {
  const [quiet, setQuiet] = useState(initialQuiet);
  const [running, setRunning] = useState(false);
  const [finished, setFinished] = useState(false);
  const [queue, setQueue] = useState<QueueState[]>(() =>
    apps.map((app) => ({ app, status: "pending" })),
  );
  const [combinedLeftovers, setCombinedLeftovers] = useState<LeftoverItem[]>([]);

  const handleStart = async () => {
    setRunning(true);
    const collected: LeftoverItem[] = [];
    const seenPaths = new Set<string>();

    for (let i = 0; i < queue.length; i++) {
      const target = queue[i].app;
      setQueue((prev) =>
        prev.map((item, idx) => (idx === i ? { ...item, status: "running" } : item)),
      );

      try {
        const res = await api.runUninstall(target, quiet);
        const found = await api.scanAppLeftovers(target);
        for (const item of found) {
          const key = `${item.kind}|${item.path.toLowerCase()}|${item.valueName ?? ""}`;
          if (!seenPaths.has(key)) {
            seenPaths.add(key);
            collected.push(item);
          }
        }
        setQueue((prev) =>
          prev.map((item, idx) =>
            idx === i ? { ...item, status: "done", detail: res.message } : item,
          ),
        );
      } catch (err) {
        setQueue((prev) =>
          prev.map((item, idx) =>
            idx === i ? { ...item, status: "error", detail: String(err) } : item,
          ),
        );
      }
    }

    setCombinedLeftovers(collected);
    setRunning(false);
    setFinished(true);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--backdrop-scrim)] p-4 backdrop-blur-[2px]">
      <div className="ui-modal flex max-h-[84vh] w-full max-w-[620px] flex-col overflow-hidden">
        <div className="flex items-center justify-between border-b border-[var(--border-hairline)] px-6 py-4">
          <div className="flex items-center gap-2">
            <Trash2 className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
            <h2 className="num-tabular text-[15px] font-medium text-[var(--text-primary)]">
              {dict.batch.title} ({apps.length})
            </h2>
          </div>
          <button
            type="button"
            disabled={running}
            onClick={() => onClose(finished)}
            aria-label="Close"
            className="ui-btn-ghost h-7 w-7 rounded-full p-0"
          >
            <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          </button>
        </div>

        <div className="flex-1 space-y-4 overflow-y-auto px-6 py-5">
          <p className="text-[12.5px] text-[var(--text-secondary)]">{dict.batch.desc}</p>

          {!running && !finished && (
            <div className="grid grid-cols-2 gap-1 rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-1">
              <button
                type="button"
                onClick={() => setQuiet(false)}
                className={`inline-flex items-center justify-center gap-1.5 rounded-[7px] px-3 py-1.5 text-[12.5px] transition-colors cursor-pointer ${
                  !quiet
                    ? "bg-[var(--bg-surface)] font-medium text-[var(--text-primary)] shadow-[0_1px_2px_rgba(0,0,0,0.05)]"
                    : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                }`}
              >
                <Play className="h-3.5 w-3.5" strokeWidth={1.5} />
                {dict.batch.modeStandard}
              </button>
              <button
                type="button"
                onClick={() => setQuiet(true)}
                className={`inline-flex items-center justify-center gap-1.5 rounded-[7px] px-3 py-1.5 text-[12.5px] transition-colors cursor-pointer ${
                  quiet
                    ? "bg-[var(--bg-surface)] font-medium text-[var(--text-primary)] shadow-[0_1px_2px_rgba(0,0,0,0.05)]"
                    : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                }`}
              >
                <Zap className="h-3.5 w-3.5" strokeWidth={1.5} />
                {dict.batch.modeQuiet}
              </button>
            </div>
          )}

          <div className="divide-y divide-[var(--border-hairline)] rounded-[12px] border border-[var(--border-hairline)]">
            {queue.map((entry) => (
              <div
                key={entry.app.id}
                className="flex items-center justify-between gap-3 px-4 py-2.5"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-medium text-[var(--text-primary)]">
                    {entry.app.name}
                  </div>
                  {entry.detail && (
                    <div className="truncate text-[11.5px] text-[var(--text-secondary)]">
                      {entry.detail}
                    </div>
                  )}
                </div>
                <span className="flex shrink-0 items-center gap-1.5 text-[11.5px] font-medium">
                  {entry.status === "pending" && (
                    <>
                      <Clock
                        className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                        strokeWidth={1.5}
                      />
                      <span className="text-[var(--text-tertiary)]">
                        {dict.batch.statusPending}
                      </span>
                    </>
                  )}
                  {entry.status === "running" && (
                    <>
                      <Loader2
                        className="h-3.5 w-3.5 animate-spin text-[var(--text-primary)]"
                        strokeWidth={1.5}
                      />
                      <span className="text-[var(--text-primary)]">{dict.batch.statusRunning}</span>
                    </>
                  )}
                  {entry.status === "done" && (
                    <>
                      <CheckCircle2
                        className="h-3.5 w-3.5 text-[var(--success-text)]"
                        strokeWidth={1.5}
                      />
                      <span className="text-[var(--success-text)]">{dict.batch.statusDone}</span>
                    </>
                  )}
                  {entry.status === "error" && (
                    <>
                      <AlertCircle
                        className="h-3.5 w-3.5 text-[var(--danger-text)]"
                        strokeWidth={1.5}
                      />
                      <span className="text-[var(--danger-text)]">{dict.batch.statusError}</span>
                    </>
                  )}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-6 py-3.5">
          {!finished ? (
            <>
              <button
                type="button"
                disabled={running}
                onClick={() => onClose(false)}
                className="ui-btn-ghost h-8"
              >
                {dict.wizard.cancel}
              </button>
              <button
                type="button"
                disabled={running}
                onClick={handleStart}
                className="ui-btn-danger-subtle h-8"
              >
                {running ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" strokeWidth={1.5} />
                ) : (
                  <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                )}
                {running ? dict.batch.running : dict.batch.startQueue}
              </button>
            </>
          ) : (
            <>
              <button type="button" onClick={() => onClose(true)} className="ui-btn-ghost h-8">
                {dict.wizard.close}
              </button>
              {combinedLeftovers.length > 0 && (
                <button
                  type="button"
                  onClick={() =>
                    onReviewCombinedLeftovers(`Batch (${apps.length} apps)`, combinedLeftovers)
                  }
                  className="ui-btn-primary h-8"
                >
                  <span className="num-tabular">
                    {dict.batch.reviewLeftovers} ({combinedLeftovers.length})
                  </span>
                </button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
