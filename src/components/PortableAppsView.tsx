import { useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDownUp,
  Check,
  ChevronDown,
  FolderOpen,
  FolderPlus,
  Link2,
  Package,
  Play,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { PortableApp, ShortcutLocation } from "../types";
import { formatBytes } from "../utils/format";

type PortableSortField = "name" | "size" | "modified";

interface PortableAppsViewProps {
  dict: Dict;
  roots: string[];
  apps: PortableApp[];
  loading: boolean;
  icons: Record<string, string>;
  onAddRoot: () => Promise<void>;
  onRemoveRoot: (root: string) => Promise<void>;
  onRefresh: () => void;
  onDeepRemove: (targetPath: string) => Promise<void>;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function PortableAppsView({
  dict,
  roots,
  apps,
  loading,
  icons,
  onAddRoot,
  onRemoveRoot,
  onRefresh,
  onDeepRemove,
  onNotify,
}: PortableAppsViewProps) {
  const [query, setQuery] = useState("");
  const [sortField, setSortField] = useState<PortableSortField>("name");
  const [sortDesc, setSortDesc] = useState(false);
  const [sortOpen, setSortOpen] = useState(false);
  const [shortcutMenuId, setShortcutMenuId] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      } else if (e.key === "F5") {
        e.preventDefault();
        onRefresh();
      } else if (e.key === "Escape") {
        setSortOpen(false);
        setShortcutMenuId(null);
      }
    };
    const dismiss = () => {
      setSortOpen(false);
      setShortcutMenuId(null);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("click", dismiss);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("click", dismiss);
    };
  }, [onRefresh]);

  const visible = useMemo(() => {
    const tokens = query
      .trim()
      .toLowerCase()
      .split(/\s+/)
      .filter(Boolean);
    const filtered = apps.filter((app) => {
      if (tokens.length === 0) return true;
      const hay = `${app.name} ${app.publisher} ${app.version} ${app.exePath} ${app.installDir}`.toLowerCase();
      return tokens.every((t) => hay.includes(t));
    });
    const dir = sortDesc ? -1 : 1;
    return [...filtered].sort((a, b) => {
      switch (sortField) {
        case "size":
          return (a.sizeBytes - b.sizeBytes) * dir || a.name.localeCompare(b.name);
        case "modified":
          return a.lastModified.localeCompare(b.lastModified) * dir || a.name.localeCompare(b.name);
        case "name":
        default:
          return a.name.toLowerCase().localeCompare(b.name.toLowerCase()) * dir;
      }
    });
  }, [apps, query, sortField, sortDesc]);

  const maxSizeBytes = useMemo(
    () => Math.max(1, ...visible.map((a) => a.sizeBytes)),
    [visible],
  );

  const handleLaunch = async (app: PortableApp) => {
    try {
      await api.launchPortableApp(app.exePath, app.installDir);
      onNotify("success", dict.portable.launchedToast, app.name);
    } catch (err) {
      onNotify("error", app.name, String(err));
    }
  };

  const handleCreateShortcut = async (app: PortableApp, location: ShortcutLocation) => {
    setShortcutMenuId(null);
    try {
      const created = await api.createPortableShortcut(app.exePath, app.name, location);
      onNotify("success", dict.portable.shortcutCreated, created);
    } catch (err) {
      onNotify("error", dict.portable.createShortcut, String(err));
    }
  };

  const sortOptions: Array<{ id: PortableSortField; label: string }> = [
    { id: "name", label: dict.portable.sortName },
    { id: "size", label: dict.portable.sortSize },
    { id: "modified", label: dict.portable.sortModified },
  ];
  const currentSortLabel =
    sortOptions.find((o) => o.id === sortField)?.label ?? dict.portable.sortName;

  if (roots.length === 0) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center px-6 py-16 text-center">
        <div className="flex h-12 w-12 items-center justify-center rounded-[14px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)]">
          <Package className="h-5 w-5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
        </div>
        <h2 className="mt-4 text-[14.5px] font-medium text-[var(--text-primary)]">
          {dict.portable.emptyRootsTitle}
        </h2>
        <p className="mt-1.5 max-w-md text-[12.5px] leading-relaxed text-[var(--text-secondary)]">
          {dict.portable.emptyRootsDesc}
        </p>
        <button
          type="button"
          onClick={() => void onAddRoot()}
          className="ui-btn-primary mt-5 h-8.5 px-3.5"
        >
          <FolderPlus className="h-3.5 w-3.5" strokeWidth={1.5} />
          <span>{dict.portable.addRoot}</span>
        </button>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col bg-[var(--bg-canvas)]">
      {/* Controls & Library Roots */}
      <div className="space-y-2.5 px-6 pt-3 pb-2.5">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-1.5">
            <span className="text-[11.5px] font-medium text-[var(--text-tertiary)]">
              {dict.portable.rootsLabel}:
            </span>
            {roots.map((root) => (
              <span
                key={root}
                className="inline-flex items-center gap-1.5 rounded-[8px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-2 py-0.5 font-mono text-[11px] text-[var(--text-secondary)]"
              >
                <span className="max-w-[260px] truncate" title={root}>
                  {root}
                </span>
                <button
                  type="button"
                  onClick={() => void onRemoveRoot(root)}
                  aria-label="Remove folder"
                  className="text-[var(--text-tertiary)] hover:text-[var(--text-primary)] cursor-pointer"
                >
                  <X className="h-3 w-3" strokeWidth={1.5} />
                </button>
              </span>
            ))}
          </div>

          <div className="flex items-center gap-0.5">
            <button
              type="button"
              onClick={() => void onAddRoot()}
              className="ui-btn-ghost h-8 px-2.5"
            >
              <FolderPlus className="h-3.5 w-3.5 text-[var(--text-tertiary)]" strokeWidth={1.5} />
              <span>{dict.portable.addRoot}</span>
            </button>
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
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="ui-search-float relative flex h-9 min-w-[240px] max-w-md flex-1 items-center">
            <Search
              className="pointer-events-none absolute left-3.5 h-[15px] w-[15px] text-[var(--text-tertiary)]"
              strokeWidth={1.5}
            />
            <input
              ref={searchRef}
              type="text"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={dict.portable.searchPlaceholder}
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

          <div className="flex items-center gap-0.5">
            <div className="relative">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  setShortcutMenuId(null);
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
                <div className="ui-popover absolute right-0 z-30 mt-1.5 w-32 p-1.5">
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
            >
              <ArrowDownUp
                className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <span>{sortDesc ? dict.toolbar.sortDesc : dict.toolbar.sortAsc}</span>
            </button>
            <span className="num-tabular ml-2 text-[12px] text-[var(--text-tertiary)]">
              {visible.length}
            </span>
          </div>
        </div>
      </div>

      {/* Borderless Table Header */}
      <div className="grid grid-cols-[minmax(200px,1fr)_104px_96px_108px_156px] items-center gap-3 border-b border-[var(--border-hairline)] py-2 pr-[30px] pl-6 text-[11.5px] font-medium text-[var(--text-tertiary)] select-none">
        <div>{dict.portable.colName}</div>
        <div>{dict.portable.colVersion}</div>
        <div>{dict.portable.colModified}</div>
        <div className="text-right">{dict.portable.colSize}</div>
        <div className="text-right">{dict.appList.colActions}</div>
      </div>

      {/* Portable List */}
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-1.5 [scrollbar-gutter:stable]">
        {loading && apps.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-[var(--text-secondary)]">
            <RefreshCw
              className="h-5 w-5 animate-spin text-[var(--text-tertiary)]"
              strokeWidth={1.5}
            />
          </div>
        ) : visible.length === 0 ? (
          <div className="flex flex-col items-center justify-center py-16 text-center">
            <Package
              className="h-8 w-8 text-[var(--text-tertiary)] opacity-50"
              strokeWidth={1.5}
            />
            <div className="mt-2.5 text-[13.5px] font-medium text-[var(--text-primary)]">
              {dict.portable.emptyScanTitle}
            </div>
            <div className="mt-1 text-[12px] text-[var(--text-secondary)]">
              {dict.portable.emptyScanDesc}
            </div>
          </div>
        ) : (
          <div className="space-y-0.5">
            {visible.map((app) => {
              const iconSrc = icons[app.id];
              const sizePct =
                app.sizeBytes > 0
                  ? Math.min(
                      100,
                      Math.round(
                        (Math.log10(app.sizeBytes + 1024) / Math.log10(maxSizeBytes + 1024)) * 100,
                      ),
                    )
                  : 0;
              const menuOpen = shortcutMenuId === app.id;

              return (
                <div
                  key={app.id}
                  onDoubleClick={() => void handleLaunch(app)}
                  className="group grid h-[60px] grid-cols-[minmax(200px,1fr)_104px_96px_108px_156px] items-center gap-3 rounded-[10px] px-3 transition-colors duration-150 hover:bg-[var(--bg-hover)]"
                >
                  <div className="flex min-w-0 items-center gap-3">
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
                        {app.isSingleFile && (
                          <span className="shrink-0 rounded-full bg-[var(--bg-subtle)] px-1.5 py-0.5 text-[10px] text-[var(--text-tertiary)]">
                            {dict.portable.singleFileBadge}
                          </span>
                        )}
                      </div>
                      <div
                        title={app.exePath}
                        className="mt-0.5 truncate text-[11.5px] text-[var(--text-secondary)]"
                      >
                        {app.publisher ? `${app.publisher} | ${app.installDir}` : app.installDir}
                      </div>
                    </div>
                  </div>

                  <div
                    className={`num-tabular truncate text-[12px] ${
                      app.version ? "text-[var(--text-secondary)]" : "text-[var(--text-tertiary)]"
                    }`}
                  >
                    {app.version || "-"}
                  </div>

                  <div className="num-tabular truncate text-[12px] text-[var(--text-secondary)]">
                    {app.lastModified || "-"}
                  </div>

                  <div className="flex flex-col items-end justify-center">
                    <span className="num-tabular text-[12.5px] font-medium text-[var(--text-primary)]">
                      {formatBytes(app.sizeBytes)}
                    </span>
                    {app.sizeBytes > 0 && (
                      <div className="mt-1.5 h-[3px] w-16 overflow-hidden rounded-full bg-[var(--bg-track)]">
                        <div
                          className="h-full rounded-full bg-[var(--bar-fill)]"
                          style={{ width: `${Math.max(6, sizePct)}%` }}
                        />
                      </div>
                    )}
                  </div>

                  <div
                    className={`flex items-center justify-end gap-0.5 transition-opacity duration-150 ${
                      menuOpen
                        ? "opacity-100"
                        : "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
                    }`}
                  >
                    <button
                      type="button"
                      onClick={() => void handleLaunch(app)}
                      title={dict.portable.launch}
                      aria-label={dict.portable.launch}
                      className="ui-btn-ghost h-7 w-7 p-0"
                    >
                      <Play
                        className="h-3.5 w-3.5 text-[var(--text-secondary)]"
                        strokeWidth={1.5}
                      />
                    </button>

                    <div className="relative">
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          setSortOpen(false);
                          setShortcutMenuId(menuOpen ? null : app.id);
                        }}
                        title={dict.portable.createShortcut}
                        aria-label={dict.portable.createShortcut}
                        className="ui-btn-ghost h-7 w-7 p-0"
                      >
                        <Link2
                          className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                          strokeWidth={1.5}
                        />
                      </button>
                      {menuOpen && (
                        <div className="ui-popover absolute right-0 z-30 mt-1 w-40 p-1">
                          <button
                            type="button"
                            onClick={() => void handleCreateShortcut(app, "desktop")}
                            className="flex w-full items-center rounded-[7px] px-2.5 py-1.5 text-left text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] cursor-pointer"
                          >
                            {dict.portable.shortcutDesktop}
                          </button>
                          <button
                            type="button"
                            onClick={() => void handleCreateShortcut(app, "startMenu")}
                            className="flex w-full items-center rounded-[7px] px-2.5 py-1.5 text-left text-[12px] text-[var(--text-primary)] hover:bg-[var(--bg-hover)] cursor-pointer"
                          >
                            {dict.portable.shortcutStartMenu}
                          </button>
                        </div>
                      )}
                    </div>

                    <button
                      type="button"
                      onClick={() =>
                        api
                          .openPathInExplorer(app.isSingleFile ? app.exePath : app.installDir)
                          .catch((e) => onNotify("error", app.name, String(e)))
                      }
                      title={dict.common.openDir}
                      aria-label={dict.common.openDir}
                      className="ui-btn-ghost h-7 w-7 p-0"
                    >
                      <FolderOpen
                        className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                        strokeWidth={1.5}
                      />
                    </button>

                    <button
                      type="button"
                      onClick={() =>
                        void onDeepRemove(app.isSingleFile ? app.exePath : app.installDir)
                      }
                      title={dict.portable.deepRemove}
                      aria-label={dict.portable.deepRemove}
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
