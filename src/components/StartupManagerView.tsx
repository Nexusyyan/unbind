import { useMemo, useState } from "react";
import {
  FolderOpen,
  KeyRound,
  Power,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { StartupItem, StartupSource } from "../types";

interface StartupManagerViewProps {
  dict: Dict;
  items: StartupItem[];
  loading: boolean;
  onRefresh: () => void;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

type StartupFilter = "all" | "enabled" | "disabled" | "invalid";

function sourceLabel(source: StartupSource): string {
  switch (source) {
    case "registryCurrentUserRun":
      return "HKCU Run";
    case "registryCurrentUserRunOnce":
      return "HKCU RunOnce";
    case "registryLocalMachineRun":
      return "HKLM Run (64)";
    case "registryLocalMachineWow64Run":
      return "HKLM Run (32)";
    case "startupFolderUser":
      return "User Startup";
    case "startupFolderCommon":
      return "Common Startup";
    case "scheduledTask":
      return "Scheduled Task";
  }
}

export function StartupManagerView({
  dict,
  items,
  loading,
  onRefresh,
  onNotify,
}: StartupManagerViewProps) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<StartupFilter>("all");
  const [busyId, setBusyId] = useState<string | null>(null);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return items.filter((item) => {
      if (filter === "enabled" && !item.enabled) return false;
      if (filter === "disabled" && item.enabled) return false;
      if (filter === "invalid" && item.isValidTarget) return false;
      if (q) {
        const hay = `${item.name} ${item.command} ${item.location} ${sourceLabel(item.source)}`.toLowerCase();
        return hay.includes(q);
      }
      return true;
    });
  }, [items, query, filter]);

  const handleToggle = async (item: StartupItem) => {
    setBusyId(item.id);
    try {
      await api.setStartupEnabled(item, !item.enabled);
      onNotify(
        "success",
        item.name,
        !item.enabled ? dict.startup.enabled : dict.startup.disabled,
      );
      onRefresh();
    } catch (err) {
      onNotify("error", item.name, String(err));
    } finally {
      setBusyId(null);
    }
  };

  const handleDelete = async (item: StartupItem) => {
    if (!window.confirm(`${dict.startup.deleteConfirm}\n\n${item.name}\n${item.location}`)) {
      return;
    }
    setBusyId(item.id);
    try {
      await api.deleteStartupItem(item);
      onNotify("success", item.name, "Removed from startup");
      onRefresh();
    } catch (err) {
      onNotify("error", item.name, String(err));
    } finally {
      setBusyId(null);
    }
  };

  const filters: Array<{ id: StartupFilter; label: string }> = [
    { id: "all", label: dict.startup.filterAll },
    { id: "enabled", label: dict.startup.filterEnabled },
    { id: "disabled", label: dict.startup.filterDisabled },
    { id: "invalid", label: dict.startup.filterInvalid },
  ];

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col bg-[var(--bg-canvas)]">
      {/* Header & Controls */}
      <div className="space-y-3 px-6 pt-5 pb-2.5">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h1 className="text-[17px] font-medium tracking-[-0.015em] text-[var(--text-primary)]">
              {dict.startup.title}
            </h1>
            <p className="mt-0.5 text-[12.5px] text-[var(--text-secondary)]">
              {dict.startup.subtitle}
            </p>
          </div>
          <button
            type="button"
            onClick={onRefresh}
            disabled={loading}
            className="ui-btn-ghost h-8 px-2.5"
          >
            <RefreshCw
              className={`h-[14px] w-[14px] text-[var(--text-tertiary)] ${loading ? "animate-spin" : ""}`}
              strokeWidth={1.5}
            />
            <span>{dict.toolbar.refresh}</span>
          </button>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="ui-search-float relative flex h-9 min-w-[240px] max-w-md flex-1 items-center">
            <Search
              className="pointer-events-none absolute left-3.5 h-[15px] w-[15px] text-[var(--text-tertiary)]"
              strokeWidth={1.5}
            />
            <input
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={dict.startup.searchPlaceholder}
              className="h-full w-full bg-transparent pr-8 pl-9.5 text-[13px] text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] outline-none"
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery("")}
                aria-label="Clear search"
                className="absolute right-2 flex h-5 w-5 items-center justify-center rounded-full text-[var(--text-tertiary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] cursor-pointer"
              >
                <X className="h-3.5 w-3.5" strokeWidth={1.5} />
              </button>
            )}
          </div>

          <div className="flex items-center gap-1">
            {filters.map((f) => {
              const active = filter === f.id;
              return (
                <button
                  key={f.id}
                  type="button"
                  onClick={() => setFilter(f.id)}
                  className={`rounded-[8px] px-2.5 py-1 text-[12px] transition-colors duration-150 cursor-pointer ${
                    active
                      ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                      : "font-normal text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                  }`}
                >
                  {f.label}
                </button>
              );
            })}
            <span className="num-tabular ml-2 text-[12px] text-[var(--text-tertiary)]">
              {visible.length}
            </span>
          </div>
        </div>
      </div>

      {/* Borderless Table Header */}
      <div className="grid grid-cols-[210px_136px_1fr_84px_108px] items-center gap-3 border-b border-[var(--border-hairline)] py-2 pr-[30px] pl-6 text-[11.5px] font-medium text-[var(--text-tertiary)] select-none">
        <div>{dict.startup.colName}</div>
        <div>{dict.startup.colSource}</div>
        <div>{dict.startup.colCommand}</div>
        <div>{dict.startup.colStatus}</div>
        <div className="text-right">{dict.appList.colActions}</div>
      </div>

      {/* Startup Items List */}
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-1.5 [scrollbar-gutter:stable]">
        {visible.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-[var(--text-tertiary)]">
            <Power className="h-8 w-8 opacity-50" strokeWidth={1.5} />
            <div className="mt-2.5 text-[13px] text-[var(--text-secondary)]">
              No startup items match current filter
            </div>
          </div>
        ) : (
          <div className="space-y-0.5">
            {visible.map((item) => {
              const isBusy = busyId === item.id;
              const isReg = item.source.startsWith("registry");
              return (
                <div
                  key={item.id}
                  className="group grid grid-cols-[210px_136px_1fr_84px_108px] items-center gap-3 rounded-[10px] px-3 py-2.5 transition-colors duration-150 hover:bg-[var(--bg-hover)]"
                >
                  <div className="min-w-0">
                    <div className="flex items-center gap-1.5">
                      <span className="truncate text-[13px] font-medium text-[var(--text-primary)]">
                        {item.name}
                      </span>
                      {!item.isValidTarget && (
                        <span
                          title={dict.startup.invalidTarget}
                          className="inline-flex shrink-0 items-center rounded-full bg-[var(--danger-bg-hover)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--danger-text)]"
                        >
                          {dict.appList.brokenBadge}
                        </span>
                      )}
                    </div>
                  </div>

                  <div>
                    <span className="inline-flex rounded-[6px] bg-[var(--bg-subtle)] px-2 py-0.5 text-[11px] font-medium text-[var(--text-secondary)]">
                      {sourceLabel(item.source)}
                    </span>
                  </div>

                  <div
                    title={item.command}
                    className="truncate font-mono text-[11.5px] text-[var(--text-secondary)]"
                  >
                    {item.command || item.location}
                  </div>

                  <div>
                    <button
                      type="button"
                      role="switch"
                      aria-checked={item.enabled}
                      disabled={isBusy}
                      onClick={() => handleToggle(item)}
                      className={`relative inline-flex h-[18px] w-[32px] items-center rounded-full transition-colors duration-150 cursor-pointer disabled:opacity-45 ${
                        item.enabled
                          ? "bg-[var(--accent-primary)]"
                          : "bg-[var(--bg-track)]"
                      }`}
                      title={item.enabled ? dict.startup.enabled : dict.startup.disabled}
                    >
                      <span
                        className={`inline-block h-[14px] w-[14px] transform rounded-full transition-transform duration-150 ${
                          item.enabled
                            ? "translate-x-[16px] bg-[var(--accent-primary-fg)]"
                            : "translate-x-[2px] bg-[var(--bg-surface)] shadow-xs"
                        }`}
                      />
                    </button>
                  </div>

                  <div className="flex items-center justify-end gap-0.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100">
                    {item.executablePath && (
                      <button
                        type="button"
                        onClick={() =>
                          api.openPathInExplorer(item.executablePath).catch((e) =>
                            onNotify("error", item.name, String(e)),
                          )
                        }
                        title={dict.appList.openFolder}
                        aria-label={dict.appList.openFolder}
                        className="ui-btn-ghost h-7 w-7 p-0"
                      >
                        <FolderOpen
                          className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                          strokeWidth={1.5}
                        />
                      </button>
                    )}
                    {isReg && (
                      <button
                        type="button"
                        onClick={() =>
                          api.openKeyInRegedit(item.location).catch((e) =>
                            onNotify("error", item.name, String(e)),
                          )
                        }
                        title={dict.appList.openRegedit}
                        aria-label={dict.appList.openRegedit}
                        className="ui-btn-ghost h-7 w-7 p-0"
                      >
                        <KeyRound
                          className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                          strokeWidth={1.5}
                        />
                      </button>
                    )}
                    <button
                      type="button"
                      disabled={isBusy}
                      onClick={() => handleDelete(item)}
                      title="Delete"
                      aria-label="Delete"
                      className="ui-btn-danger-ghost h-7 w-7 p-0"
                    >
                      <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
