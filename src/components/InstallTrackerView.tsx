import { useState } from "react";
import {
  Activity,
  ChevronDown,
  ChevronRight,
  Database,
  FileSearch,
  Folder,
  Loader2,
  Play,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { api } from "../api/client";
import type { Dict } from "../i18n";
import type { ActiveTrackSession, InstallTrace, InstalledApp, LeftoverItem } from "../types";
import { formatBytes, formatDateTime } from "../utils/format";

interface InstallTrackerViewProps {
  dict: Dict;
  activeSessions: ActiveTrackSession[];
  traces: InstallTrace[];
  onRefresh: () => void;
  onTrackedUninstall: (syntheticApp: InstalledApp, leftovers: LeftoverItem[]) => void;
  onNotify: (kind: "info" | "success" | "warning" | "error", title: string, detail?: string) => void;
}

export function InstallTrackerView({
  dict,
  activeSessions,
  traces,
  onRefresh,
  onTrackedUninstall,
  onNotify,
}: InstallTrackerViewProps) {
  const [appName, setAppName] = useState("");
  const [installerPath, setInstallerPath] = useState("");
  const [launchInstaller, setLaunchInstaller] = useState(true);
  const [starting, setStarting] = useState(false);
  const [finishingId, setFinishingId] = useState<string | null>(null);
  const [expandedTraceId, setExpandedTraceId] = useState<string | null>(null);

  const handleBrowse = async () => {
    const picked = await api.pickFile([
      { name: "Installers", extensions: ["exe", "msi", "msix", "bat"] },
    ]);
    if (picked) {
      setInstallerPath(picked);
      if (!appName.trim()) {
        const base = picked.split(/[\\/]/).pop()?.replace(/\.(exe|msi|msix|bat)$/i, "") ?? "";
        setAppName(base);
      }
    }
  };

  const handleStart = async () => {
    if (!appName.trim()) return;
    setStarting(true);
    try {
      await api.startInstallTrack(appName.trim(), installerPath.trim(), launchInstaller);
      onNotify("success", dict.tracker.activeTitle, appName.trim());
      setAppName("");
      setInstallerPath("");
      onRefresh();
    } catch (err) {
      onNotify("error", dict.tracker.title, String(err));
    } finally {
      setStarting(false);
    }
  };

  const handleFinish = async (sessionId: string) => {
    setFinishingId(sessionId);
    try {
      const trace = await api.finishInstallTrack(sessionId);
      onNotify(
        "success",
        trace.appName,
        `${trace.addedDirs.length} ${dict.tracker.dirs}, ${trace.addedFiles.length} ${dict.tracker.files}, ${
          trace.addedRegistryKeys.length + trace.addedRegistryValues.length
        } ${dict.tracker.regItems}`,
      );
      onRefresh();
    } catch (err) {
      onNotify("error", dict.tracker.title, String(err));
    } finally {
      setFinishingId(null);
    }
  };

  const handleCancel = async (sessionId: string) => {
    await api.cancelInstallTrack(sessionId);
    onRefresh();
  };

  const handleRollback = async (trace: InstallTrace) => {
    try {
      const leftovers = await api.getTraceLeftovers(trace.id);
      const synthetic: InstalledApp = {
        id: `trace:${trace.id}`,
        name: `${trace.appName} (Tracked)`,
        publisher: "",
        version: "",
        installDate: trace.completedAt.slice(0, 10),
        estimatedSizeKb: Math.round(trace.totalSizeBytes / 1024),
        installLocation: trace.addedDirs[0] ?? "",
        uninstallString: "",
        quietUninstallString: "",
        displayIcon: "",
        urlInfoAbout: "",
        registryKey: "",
        registryHive: "",
        registryView: "",
        productCode: "",
        packageFullName: "",
        packageFamilyName: "",
        kind: "win32",
        scope: "machine",
        arch: "x64",
        isSystemComponent: false,
        isWindowsUpdate: false,
        isValidEntry: true,
        hasUninstaller: false,
        trackedTraceId: trace.id,
      };
      onTrackedUninstall(synthetic, leftovers);
    } catch (err) {
      onNotify("error", trace.appName, String(err));
    }
  };

  const handleDeleteTrace = async (traceId: string) => {
    try {
      await api.deleteInstallTrace(traceId);
      onRefresh();
    } catch (err) {
      onNotify("error", dict.tracker.title, String(err));
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-1 flex-col overflow-y-auto bg-[var(--bg-canvas)] px-6 pt-5 pb-10">
      <div className="w-full max-w-[920px] space-y-6">
        <div>
          <h1 className="text-[17px] font-medium tracking-[-0.015em] text-[var(--text-primary)]">
            {dict.tracker.title}
          </h1>
          <p className="mt-0.5 text-[12.5px] text-[var(--text-secondary)]">
            {dict.tracker.subtitle}
          </p>
        </div>

        {/* Active Sessions Banner */}
        {activeSessions.map((sess) => (
          <div
            key={sess.sessionId}
            className="space-y-3 rounded-[14px] border border-[var(--success-border)] bg-[var(--success-bg)] p-4"
          >
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <span className="h-2 w-2 rounded-full bg-[var(--success-text)] animate-pulse" />
                <span className="text-[13.5px] font-medium text-[var(--text-primary)]">
                  {dict.tracker.activeTitle}: {sess.appName}
                </span>
              </div>
              <span className="num-tabular text-[11.5px] text-[var(--text-secondary)]">
                Baseline: {sess.baselineDirCount} dirs / {sess.baselineRegCount} reg entries
              </span>
            </div>
            <p className="text-[12.5px] text-[var(--text-secondary)]">{dict.tracker.activeHint}</p>
            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={finishingId === sess.sessionId}
                onClick={() => handleFinish(sess.sessionId)}
                className="ui-btn-primary h-8"
              >
                {finishingId === sess.sessionId ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" strokeWidth={1.5} />
                ) : (
                  <Square className="h-3.5 w-3.5" strokeWidth={1.5} />
                )}
                {dict.tracker.finishTracking}
              </button>
              <button
                type="button"
                onClick={() => handleCancel(sess.sessionId)}
                className="ui-btn-ghost h-8"
              >
                <X className="h-3.5 w-3.5" strokeWidth={1.5} />
                {dict.tracker.cancelTracking}
              </button>
            </div>
          </div>
        ))}

        {/* Start New Tracking Card */}
        <section className="ui-card space-y-4 p-5">
          <div className="flex items-center gap-2 text-[13.5px] font-medium text-[var(--text-primary)]">
            <Activity className="h-4 w-4 text-[var(--text-tertiary)]" strokeWidth={1.5} />
            <span>{dict.tracker.newTitle}</span>
          </div>

          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <input
              type="text"
              value={appName}
              onChange={(e) => setAppName(e.target.value)}
              placeholder={dict.tracker.appNamePlaceholder}
              className="ui-input h-9 px-3 text-[12.5px]"
            />
            <div className="flex gap-2">
              <input
                type="text"
                value={installerPath}
                onChange={(e) => setInstallerPath(e.target.value)}
                placeholder={dict.tracker.installerPlaceholder}
                className="ui-input h-9 flex-1 px-3 font-mono text-[12px]"
              />
              <button
                type="button"
                onClick={handleBrowse}
                className="ui-btn-secondary h-9 shrink-0"
              >
                <FileSearch
                  className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                  strokeWidth={1.5}
                />
                {dict.tracker.browseInstaller}
              </button>
            </div>
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 pt-0.5">
            <label className="flex items-center gap-2.5 text-[12.5px] text-[var(--text-secondary)] cursor-pointer">
              <input
                type="checkbox"
                checked={launchInstaller}
                onChange={(e) => setLaunchInstaller(e.target.checked)}
                className="ui-checkbox"
              />
              <span>{dict.tracker.launchOption}</span>
            </label>
            <button
              type="button"
              disabled={!appName.trim() || starting}
              onClick={handleStart}
              className="ui-btn-primary h-8.5"
            >
              {starting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" strokeWidth={1.5} />
              ) : (
                <Play className="h-3.5 w-3.5" strokeWidth={1.5} />
              )}
              {dict.tracker.startMonitoring}
            </button>
          </div>
        </section>

        {/* Saved Traces History */}
        <section className="space-y-2.5">
          <div className="flex items-center justify-between">
            <h2 className="text-[13.5px] font-medium text-[var(--text-primary)]">
              {dict.tracker.historyTitle}
            </h2>
            {traces.length > 0 && (
              <span className="num-tabular text-[12px] text-[var(--text-tertiary)]">
                {traces.length}
              </span>
            )}
          </div>

          {traces.length === 0 ? (
            <div className="rounded-[14px] border border-[var(--border-hairline)] bg-[var(--bg-subtle)] py-12 text-center text-[12.5px] text-[var(--text-secondary)]">
              {dict.tracker.emptyHistory}
            </div>
          ) : (
            <div className="space-y-2">
              {traces.map((trace) => {
                const expanded = expandedTraceId === trace.id;
                const regCount =
                  trace.addedRegistryKeys.length + trace.addedRegistryValues.length;
                return (
                  <div key={trace.id} className="ui-card overflow-hidden">
                    <div className="group flex flex-wrap items-center justify-between gap-3 px-4 py-3.5">
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-2">
                          <button
                            type="button"
                            onClick={() => setExpandedTraceId(expanded ? null : trace.id)}
                            className="ui-btn-ghost h-6 w-6 p-0"
                          >
                            {expanded ? (
                              <ChevronDown
                                className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                strokeWidth={1.5}
                              />
                            ) : (
                              <ChevronRight
                                className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                                strokeWidth={1.5}
                              />
                            )}
                          </button>
                          <span className="text-[13.5px] font-medium text-[var(--text-primary)]">
                            {trace.appName}
                          </span>
                          <span className="num-tabular text-[11.5px] text-[var(--text-tertiary)]">
                            {formatDateTime(trace.completedAt)}
                          </span>
                        </div>
                        <div className="num-tabular mt-1 ml-8 flex flex-wrap items-center gap-3.5 text-[12px] text-[var(--text-secondary)]">
                          <span>
                            {trace.addedDirs.length} {dict.tracker.dirs}
                          </span>
                          <span>
                            {trace.addedFiles.length} {dict.tracker.files}
                          </span>
                          <span>
                            {regCount} {dict.tracker.regItems}
                          </span>
                          {trace.totalSizeBytes > 0 && (
                            <span className="font-medium text-[var(--text-primary)]">
                              {formatBytes(trace.totalSizeBytes)}
                            </span>
                          )}
                        </div>
                      </div>

                      <div className="flex items-center gap-1">
                        <button
                          type="button"
                          onClick={() => handleRollback(trace)}
                          className="ui-btn-danger-ghost h-8 px-2.5 text-[12px]"
                        >
                          <Trash2 className="h-3.5 w-3.5" strokeWidth={1.5} />
                          {dict.tracker.trackedUninstall}
                        </button>
                        <button
                          type="button"
                          onClick={() => handleDeleteTrace(trace.id)}
                          title={dict.tracker.deleteTrace}
                          aria-label={dict.tracker.deleteTrace}
                          className="ui-btn-ghost h-8 w-8 p-0"
                        >
                          <X
                            className="h-3.5 w-3.5 text-[var(--text-tertiary)]"
                            strokeWidth={1.5}
                          />
                        </button>
                      </div>
                    </div>

                    {expanded && (
                      <div className="max-h-60 space-y-1.5 overflow-y-auto border-t border-[var(--border-hairline)] bg-[var(--bg-subtle)] px-4 py-3 font-mono text-[11px]">
                        {trace.addedDirs.map((d) => (
                          <div
                            key={d}
                            className="flex items-center gap-1.5 text-[var(--text-primary)]"
                          >
                            <Folder
                              className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
                              strokeWidth={1.5}
                            />
                            <span className="truncate">{d}</span>
                          </div>
                        ))}
                        {trace.addedFiles.map((f) => (
                          <div key={f} className="truncate pl-5 text-[var(--text-secondary)]">
                            + {f}
                          </div>
                        ))}
                        {trace.addedRegistryKeys.map((k) => (
                          <div
                            key={k}
                            className="flex items-center gap-1.5 text-[var(--text-primary)]"
                          >
                            <Database
                              className="h-3.5 w-3.5 shrink-0 text-[var(--text-tertiary)]"
                              strokeWidth={1.5}
                            />
                            <span className="truncate">{k}</span>
                          </div>
                        ))}
                        {trace.addedRegistryValues.map((rv, idx) => (
                          <div
                            key={`${rv.keyPath}-${rv.valueName}-${idx}`}
                            className="truncate pl-5 text-[var(--text-secondary)]"
                          >
                            [REG] {rv.keyPath} -&gt; {rv.valueName}
                          </div>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
