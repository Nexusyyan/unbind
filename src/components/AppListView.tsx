import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowDownUp,
  Check,
  CheckSquare,
  ChevronDown,
  Download,
  ExternalLink,
  FolderOpen,
  Hammer,
  KeyRound,
  Lock,
  Package,
  RefreshCw,
  Search,
  Trash2,
  X,
  Zap,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type {
  AppFilterMode,
  AppSettings,
  AppSortField,
  ExportFormat,
  InstalledApp,
} from "../types";
import { filterAndSortApps, formatSizeKb } from "../utils/format";

interface AppListViewProps {
  dict: Dict;
  apps: InstalledApp[];
  loading: boolean;
  storeOnly: boolean;
  settings: AppSettings;
  icons: Record<string, string>;
  onRequestIcons: (ids: string[]) => void;
  onRefresh: () => void;
  onUninstall: (app: InstalledApp, quiet: boolean) => void;
  onForceUninstall: (app?: InstalledApp) => void;
  onBatchUninstall: (apps: InstalledApp[], quiet: boolean) => void;
  onExport: (format: ExportFormat, visibleApps: InstalledApp[]) => void;
  onCheckLocks: (app: InstalledApp) => void;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function AppListView({
  dict,
  apps,
  loading,
  storeOnly,
  settings,
  icons,
  onRequestIcons,
  onRefresh,
  onUninstall,
  onForceUninstall,
  onBatchUninstall,
  onExport,
  onCheckLocks,
  onNotify,
}: AppListViewProps) {
  const [query, setQuery] = useState("");
  const [filterMode, setFilterMode] = useState<AppFilterMode>("all");
  const [sortField, setSortField] = useState<AppSortField>("name");
  const [sortDesc, setSortDesc] = useState(false);
  const [selectedAppId, setSelectedAppId] = useState<string | null>(null);
  const [batchMode, setBatchMode] = useState(false);
  const [batchIds, setBatchIds] = useState<Set<string>>(new Set());
  const [exportOpen, setExportOpen] = useState(false);
  const [sortOpen, setSortOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    app: InstalledApp;
  } | null>(null);

  const searchRef = useRef<HTMLInputElement>(null);
  const parentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (e.key === "F5") {
        e.preventDefault();
        onRefresh();
      } else if (e.key === "Escape") {
        setContextMenu(null);
        setExportOpen(false);
        setSortOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onRefresh]);

  useEffect(() => {
    const dismiss = () => {
      setContextMenu(null);
      setExportOpen(false);
      setSortOpen(false);
    };
    window.addEventListener("click", dismiss);
    return () => window.removeEventListener("click", dismiss);
  }, []);

  const visibleApps = useMemo(
    () =>
      filterAndSortApps(apps, {
        storeOnly,
        query,
        filterMode,
        sortField,
        sortDesc,
        showSystemComponents: settings.showSystemComponents,
        showWindowsUpdates: settings.showWindowsUpdates,
      }),
    [apps, storeOnly, query, filterMode, sortField, sortDesc, settings],
  );

  const maxSizeKb = useMemo(
    () => Math.max(1, ...visibleApps.map((a) => a.estimatedSizeKb)),
    [visibleApps],
  );

  const selectedApp = useMemo(
    () => visibleApps.find((a) => a.id === selectedAppId) ?? null,
    [visibleApps, selectedAppId],
  );

  const virtualizer = useVirtualizer({
    count: visibleApps.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 62,
    overscan: 10,
  });

  const virtualItems = virtualizer.getVirtualItems();

  // Lazily request native PNG icons only for rows currently near the viewport.
  useEffect(() => {
    if (virtualItems.length === 0) return;
    const missing: string[] = [];
    for (const v of virtualItems) {
      const app = visibleApps[v.index];
      if (app && !(app.id in icons)) {
        missing.push(app.id);
      }
    }
    if (missing.length > 0) {
      onRequestIcons(missing);
    }
  }, [virtualItems, visibleApps, icons, onRequestIcons]);

  const toggleBatchId = (id: string) => {
    setBatchIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const selectedBatchApps = useMemo(
    () => visibleApps.filter((a) => batchIds.has(a.id)),
    [visibleApps, batchIds],
  );

  const handleOpenFolder = async (path: string) => {
    try {
      await api.openPathInExplorer(path);
    } catch (err) {
      onNotify("error", dict.appList.openFolder, String(err));
    }
  };

  const handleOpenReg = async (key: string) => {
    try {
      await api.openKeyInRegedit(key);
    } catch (err) {
      onNotify("error", dict.appList.openRegedit, String(err));
    }
  };

  const handleOpenUrl = async (url: string) => {
    try {
      await api.openExternalUrl(url);
    } catch (err) {
      onNotify("error", dict.appList.openWebsite, String(err));
    }
  };

  const filters: Array<{ id: AppFilterMode; label: string }> = storeOnly
    ? [
        { id: "all", label: dict.toolbar.filterAll },
        { id: "recent", label: dict.toolbar.filterRecent },
        { id: "system", label: dict.toolbar.filterSystem },
      ]
    : [
        { id: "all", label: dict.toolbar.filterAll },
        { id: "recent", label: dict.toolbar.filterRecent },
        { id: "large", label: dict.toolbar.filterLarge },
        { id: "broken", label: dict.toolbar.filterBroken },
        { id: "system", label: dict.toolbar.filterSystem },
      ];

  const sortOptions: Array<{ id: AppSortField; label: string }> = [
    { id: "name", label: dict.toolbar.sortName },
    { id: "size", label: dict.toolbar.sortSize },
    { id: "date", label: dict.toolbar.sortDate },
    { id: "publisher", label: dict.toolbar.sortPublisher },
  ];

  const currentSortLabel =
    sortOptions.find((opt) => opt.id === sortField)?.label ?? dict.toolbar.sortName;

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col bg-[var(--bg-canvas)]">
      {/* Top Toolbar: floating search bar + borderless ghost controls */}
      <div className="flex flex-col gap-3 px-6 pt-4 pb-2.5">
        <div className="flex flex-wrap items-center gap-2">
          <div className="ui-search-float relative flex h-9 min-w-[240px] flex-1 items-center">
            <Search
              className="pointer-events-none absolute left-3.5 h-[15px] w-[15px] text-[var(--text-tertiary)]"
              strokeWidth={1.5}
            />
            <input
              ref={searchRef}
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={dict.toolbar.searchPlaceholder}
              className="h-full w-full bg-transparent pr-8 pl-9.5 text-[13px] text-[var(--text-primary)] placeholder:text-[var(--text-tertiary)] outline-none"
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery("")}
                aria-label="Clear search"
                className="absolute right-2 flex h-5 w-5 items-center justify-center rounded-full text-[var(--text-tertiary)] transition-colors hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)] cursor-pointer"
              >
                <X className="h-3.5 w-3.5" strokeWidth={1.5} />
              </button>
            )}
          </div>

          <div className="flex items-center gap-0.5">
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

            {!storeOnly && (
              <button
                type="button"
                onClick={() => onForceUninstall()}
                className="ui-btn-ghost h-8 px-2.5"
              >
                <Hammer
                  className="h-[14px] w-[14px] text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
                <span>{dict.toolbar.forceUninstall}</span>
              </button>
            )}

            <button
              type="button"
              onClick={() => {
                setBatchMode((m) => !m);
                setBatchIds(new Set());
              }}
              className={`h-8 px-2.5 ${batchMode ? "ui-btn-primary" : "ui-btn-ghost"}`}
            >
              <CheckSquare
                className={`h-[14px] w-[14px] ${batchMode ? "" : "text-[var(--text-tertiary)]"}`}
                strokeWidth={1.5}
              />
              <span>{batchMode ? dict.toolbar.exitBatch : dict.toolbar.batchMode}</span>
            </button>

            <div className="relative">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  setSortOpen(false);
                  setExportOpen((v) => !v);
                }}
                className="ui-btn-ghost h-8 px-2.5"
              >
                <Download
                  className="h-[14px] w-[14px] text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
                <span>{dict.toolbar.export}</span>
              </button>
              {exportOpen && (
                <div className="ui-popover absolute right-0 z-30 mt-1.5 w-38 p-1.5">
                  {(["html", "csv", "json"] as ExportFormat[]).map((fmt) => (
                    <button
                      key={fmt}
                      type="button"
                      onClick={() => {
                        setExportOpen(false);
                        onExport(fmt, visibleApps);
                      }}
                      className="flex w-full items-center rounded-[8px] px-2.5 py-1.5 text-left text-[12.5px] text-[var(--text-primary)] transition-colors hover:bg-[var(--bg-hover)] cursor-pointer"
                    >
                      Export .{fmt.toUpperCase()}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Lightweight Flat Filter Tabs & Minimal Sort Controls */}
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1">
            {filters.map((f) => {
              const active = filterMode === f.id;
              return (
                <button
                  key={f.id}
                  type="button"
                  onClick={() => setFilterMode(f.id)}
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
              {visibleApps.length} {dict.status.programs}
            </span>
          </div>

          <div className="flex items-center gap-0.5">
            <div className="relative">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  setExportOpen(false);
                  setSortOpen((v) => !v);
                }}
                className="ui-btn-ghost h-7 px-2 text-[12px]"
              >
                <span>{currentSortLabel}</span>
                <ChevronDown
                  className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
              </button>
              {sortOpen && (
                <div className="ui-popover absolute right-0 z-30 mt-1.5 w-36 p-1.5">
                  {sortOptions.map((opt) => {
                    const active = sortField === opt.id;
                    return (
                      <button
                        key={opt.id}
                        type="button"
                        onClick={() => {
                          setSortField(opt.id);
                          setSortOpen(false);
                        }}
                        className={`flex w-full items-center justify-between rounded-[8px] px-2.5 py-1.5 text-left text-[12px] transition-colors cursor-pointer ${
                          active
                            ? "bg-[var(--bg-selected)] font-medium text-[var(--text-primary)]"
                            : "text-[var(--text-secondary)] hover:bg-[var(--bg-hover)] hover:text-[var(--text-primary)]"
                        }`}
                      >
                        <span>{opt.label}</span>
                        {active && <Check className="h-3.5 w-3.5" strokeWidth={1.5} />}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>

            <button
              type="button"
              onClick={() => setSortDesc((d) => !d)}
              className="ui-btn-ghost h-7 px-2 text-[11.5px]"
              title={sortDesc ? "Descending" : "Ascending"}
            >
              <ArrowDownUp
                className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <span className="num-tabular">{sortDesc ? "DESC" : "ASC"}</span>
            </button>
          </div>
        </div>

        {/* Batch Action Bar (Neutral, Zero Blue) */}
        {batchMode && (
          <div className="flex items-center justify-between rounded-[10px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-3.5 py-2">
            <span className="num-tabular text-[12.5px] font-medium text-[var(--text-primary)]">
              {selectedBatchApps.length} {dict.toolbar.selectedCount}
            </span>
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                disabled={selectedBatchApps.length === 0}
                onClick={() => onBatchUninstall(selectedBatchApps, false)}
                className="ui-btn-danger-subtle h-7 px-2.5 text-[12px]"
              >
                <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                {dict.toolbar.batchUninstall}
              </button>
              <button
                type="button"
                disabled={selectedBatchApps.length === 0}
                onClick={() => onBatchUninstall(selectedBatchApps, true)}
                className="ui-btn-primary h-7 px-2.5 text-[12px]"
              >
                <Zap className="h-3.5 w-3.5" strokeWidth={1.5} />
                {dict.toolbar.batchQuiet}
              </button>
              <button
                type="button"
                onClick={() => setBatchIds(new Set())}
                className="ui-btn-ghost h-7 px-2.5 text-[12px]"
              >
                {dict.toolbar.clearSelection}
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Main Content Area: Virtualized List + Optional Inspector Drawer */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <div className="flex min-w-0 flex-1 flex-col">
          {/* Borderless Minimal Table Header (no gray fill strip) */}
          <div className="grid grid-cols-[minmax(200px,1fr)_104px_98px_108px_108px] items-center gap-3 border-b border-[var(--border-hairline)] py-2 pr-[30px] pl-6 text-[11.5px] font-medium text-[var(--text-tertiary)] select-none">
            <div>{dict.appList.colName}</div>
            <div>{dict.appList.colVersion}</div>
            <div>{dict.appList.colDate}</div>
            <div className="text-right">{dict.appList.colSize}</div>
            <div className="text-right">{dict.appList.colActions}</div>
          </div>

          {loading && apps.length === 0 ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-3 text-[var(--text-secondary)]">
              <RefreshCw
                className="h-5 w-5 animate-spin text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <span className="text-[12.5px]">{dict.appList.loading}</span>
            </div>
          ) : visibleApps.length === 0 ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 p-10 text-center">
              <Package
                className="h-9 w-9 text-[var(--text-tertiary)] opacity-60"
                strokeWidth={1.5}
              />
              <div className="text-[14px] font-medium text-[var(--text-primary)]">
                {dict.appList.emptyTitle}
              </div>
              <div className="max-w-sm text-[12.5px] text-[var(--text-secondary)]">
                {dict.appList.emptyDesc}
              </div>
            </div>
          ) : (
            <div
              ref={parentRef}
              className="min-h-0 flex-1 overflow-y-auto px-3 py-1.5 [scrollbar-gutter:stable]"
            >
              <div
                style={{
                  height: `${virtualizer.getTotalSize()}px`,
                  width: "100%",
                  position: "relative",
                }}
              >
                {virtualItems.map((virtualRow) => {
                  const app = visibleApps[virtualRow.index];
                  const isSelected = selectedAppId === app.id;
                  const isChecked = batchIds.has(app.id);
                  const iconSrc = icons[app.id];
                  const sizePct =
                    app.estimatedSizeKb > 0
                      ? Math.min(
                          100,
                          Math.round(
                            (Math.log10(app.estimatedSizeKb + 10) /
                              Math.log10(maxSizeKb + 10)) *
                              100,
                          ),
                        )
                      : 0;

                  return (
                    <div
                      key={app.id}
                      onClick={() => {
                        if (batchMode) toggleBatchId(app.id);
                        else setSelectedAppId(app.id);
                      }}
                      onDoubleClick={() => onUninstall(app, false)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setSelectedAppId(app.id);
                        setContextMenu({ x: e.clientX, y: e.clientY, app });
                      }}
                      style={{
                        position: "absolute",
                        top: 0,
                        left: 0,
                        width: "100%",
                        height: `${virtualRow.size - 2}px`,
                        transform: `translateY(${virtualRow.start}px)`,
                      }}
                      className={`group grid grid-cols-[minmax(200px,1fr)_104px_98px_108px_108px] items-center gap-3 rounded-[10px] px-3 transition-colors duration-150 cursor-pointer ${
                        isSelected || isChecked
                          ? "bg-[var(--bg-selected)]"
                          : "hover:bg-[var(--bg-hover)]"
                      }`}
                    >
                      {/* App Icon (full native color preserved) + Name + Publisher */}
                      <div className="flex min-w-0 items-center gap-3">
                        {batchMode && (
                          <input
                            type="checkbox"
                            checked={isChecked}
                            onChange={() => toggleBatchId(app.id)}
                            onClick={(e) => e.stopPropagation()}
                            className="ui-checkbox"
                          />
                        )}
                        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-[10px] bg-[var(--bg-subtle)]">
                          {iconSrc ? (
                            <img
                              src={iconSrc}
                              alt=""
                              className="h-6 w-6 object-contain"
                              loading="lazy"
                            />
                          ) : (
                            <Package
                              className="h-4 w-4 text-[var(--text-tertiary)]"
                              strokeWidth={1.5}
                            />
                          )}
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="flex items-center gap-1.5">
                            <span className="truncate text-[13px] font-medium text-[var(--text-primary)]">
                              {app.name}
                            </span>
                            {app.trackedTraceId && (
                              <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-[var(--bg-subtle)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--text-secondary)]">
                                <span className="h-1.5 w-1.5 rounded-full bg-[var(--success-text)]" />
                                {dict.appList.trackedBadge}
                              </span>
                            )}
                            {!app.isValidEntry && (
                              <span className="inline-flex shrink-0 items-center rounded-full bg-[var(--danger-bg-hover)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--danger-text)]">
                                {dict.appList.brokenBadge}
                              </span>
                            )}
                            {app.kind === "msi" && (
                              <span className="shrink-0 rounded-full bg-[var(--bg-subtle)] px-1.5 py-0.5 text-[10px] text-[var(--text-tertiary)]">
                                MSI
                              </span>
                            )}
                          </div>
                          <div className="mt-0.5 truncate text-[12px] text-[var(--text-secondary)]">
                            {app.publisher || dict.inspector.unknown}
                          </div>
                        </div>
                      </div>

                      {/* Version */}
                      <div
                        className={`num-tabular truncate text-[12px] ${
                          app.version ? "text-[var(--text-secondary)]" : "text-[var(--text-tertiary)]"
                        }`}
                      >
                        {app.version || "-"}
                      </div>

                      {/* Install Date */}
                      <div
                        className={`num-tabular truncate text-[12px] ${
                          app.installDate
                            ? "text-[var(--text-secondary)]"
                            : "text-[var(--text-tertiary)]"
                        }`}
                      >
                        {app.installDate || "-"}
                      </div>

                      {/* Size + 3px Ultra-Thin Neutral Progress Bar (Log Scaled) */}
                      <div className="flex flex-col items-end justify-center">
                        <span
                          className={`num-tabular text-[12.5px] ${
                            app.estimatedSizeKb > 0
                              ? "font-medium text-[var(--text-primary)]"
                              : "font-normal text-[var(--text-tertiary)]"
                          }`}
                        >
                          {formatSizeKb(app.estimatedSizeKb)}
                        </span>
                        {app.estimatedSizeKb > 0 && (
                          <div className="mt-1.5 h-[3px] w-16 overflow-hidden rounded-full bg-[var(--bg-track)]">
                            <div
                              className="h-full rounded-full bg-[var(--bar-fill)]"
                              style={{ width: `${Math.max(6, sizePct)}%` }}
                            />
                          </div>
                        )}
                      </div>

                      {/* Hover-Reveal Quick Actions (hidden at rest, smooth fade on row hover) */}
                      <div
                        className="flex items-center justify-end gap-1 opacity-0 transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <button
                          type="button"
                          onClick={() => onUninstall(app, false)}
                          className="ui-btn-danger-ghost h-7 px-2.5 text-[12px]"
                        >
                          {dict.appList.uninstall}
                        </button>
                        {app.installLocation && (
                          <button
                            type="button"
                            onClick={() => handleOpenFolder(app.installLocation)}
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
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </div>

        {/* Right Inspector Drawer (secondary neutral container #F9F9FA) */}
        {selectedApp && (
          <aside className="flex w-[312px] shrink-0 flex-col overflow-y-auto border-l border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-5">
            <div className="flex items-start justify-between gap-2">
              <div className="flex min-w-0 items-center gap-3">
                <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-[12px] border border-[var(--border-hairline)] bg-[var(--bg-surface)]">
                  {icons[selectedApp.id] ? (
                    <img
                      src={icons[selectedApp.id]}
                      alt=""
                      className="h-7 w-7 object-contain"
                    />
                  ) : (
                    <Package
                      className="h-5 w-5 text-[var(--text-tertiary)]"
                      strokeWidth={1.5}
                    />
                  )}
                </div>
                <div className="min-w-0">
                  <h3 className="truncate text-[14px] font-medium text-[var(--text-primary)]">
                    {selectedApp.name}
                  </h3>
                  <p className="mt-0.5 truncate text-[12px] text-[var(--text-secondary)]">
                    {selectedApp.publisher || dict.inspector.unknown}
                  </p>
                </div>
              </div>
              <button
                type="button"
                onClick={() => setSelectedAppId(null)}
                aria-label="Close inspector"
                className="ui-btn-ghost h-7 w-7 rounded-full p-0"
              >
                <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
              </button>
            </div>

            {/* Action Buttons */}
            <div className="mt-5 grid grid-cols-2 gap-2">
              <button
                type="button"
                onClick={() => onUninstall(selectedApp, false)}
                className="ui-btn-danger-subtle h-8"
              >
                <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                <span>{dict.appList.uninstall}</span>
              </button>
              <button
                type="button"
                disabled={!selectedApp.quietUninstallString}
                onClick={() => onUninstall(selectedApp, true)}
                className="ui-btn-secondary h-8"
              >
                <Zap className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
                <span>{dict.appList.quietUninstall}</span>
              </button>
              {selectedApp.kind !== "storeApp" && (
                <button
                  type="button"
                  onClick={() => onForceUninstall(selectedApp)}
                  className="ui-btn-secondary col-span-2 h-8"
                >
                  <Hammer
                    className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                  <span>{dict.appList.forceRemove}</span>
                </button>
              )}
            </div>

            {/* Secondary Shortcut Links */}
            <div className="mt-3 flex flex-wrap gap-1">
              {selectedApp.installLocation && (
                <button
                  type="button"
                  onClick={() => handleOpenFolder(selectedApp.installLocation)}
                  className="ui-btn-ghost h-7 px-2 text-[11.5px]"
                >
                  <FolderOpen
                    className="h-3 w-3 text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                  {dict.appList.openFolder}
                </button>
              )}
              {selectedApp.registryKey && (
                <button
                  type="button"
                  onClick={() => handleOpenReg(selectedApp.registryKey)}
                  className="ui-btn-ghost h-7 px-2 text-[11.5px]"
                >
                  <KeyRound
                    className="h-3 w-3 text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                  {dict.appList.openRegedit}
                </button>
              )}
              {selectedApp.installLocation && (
                <button
                  type="button"
                  onClick={() => onCheckLocks(selectedApp)}
                  className="ui-btn-ghost h-7 px-2 text-[11.5px]"
                >
                  <Lock className="h-3 w-3 text-[var(--text-tertiary)]" strokeWidth={1.5} />
                  {dict.appList.checkLocks}
                </button>
              )}
              {selectedApp.urlInfoAbout && (
                <button
                  type="button"
                  onClick={() => handleOpenUrl(selectedApp.urlInfoAbout)}
                  className="ui-btn-ghost h-7 px-2 text-[11.5px]"
                >
                  <ExternalLink
                    className="h-3 w-3 text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                  {dict.appList.openWebsite}
                </button>
              )}
            </div>

            {/* Metadata Details */}
            <div className="mt-4 space-y-3 border-t border-[var(--border-hairline)] pt-4 text-[12px]">
              <MetaRow label={dict.inspector.version} value={selectedApp.version || "-"} tabular />
              <MetaRow
                label={dict.inspector.installDate}
                value={selectedApp.installDate || "-"}
                tabular
              />
              <MetaRow
                label={dict.inspector.size}
                value={formatSizeKb(selectedApp.estimatedSizeKb)}
                tabular
              />
              <MetaRow
                label={dict.inspector.typeScope}
                value={`${selectedApp.kind.toUpperCase()} / ${selectedApp.scope} / ${selectedApp.arch}`}
              />
              {selectedApp.installLocation && (
                <MetaRow
                  label={dict.inspector.installLocation}
                  value={selectedApp.installLocation}
                  mono
                />
              )}
              {selectedApp.registryKey && (
                <MetaRow
                  label={dict.inspector.registryKey}
                  value={selectedApp.registryKey}
                  mono
                />
              )}
              {selectedApp.productCode && (
                <MetaRow
                  label={dict.inspector.productCode}
                  value={selectedApp.productCode}
                  mono
                />
              )}
              {selectedApp.packageFullName && (
                <MetaRow
                  label={dict.inspector.packageName}
                  value={selectedApp.packageFullName}
                  mono
                />
              )}
              {selectedApp.uninstallString && (
                <MetaRow
                  label={dict.inspector.uninstallCmd}
                  value={selectedApp.uninstallString}
                  mono
                />
              )}
              {selectedApp.quietUninstallString && (
                <MetaRow
                  label={dict.inspector.quietCmd}
                  value={selectedApp.quietUninstallString}
                  mono
                />
              )}
            </div>
          </aside>
        )}
      </div>

      {/* Context Menu */}
      {contextMenu && (
        <div
          style={{
            top: Math.min(contextMenu.y, window.innerHeight - 250),
            left: Math.min(contextMenu.x, window.innerWidth - 220),
          }}
          className="ui-popover fixed z-50 w-52 p-1.5"
        >
          <CtxItem
            icon={Trash2}
            label={dict.appList.uninstall}
            danger
            onClick={() => onUninstall(contextMenu.app, false)}
          />
          {contextMenu.app.quietUninstallString && (
            <CtxItem
              icon={Zap}
              label={dict.appList.quietUninstall}
              onClick={() => onUninstall(contextMenu.app, true)}
            />
          )}
          {contextMenu.app.kind !== "storeApp" && (
            <CtxItem
              icon={Hammer}
              label={dict.appList.forceRemove}
              onClick={() => onForceUninstall(contextMenu.app)}
            />
          )}
          <div className="my-1.5 h-px bg-[var(--border-hairline)]" />
          {contextMenu.app.installLocation && (
            <CtxItem
              icon={FolderOpen}
              label={dict.appList.openFolder}
              onClick={() => handleOpenFolder(contextMenu.app.installLocation)}
            />
          )}
          {contextMenu.app.registryKey && (
            <CtxItem
              icon={KeyRound}
              label={dict.appList.openRegedit}
              onClick={() => handleOpenReg(contextMenu.app.registryKey)}
            />
          )}
          {contextMenu.app.installLocation && (
            <CtxItem
              icon={Lock}
              label={dict.appList.checkLocks}
              onClick={() => onCheckLocks(contextMenu.app)}
            />
          )}
          {contextMenu.app.urlInfoAbout && (
            <CtxItem
              icon={ExternalLink}
              label={dict.appList.openWebsite}
              onClick={() => handleOpenUrl(contextMenu.app.urlInfoAbout)}
            />
          )}
        </div>
      )}
    </div>
  );
}

function MetaRow({
  label,
  value,
  mono,
  tabular,
}: {
  label: string;
  value: string;
  mono?: boolean;
  tabular?: boolean;
}) {
  return (
    <div>
      <div className="text-[11px] font-medium text-[var(--text-tertiary)]">{label}</div>
      <div
        className={`mt-0.5 break-all select-text ${
          mono
            ? "font-mono text-[11px] text-[var(--text-secondary)]"
            : `text-[12.5px] text-[var(--text-primary)] ${tabular ? "num-tabular" : ""}`
        }`}
      >
        {value}
      </div>
    </div>
  );
}

function CtxItem({
  icon: Icon,
  label,
  danger,
  onClick,
}: {
  icon: typeof Trash2;
  label: string;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-center gap-2.5 rounded-[8px] px-2.5 py-1.5 text-left text-[12.5px] transition-colors duration-150 cursor-pointer ${
        danger
          ? "text-[var(--text-primary)] hover:bg-[var(--danger-bg-hover)] hover:text-[var(--danger-text)]"
          : "text-[var(--text-primary)] hover:bg-[var(--bg-hover)]"
      }`}
    >
      <Icon className="h-3.5 w-3.5 opacity-70" strokeWidth={1.5} />
      <span>{label}</span>
    </button>
  );
}
