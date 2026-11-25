import { useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Copy,
  Database,
  FileText,
  Folder,
  FolderOpen,
  KeyRound,
  Link2,
  Loader2,
  Lock,
  Search,
  ShieldCheck,
  Skull,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type {
  AppSettings,
  CleanupOptions,
  CleanupReport,
  InstalledApp,
  LeftoverItem,
  LeftoverKind,
  LockingProcessInfo,
  RunningProcessInfo,
} from "../types";
import { formatBytes } from "../utils/format";
import { formatLeftoverReason } from "../utils/leftoverReason";

type WizardStage = "prepare" | "uninstalling" | "scanning" | "review" | "cleaning" | "done";

interface UninstallWizardModalProps {
  dict: Dict;
  app: InstalledApp;
  initialQuiet: boolean;
  skipNative?: boolean;
  preloadedLeftovers?: LeftoverItem[];
  settings: AppSettings;
  onClose: (didModify: boolean) => void;
  onStatusRefresh: () => void;
  onNotify?: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function UninstallWizardModal({
  dict,
  app,
  initialQuiet,
  skipNative = false,
  preloadedLeftovers,
  settings,
  onClose,
  onStatusRefresh,
  onNotify,
}: UninstallWizardModalProps) {
  const [stage, setStage] = useState<WizardStage>(
    preloadedLeftovers ? "review" : skipNative ? "scanning" : "prepare",
  );
  const [quiet, setQuiet] = useState(initialQuiet);
  const [options, setOptions] = useState<CleanupOptions>({
    useRecycleBin: settings.useRecycleBin,
    backupRegistry: settings.autoBackupRegistry,
    createRestorePoint: settings.createRestorePoint,
    scheduleRebootOnLock: true,
    terminateLockingProcesses: false,
  });

  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [vendorMsg, setVendorMsg] = useState<string>("");
  const [runningProcs, setRunningProcs] = useState<RunningProcessInfo[]>([]);
  const [closingRunning, setClosingRunning] = useState(false);

  const [leftovers, setLeftovers] = useState<LeftoverItem[]>(preloadedLeftovers ?? []);
  const [leftoverQuery, setLeftoverQuery] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() =>
    new Set((preloadedLeftovers ?? []).filter((i) => i.selectedByDefault).map((i) => i.id)),
  );
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const [locks, setLocks] = useState<LockingProcessInfo[]>([]);
  const [checkingLocks, setCheckingLocks] = useState(false);
  const [report, setReport] = useState<CleanupReport | null>(null);
  const [didModify, setDidModify] = useState(false);

  const isBusy = stage === "uninstalling" || stage === "scanning" || stage === "cleaning";

  // Escape closes wizard when idle
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isBusy) {
        e.preventDefault();
        onClose(didModify);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [isBusy, didModify, onClose]);

  // Pre-uninstall check: detect if target application has active running processes in its folder
  useEffect(() => {
    if (stage === "prepare" && app.installLocation) {
      let active = true;
      void api
        .findRunningAppProcesses(app.installLocation)
        .then((procs) => {
          if (active) setRunningProcs(procs);
        })
        .catch(() => {});
      return () => {
        active = false;
      };
    }
  }, [stage, app.installLocation]);

  useEffect(() => {
    if (skipNative && !preloadedLeftovers) {
      void runScan();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleCloseRunning = async () => {
    if (runningProcs.length === 0) return;
    setClosingRunning(true);
    setErrorMsg(null);
    try {
      await api.terminateLockingProcesses(runningProcs.map((p) => p.pid));
      const remaining = await api.findRunningAppProcesses(app.installLocation);
      setRunningProcs(remaining);
      if (remaining.length === 0 && onNotify) {
        onNotify("success", app.name, dict.wizard.runningClosed);
      }
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setClosingRunning(false);
    }
  };

  const runScan = async () => {
    setStage("scanning");
    setErrorMsg(null);
    try {
      const found = await api.scanAppLeftovers(app);
      setLeftovers(found);
      setSelectedIds(new Set(found.filter((i) => i.selectedByDefault).map((i) => i.id)));
      setStage("review");
    } catch (err) {
      setErrorMsg(String(err));
      setStage("review");
    }
  };

  const handleStartUninstall = async () => {
    setStage("uninstalling");
    setErrorMsg(null);
    try {
      const res = await api.runUninstall(app, quiet);
      setVendorMsg(res.message);
      setDidModify(true);
      onStatusRefresh();
      await runScan();
    } catch (err) {
      setErrorMsg(String(err));
      setStage("prepare");
    }
  };

  const handleCheckLocks = async () => {
    const fsPaths = leftovers
      .filter(
        (i) =>
          selectedIds.has(i.id) &&
          (i.kind === "directory" || i.kind === "file" || i.kind === "shortcut"),
      )
      .map((i) => i.path);
    if (fsPaths.length === 0) return;
    setCheckingLocks(true);
    try {
      const found = await api.checkFileLocks(fsPaths);
      setLocks(found);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setCheckingLocks(false);
    }
  };

  const handleKillLocks = async () => {
    if (locks.length === 0) return;
    try {
      await api.terminateLockingProcesses(locks.map((l) => l.pid));
      setLocks([]);
    } catch (err) {
      setErrorMsg(String(err));
    }
  };

  const handleClean = async () => {
    const chosen = leftovers.filter((i) => selectedIds.has(i.id));
    if (chosen.length === 0) {
      onClose(didModify);
      return;
    }
    setStage("cleaning");
    setErrorMsg(null);
    try {
      const res = await api.cleanLeftovers(app.name, chosen, options);
      setReport(res);
      setDidModify(true);
      onStatusRefresh();
      setStage("done");
    } catch (err) {
      setErrorMsg(String(err));
      setStage("review");
    }
  };

  const filteredLeftovers = useMemo(() => {
    const q = leftoverQuery.trim().toLowerCase();
    if (!q) return leftovers;
    return leftovers.filter((item) => {
      const localizedReason = formatLeftoverReason(item, dict);
      const hay = `${item.path} ${item.valueName ?? ""} ${localizedReason}`.toLowerCase();
      return hay.includes(q);
    });
  }, [leftovers, leftoverQuery, dict]);

  const grouped = useMemo(() => {
    const map: Record<string, LeftoverItem[]> = {
      registry: [],
      filesystem: [],
      shortcuts: [],
      services: [],
    };
    for (const item of filteredLeftovers) {
      if (item.kind === "registryKey" || item.kind === "registryValue") {
        map.registry.push(item);
      } else if (item.kind === "shortcut") {
        map.shortcuts.push(item);
      } else if (item.kind === "service" || item.kind === "scheduledTask") {
        map.services.push(item);
      } else {
        map.filesystem.push(item);
      }
    }
    return map;
  }, [filteredLeftovers]);

  const selectedBytes = useMemo(
    () =>
      leftovers
        .filter((i) => selectedIds.has(i.id))
        .reduce((acc, i) => acc + i.sizeBytes, 0),
    [leftovers, selectedIds],
  );

  const toggleItem = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleGroupSelection = (items: LeftoverItem[]) => {
    const allChecked = items.every((i) => selectedIds.has(i.id));
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (allChecked) {
        items.forEach((i) => next.delete(i.id));
      } else {
        items.forEach((i) => next.add(i.id));
      }
      return next;
    });
  };

  const toggleGroup = (key: string) => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleCopyPath = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      onNotify?.("success", dict.common.copied, text);
    } catch {
      // Ignore
    }
  };

  const stepIndex =
    stage === "prepare"
      ? 0
      : stage === "uninstalling"
        ? 1
        : stage === "scanning" || stage === "review"
          ? 2
          : 3;

  const steps = [
    dict.wizard.stepPrepare,
    dict.wizard.stepUninstall,
    dict.wizard.stepScan,
    dict.wizard.stepFinish,
  ];

  const groupMeta: Array<[string, string, typeof Database]> = [
    ["registry", dict.wizard.groupRegistry, Database],
    ["filesystem", dict.wizard.groupFilesystem, Folder],
    ["shortcuts", dict.wizard.groupShortcuts, Link2],
    ["services", dict.wizard.groupServices, Wrench],
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--backdrop-scrim)] p-4 backdrop-blur-[2px]">
      <div className="ui-modal flex max-h-[86vh] w-full max-w-[780px] flex-col overflow-hidden">
        <div className="flex items-start justify-between border-b border-[var(--border-hairline)] px-6 py-4">
          <div>
            <h2 className="text-[15px] font-medium text-[var(--text-primary)]">
              {dict.wizard.title} - {app.name}
            </h2>
            <div className="mt-2.5 flex flex-wrap items-center gap-2">
              {steps.map((label, idx) => (
                <div key={label} className="flex items-center gap-1.5 text-[11.5px]">
                  <span
                    className={`num-tabular flex h-5 w-5 items-center justify-center rounded-full text-[10.5px] font-medium ${
                      idx < stepIndex
                        ? "bg-[var(--bg-selected)] text-[var(--text-primary)]"
                        : idx === stepIndex
                          ? "bg-[var(--accent-primary)] text-[var(--accent-primary-fg)]"
                          : "bg-[var(--bg-hover)] text-[var(--text-tertiary)]"
                    }`}
                  >
                    {idx < stepIndex ? <Check className="h-3 w-3" strokeWidth={1.75} /> : idx + 1}
                  </span>
                  <span
                    className={
                      idx === stepIndex
                        ? "font-medium text-[var(--text-primary)]"
                        : "text-[var(--text-tertiary)]"
                    }
                  >
                    {label}
                  </span>
                  {idx < steps.length - 1 && (
                    <span className="mx-1 h-px w-5 bg-[var(--border-hairline)]" />
                  )}
                </div>
              ))}
            </div>
          </div>
          <button
            type="button"
            disabled={isBusy}
            onClick={() => onClose(didModify)}
            aria-label={dict.common.close}
            className="ui-btn-ghost h-7 w-7 rounded-full p-0"
          >
            <X className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
          </button>
        </div>

        <div className="min-h-[260px] flex-1 overflow-y-auto px-6 py-5">
          {errorMsg && (
            <div className="mb-4 flex items-start gap-2.5 rounded-[12px] border border-[var(--danger-border)] bg-[var(--danger-bg-hover)] p-3.5 text-[12.5px] text-[var(--danger-text)]">
              <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" strokeWidth={1.5} />
              <div className="break-words">{errorMsg}</div>
            </div>
          )}

          {stage === "prepare" && (
            <div className="space-y-4">
              <p className="text-[13px] text-[var(--text-secondary)]">{dict.wizard.confirmDesc}</p>

              {/* Pre-uninstall running process warning */}
              {runningProcs.length > 0 && (
                <div className="rounded-[12px] border border-[var(--warning-border)] bg-[var(--warning-bg)] p-3.5">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <span className="text-[12.5px] font-medium text-[var(--warning-text)]">
                      {dict.wizard.runningDetected}
                    </span>
                    <button
                      type="button"
                      disabled={closingRunning}
                      onClick={handleCloseRunning}
                      className="ui-btn-danger-subtle h-7 px-2.5 text-[11.5px]"
                    >
                      {closingRunning ? (
                        <Loader2 className="h-3 w-3 animate-spin" strokeWidth={1.5} />
                      ) : (
                        <Skull className="h-3 w-3" strokeWidth={1.5} />
                      )}
                      {dict.wizard.closeRunning}
                    </button>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {runningProcs.map((p) => (
                      <span
                        key={p.pid}
                        className="num-tabular rounded-[6px] border border-[var(--border-hairline)] bg-[var(--bg-surface)] px-2 py-0.5 font-mono text-[11px] text-[var(--text-primary)]"
                      >
                        {p.name} (PID {p.pid})
                      </span>
                    ))}
                  </div>
                </div>
              )}

              <div className="space-y-1 rounded-[12px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-3.5 text-[12px]">
                <div>
                  <span className="text-[var(--text-tertiary)]">{dict.inspector.publisher}: </span>
                  <span className="font-medium text-[var(--text-primary)]">
                    {app.publisher || dict.inspector.unknown}
                  </span>
                  {app.version && (
                    <span className="num-tabular ml-2 text-[var(--text-secondary)]">
                      v{app.version}
                    </span>
                  )}
                </div>
                {app.installLocation && (
                  <div className="truncate font-mono text-[11px] text-[var(--text-secondary)]">
                    {app.installLocation}
                  </div>
                )}
              </div>
              <div className="space-y-2.5">
                {app.quietUninstallString && (
                  <CheckOption checked={quiet} onChange={setQuiet} label={dict.wizard.quietMode} />
                )}
                <CheckOption
                  checked={options.useRecycleBin}
                  onChange={(v) => setOptions((o) => ({ ...o, useRecycleBin: v }))}
                  label={dict.wizard.useRecycleBin}
                />
                <CheckOption
                  checked={options.backupRegistry}
                  onChange={(v) => setOptions((o) => ({ ...o, backupRegistry: v }))}
                  label={dict.wizard.backupRegistry}
                />
                <CheckOption
                  checked={options.scheduleRebootOnLock}
                  onChange={(v) => setOptions((o) => ({ ...o, scheduleRebootOnLock: v }))}
                  label={dict.wizard.rebootOnLock}
                />
                <CheckOption
                  checked={options.createRestorePoint}
                  onChange={(v) => setOptions((o) => ({ ...o, createRestorePoint: v }))}
                  label={dict.wizard.createRestorePoint}
                />
              </div>
            </div>
          )}

          {stage === "uninstalling" && (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <Loader2
                className="h-8 w-8 animate-spin text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <div className="mt-4 text-[14px] font-medium text-[var(--text-primary)]">
                {dict.wizard.runningVendor}
              </div>
              <p className="mt-1.5 max-w-md text-[12.5px] text-[var(--text-secondary)]">
                {dict.wizard.waitingTree}
              </p>
            </div>
          )}

          {stage === "scanning" && (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <Loader2
                className="h-8 w-8 animate-spin text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <div className="mt-4 text-[14px] font-medium text-[var(--text-primary)]">
                {dict.wizard.scanningLeftovers}
              </div>
              {vendorMsg && (
                <p className="mt-1.5 text-[12px] text-[var(--text-secondary)]">{vendorMsg}</p>
              )}
            </div>
          )}

          {stage === "review" && (
            <div className="space-y-4">
              {leftovers.length === 0 ? (
                <div className="flex flex-col items-center justify-center py-10 text-center">
                  <ShieldCheck
                    className="h-10 w-10 text-[var(--success-text)]"
                    strokeWidth={1.5}
                  />
                  <div className="mt-3 text-[15px] font-medium text-[var(--text-primary)]">
                    {dict.wizard.noLeftoversTitle}
                  </div>
                  <p className="mt-1 max-w-md text-[12.5px] text-[var(--text-secondary)]">
                    {dict.wizard.noLeftoversDesc}
                  </p>
                </div>
              ) : (
                <>
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="num-tabular text-[13px] font-medium text-[var(--text-primary)]">
                      {leftovers.length} {dict.wizard.leftoversFound}
                      {selectedBytes > 0 && (
                        <span className="ml-2 font-normal text-[var(--text-secondary)]">
                          ({formatBytes(selectedBytes)})
                        </span>
                      )}
                    </div>
                    <div className="flex flex-wrap items-center gap-1">
                      <button
                        type="button"
                        onClick={() =>
                          setSelectedIds(
                            new Set(
                              leftovers.filter((i) => i.confidence === "high").map((i) => i.id),
                            ),
                          )
                        }
                        className="ui-btn-ghost h-7 px-2.5 text-[11.5px]"
                      >
                        {dict.wizard.selectSafe}
                      </button>
                      <button
                        type="button"
                        onClick={() => setSelectedIds(new Set(leftovers.map((i) => i.id)))}
                        className="ui-btn-ghost h-7 px-2.5 text-[11.5px]"
                      >
                        {dict.wizard.selectAll}
                      </button>
                      <button
                        type="button"
                        onClick={() => setSelectedIds(new Set())}
                        className="ui-btn-ghost h-7 px-2.5 text-[11.5px]"
                      >
                        {dict.wizard.deselectAll}
                      </button>
                      <button
                        type="button"
                        disabled={checkingLocks}
                        onClick={handleCheckLocks}
                        className="ui-btn-ghost h-7 px-2.5 text-[11.5px]"
                      >
                        <Lock className="h-3 w-3 text-[var(--text-tertiary)]" strokeWidth={1.5} />
                        {dict.wizard.checkLocks}
                      </button>
                    </div>
                  </div>

                  {/* Quick filter input inside leftover review */}
                  {leftovers.length > 4 && (
                    <div className="relative flex h-8 items-center">
                      <Search
                        className="pointer-events-none absolute left-3 h-3.5 w-3.5 text-[var(--text-tertiary)]"
                        strokeWidth={1.5}
                      />
                      <input
                        type="text"
                        value={leftoverQuery}
                        onChange={(e) => setLeftoverQuery(e.target.value)}
                        placeholder={dict.wizard.searchLeftoversPlaceholder}
                        className="ui-input h-8 w-full pr-7 pl-8.5 text-[12px]"
                      />
                      {leftoverQuery && (
                        <button
                          type="button"
                          onClick={() => setLeftoverQuery("")}
                          className="absolute right-2 flex h-4.5 w-4.5 items-center justify-center rounded-full text-[var(--text-tertiary)] hover:bg-[var(--bg-hover)] cursor-pointer"
                        >
                          <X className="h-3 w-3" strokeWidth={1.5} />
                        </button>
                      )}
                    </div>
                  )}

                  {locks.length > 0 && (
                    <div className="rounded-[12px] border border-[var(--warning-border)] bg-[var(--warning-bg)] p-3.5">
                      <div className="flex items-center justify-between">
                        <span className="text-[12.5px] font-medium text-[var(--warning-text)]">
                          {dict.wizard.locksDetected}
                        </span>
                        <button
                          type="button"
                          onClick={handleKillLocks}
                          className="ui-btn-danger-subtle h-7 px-2.5 text-[11.5px]"
                        >
                          <Skull className="h-3 w-3" strokeWidth={1.5} />
                          {dict.wizard.killAndRetry}
                        </button>
                      </div>
                      <div className="mt-2 flex flex-wrap gap-1.5">
                        {locks.map((l) => (
                          <span
                            key={l.pid}
                            className="num-tabular rounded-[6px] border border-[var(--border-hairline)] bg-[var(--bg-surface)] px-2 py-0.5 font-mono text-[11px] text-[var(--text-primary)]"
                          >
                            {l.name} (PID {l.pid})
                          </span>
                        ))}
                      </div>
                    </div>
                  )}

                  {skipNative && (
                    <div className="flex flex-wrap gap-4 rounded-[12px] bg-[var(--bg-subtle)] px-3.5 py-2.5 text-[12px]">
                      <CheckOption
                        checked={options.useRecycleBin}
                        onChange={(v) => setOptions((o) => ({ ...o, useRecycleBin: v }))}
                        label={dict.wizard.useRecycleBin}
                      />
                      <CheckOption
                        checked={options.backupRegistry}
                        onChange={(v) => setOptions((o) => ({ ...o, backupRegistry: v }))}
                        label={dict.wizard.backupRegistry}
                      />
                    </div>
                  )}

                  <div className="space-y-2.5">
                    {groupMeta.map(([groupKey, groupTitle, GroupIcon]) => {
                      const items = grouped[groupKey];
                      if (items.length === 0) return null;
                      const collapsed = collapsedGroups.has(groupKey);
                      const checkedInGroup = items.filter((i) => selectedIds.has(i.id)).length;
                      const allChecked = checkedInGroup === items.length;
                      const partialChecked = checkedInGroup > 0 && checkedInGroup < items.length;
                      const groupBytes = items.reduce((acc, i) => acc + i.sizeBytes, 0);

                      return (
                        <div
                          key={groupKey}
                          className="overflow-hidden rounded-[12px] border border-[var(--border-hairline)]"
                        >
                          <div className="flex items-center justify-between bg-[var(--bg-subtle)] px-3.5 py-2">
                            <div className="flex items-center gap-2">
                              <GroupCheckbox
                                checked={allChecked}
                                indeterminate={partialChecked}
                                onChange={() => toggleGroupSelection(items)}
                              />
                              <button
                                type="button"
                                onClick={() => toggleGroup(groupKey)}
                                className="flex items-center gap-1.5 text-left text-[12px] font-medium text-[var(--text-primary)] cursor-pointer"
                              >
                                {collapsed ? (
                                  <ChevronRight
                                    className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                    strokeWidth={1.5}
                                  />
                                ) : (
                                  <ChevronDown
                                    className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                    strokeWidth={1.5}
                                  />
                                )}
                                <GroupIcon
                                  className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                  strokeWidth={1.5}
                                />
                                <span>{groupTitle}</span>
                                <span className="num-tabular text-[11.5px] font-normal text-[var(--text-tertiary)]">
                                  {checkedInGroup}/{items.length}
                                </span>
                              </button>
                            </div>
                            {groupBytes > 0 && (
                              <span className="num-tabular text-[11.5px] text-[var(--text-tertiary)]">
                                {formatBytes(groupBytes)}
                              </span>
                            )}
                          </div>

                          {!collapsed && (
                            <div className="divide-y divide-[var(--border-hairline)]">
                              {items.map((item) => {
                                const isFs =
                                  item.kind === "directory" ||
                                  item.kind === "file" ||
                                  item.kind === "shortcut";
                                const isReg =
                                  item.kind === "registryKey" ||
                                  item.kind === "registryValue" ||
                                  item.kind === "service";

                                return (
                                  <div
                                    key={item.id}
                                    onClick={() => toggleItem(item.id)}
                                    className="group flex items-start gap-2.5 px-3.5 py-2.5 transition-colors hover:bg-[var(--bg-hover)] cursor-pointer"
                                  >
                                    <input
                                      type="checkbox"
                                      checked={selectedIds.has(item.id)}
                                      onChange={() => {}}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        toggleItem(item.id);
                                      }}
                                      className="ui-checkbox mt-0.5"
                                    />
                                    <div className="min-w-0 flex-1">
                                      <div className="flex items-center gap-1.5">
                                        <KindIcon kind={item.kind} />
                                        <span className="truncate font-mono text-[11.5px] text-[var(--text-primary)]">
                                          {item.valueName
                                            ? `${item.path}  ->  [${item.valueName}]`
                                            : item.path}
                                        </span>
                                      </div>
                                      <div className="mt-1 flex items-center gap-2 text-[11px] text-[var(--text-secondary)]">
                                        <span
                                          className={`rounded-full px-1.5 py-0.5 text-[10px] font-medium ${
                                            item.confidence === "high"
                                              ? "bg-[var(--bg-subtle)] text-[var(--text-secondary)]"
                                              : "bg-[var(--warning-bg)] text-[var(--warning-text)]"
                                          }`}
                                        >
                                          {item.confidence === "high"
                                            ? dict.wizard.confHigh
                                            : dict.wizard.confMedium}
                                        </span>
                                        <span className="truncate">
                                          {formatLeftoverReason(item, dict)}
                                        </span>
                                      </div>
                                    </div>

                                    <div
                                      className="flex shrink-0 items-center gap-1"
                                      onClick={(e) => e.stopPropagation()}
                                    >
                                      {item.sizeBytes > 0 && (
                                        <span className="num-tabular mr-1 text-[11.5px] font-medium text-[var(--text-secondary)]">
                                          {formatBytes(item.sizeBytes)}
                                        </span>
                                      )}
                                      <div className="flex items-center gap-0.5 opacity-0 transition-opacity duration-150 group-hover:opacity-100">
                                        {isFs && (
                                          <button
                                            type="button"
                                            title={dict.common.openDir}
                                            aria-label={dict.common.openDir}
                                            onClick={() =>
                                              api
                                                .openPathInExplorer(item.path)
                                                .catch((e) =>
                                                  onNotify?.(
                                                    "error",
                                                    dict.common.openDir,
                                                    String(e),
                                                  ),
                                                )
                                            }
                                            className="ui-btn-ghost h-6 w-6 p-0"
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
                                            title={dict.appList.openRegedit}
                                            aria-label={dict.appList.openRegedit}
                                            onClick={() =>
                                              api
                                                .openKeyInRegedit(item.path)
                                                .catch((e) =>
                                                  onNotify?.(
                                                    "error",
                                                    dict.appList.openRegedit,
                                                    String(e),
                                                  ),
                                                )
                                            }
                                            className="ui-btn-ghost h-6 w-6 p-0"
                                          >
                                            <KeyRound
                                              className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                              strokeWidth={1.5}
                                            />
                                          </button>
                                        )}
                                        <button
                                          type="button"
                                          title={dict.common.copy}
                                          aria-label={dict.common.copy}
                                          onClick={() =>
                                            handleCopyPath(
                                              item.valueName
                                                ? `${item.path}\\${item.valueName}`
                                                : item.path,
                                            )
                                          }
                                          className="ui-btn-ghost h-6 w-6 p-0"
                                        >
                                          <Copy
                                            className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                            strokeWidth={1.5}
                                          />
                                        </button>
                                      </div>
                                    </div>
                                  </div>
                                );
                              })}
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                </>
              )}
            </div>
          )}

          {stage === "cleaning" && (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <Loader2
                className="h-8 w-8 animate-spin text-[var(--text-tertiary)]"
                strokeWidth={1.5}
              />
              <div className="mt-4 text-[14px] font-medium text-[var(--text-primary)]">
                {dict.wizard.cleaning}
              </div>
            </div>
          )}

          {stage === "done" && report && (
            <div className="space-y-4">
              <div className="flex items-center gap-3">
                <CheckCircle2
                  className="h-7 w-7 text-[var(--success-text)]"
                  strokeWidth={1.5}
                />
                <div>
                  <div className="text-[15px] font-medium text-[var(--text-primary)]">
                    {dict.wizard.doneTitle}
                  </div>
                  {report.regBackupPath && (
                    <div className="mt-0.5 text-[11.5px] text-[var(--text-secondary)]">
                      {dict.wizard.regBackupSaved}{" "}
                      <span className="font-mono text-[var(--text-primary)]">
                        {report.regBackupPath}
                      </span>
                    </div>
                  )}
                </div>
              </div>

              <div className="grid grid-cols-4 gap-2.5">
                <StatBox
                  label={dict.wizard.succeeded}
                  value={String(report.succeeded)}
                  tone="neutral"
                />
                <StatBox
                  label={dict.wizard.failed}
                  value={String(report.failed)}
                  tone={report.failed > 0 ? "danger" : "neutral"}
                />
                <StatBox
                  label={dict.wizard.rebootScheduled}
                  value={String(report.scheduledForReboot)}
                  tone={report.scheduledForReboot > 0 ? "warning" : "neutral"}
                />
                <StatBox
                  label={dict.wizard.freed}
                  value={formatBytes(report.freedBytes)}
                  tone="neutral"
                />
              </div>

              {report.details.some((d) => !d.success) && (
                <div className="max-h-40 space-y-1 overflow-y-auto rounded-[12px] border border-[var(--danger-border)] bg-[var(--danger-bg-hover)] p-3 text-[11.5px]">
                  {report.details
                    .filter((d) => !d.success)
                    .map((d) => (
                      <div key={d.itemId} className="font-mono text-[var(--danger-text)]">
                        {d.path}: {d.error}
                      </div>
                    ))}
                </div>
              )}

              {report.regBackupPath && (
                <button
                  type="button"
                  onClick={() => api.openBackupsFolder()}
                  className="ui-btn-secondary h-8"
                >
                  <FolderOpen
                    className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                    strokeWidth={1.5}
                  />
                  {dict.wizard.openBackups}
                </button>
              )}
            </div>
          )}
        </div>

        <div className="flex items-center justify-between border-t border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-6 py-3.5">
          <div>
            {stage === "prepare" && app.kind !== "storeApp" && (
              <button type="button" onClick={runScan} className="ui-btn-ghost h-8 text-[12px]">
                {dict.wizard.skipToScan}
              </button>
            )}
          </div>
          <div className="flex items-center gap-2">
            {stage === "prepare" && (
              <>
                <button
                  type="button"
                  onClick={() => onClose(didModify)}
                  className="ui-btn-ghost h-8"
                >
                  {dict.wizard.cancel}
                </button>
                <button
                  type="button"
                  onClick={handleStartUninstall}
                  className="ui-btn-danger-subtle h-8"
                >
                  <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                  {dict.wizard.startUninstall}
                </button>
              </>
            )}

            {stage === "review" && (
              <>
                <button
                  type="button"
                  onClick={() => onClose(didModify)}
                  className="ui-btn-ghost h-8"
                >
                  {leftovers.length === 0 ? dict.wizard.close : dict.wizard.finishWithoutClean}
                </button>
                {leftovers.length > 0 && (
                  <button
                    type="button"
                    disabled={selectedIds.size === 0}
                    onClick={handleClean}
                    className="ui-btn-primary h-8"
                  >
                    <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                    <span className="num-tabular">
                      {dict.wizard.cleanSelected} ({selectedIds.size})
                    </span>
                  </button>
                )}
              </>
            )}

            {stage === "done" && (
              <button
                type="button"
                onClick={() => onClose(true)}
                className="ui-btn-primary h-8"
              >
                {dict.wizard.close}
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function GroupCheckbox({
  checked,
  indeterminate,
  onChange,
}: {
  checked: boolean;
  indeterminate: boolean;
  onChange: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) {
      ref.current.indeterminate = indeterminate;
    }
  }, [indeterminate]);
  return (
    <input
      ref={ref}
      type="checkbox"
      checked={checked}
      onChange={onChange}
      className="ui-checkbox"
    />
  );
}

function CheckOption({
  checked,
  onChange,
  label,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
}) {
  return (
    <label className="flex items-center gap-2.5 text-[12.5px] text-[var(--text-primary)] cursor-pointer">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="ui-checkbox"
      />
      <span>{label}</span>
    </label>
  );
}

function KindIcon({ kind }: { kind: LeftoverKind }) {
  switch (kind) {
    case "registryKey":
    case "registryValue":
      return (
        <Database
          className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
          strokeWidth={1.5}
        />
      );
    case "directory":
      return (
        <Folder
          className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
          strokeWidth={1.5}
        />
      );
    case "shortcut":
      return (
        <Link2
          className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
          strokeWidth={1.5}
        />
      );
    case "service":
    case "scheduledTask":
      return (
        <Wrench
          className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
          strokeWidth={1.5}
        />
      );
    default:
      return (
        <FileText
          className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
          strokeWidth={1.5}
        />
      );
  }
}

function StatBox({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone: "danger" | "warning" | "neutral";
}) {
  const valueClass =
    tone === "danger"
      ? "text-[var(--danger-text)]"
      : tone === "warning"
        ? "text-[var(--warning-text)]"
        : "text-[var(--text-primary)]";
  return (
    <div className="rounded-[12px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] p-3">
      <div className="text-[11px] text-[var(--text-tertiary)]">{label}</div>
      <div className={`num-tabular mt-1 text-[16px] font-medium ${valueClass}`}>{value}</div>
    </div>
  );
}
