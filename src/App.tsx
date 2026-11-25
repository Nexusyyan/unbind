import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isTauriRuntime } from "./api/client";
import { AppListView } from "./components/AppListView";
import { BackupsView } from "./components/BackupsView";
import { BatchUninstallModal } from "./components/BatchUninstallModal";
import { ForceUninstallModal } from "./components/ForceUninstallModal";
import { InstallTrackerView } from "./components/InstallTrackerView";
import { SettingsView } from "./components/SettingsView";
import { Sidebar } from "./components/Sidebar";
import { StartupManagerView } from "./components/StartupManagerView";
import { TitleBar } from "./components/TitleBar";
import { ToastContainer } from "./components/ToastContainer";
import { UninstallWizardModal } from "./components/UninstallWizardModal";
import { getDict, resolveLocale } from "./i18n";
import type {
  ActiveTrackSession,
  AppSettings,
  BackupEntry,
  ExportFormat,
  InstallTrace,
  InstalledApp,
  LeftoverItem,
  NavTab,
  OperationLogEntry,
  StartupItem,
  SystemStatus,
  ToastMessage,
} from "./types";

const DEFAULT_SETTINGS: AppSettings = {
  portableMode: false,
  dataDir: "",
  useRecycleBin: true,
  autoBackupRegistry: true,
  createRestorePoint: false,
  showSystemComponents: false,
  showWindowsUpdates: false,
  language: "auto",
  theme: "system",
};

export function App() {
  const [activeTab, setActiveTab] = useState<NavTab>("apps");
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);

  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [appsLoading, setAppsLoading] = useState(true);
  const [icons, setIcons] = useState<Record<string, string>>({});
  const pendingIconIdsRef = useRef<Set<string>>(new Set());

  const [startupItems, setStartupItems] = useState<StartupItem[]>([]);
  const [startupLoading, setStartupLoading] = useState(false);

  const [activeTracks, setActiveTracks] = useState<ActiveTrackSession[]>([]);
  const [traces, setTraces] = useState<InstallTrace[]>([]);
  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [logs, setLogs] = useState<OperationLogEntry[]>([]);

  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const [dragHover, setDragHover] = useState(false);

  // Modal states
  const [wizardState, setWizardState] = useState<{
    app: InstalledApp;
    quiet: boolean;
    skipNative?: boolean;
    preloadedLeftovers?: LeftoverItem[];
  } | null>(null);

  const [forceModalState, setForceModalState] = useState<{
    initialPath?: string;
  } | null>(null);

  const [batchModalState, setBatchModalState] = useState<{
    apps: InstalledApp[];
    quiet: boolean;
  } | null>(null);

  const locale = useMemo(() => resolveLocale(settings.language), [settings.language]);
  const dict = useMemo(() => getDict(locale), [locale]);

  const notify = useCallback(
    (kind: ToastMessage["kind"], title: string, detail?: string) => {
      const id = `${Date.now()}-${Math.random()}`;
      setToasts((prev) => [...prev.slice(-3), { id, kind, title, detail }]);
      window.setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, 4500);
    },
    [],
  );

  // Apply Dark / Light / System theme class to <html>
  useEffect(() => {
    const apply = () => {
      const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
      const isDark =
        settings.theme === "dark" || (settings.theme === "system" && prefersDark);
      document.documentElement.classList.toggle("dark", isDark);
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", apply);
    return () => mq.removeEventListener("change", apply);
  }, [settings.theme]);

  const refreshStatus = useCallback(async () => {
    try {
      const s = await api.getSystemStatus();
      setStatus(s);
    } catch {
      // Ignore transient status read errors
    }
  }, []);

  const refreshApps = useCallback(async () => {
    setAppsLoading(true);
    try {
      const list = await api.listApps();
      setApps(list);
    } catch (err) {
      notify("error", "Failed to load applications", String(err));
    } finally {
      setAppsLoading(false);
    }
  }, [notify]);

  const refreshStartup = useCallback(async () => {
    setStartupLoading(true);
    try {
      const list = await api.listStartupItems();
      setStartupItems(list);
    } catch (err) {
      notify("error", "Failed to load startup items", String(err));
    } finally {
      setStartupLoading(false);
    }
  }, [notify]);

  const refreshTracker = useCallback(async () => {
    try {
      const [active, saved] = await Promise.all([
        api.listActiveTrackSessions(),
        api.listInstallTraces(),
      ]);
      setActiveTracks(active);
      setTraces(saved);
    } catch {
      // Ignore
    }
  }, []);

  const refreshBackupsAndLogs = useCallback(async () => {
    try {
      const [b, l] = await Promise.all([
        api.listRegistryBackups(),
        api.listOperationLogs(200),
      ]);
      setBackups(b);
      setLogs(l);
    } catch {
      // Ignore
    }
  }, []);

  // Initial load
  useEffect(() => {
    void (async () => {
      const [st, cfg] = await Promise.all([api.getSystemStatus(), api.getSettings()]);
      setStatus(st);
      setSettings(cfg);
      void refreshApps();
      void refreshStartup();
      void refreshTracker();
      void refreshBackupsAndLogs();
    })();
  }, [refreshApps, refreshStartup, refreshTracker, refreshBackupsAndLogs]);

  // Listen for native OS drag-and-drop (unblocked because UI runs non-elevated!)
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "over" || event.payload.type === "enter") {
          setDragHover(true);
        } else if (event.payload.type === "leave") {
          setDragHover(false);
        } else if (event.payload.type === "drop") {
          setDragHover(false);
          const dropped = event.payload.paths?.[0];
          if (dropped) {
            setForceModalState({ initialPath: dropped });
          }
        }
      })
      .then((fn) => {
        unlisten = fn;
      });
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Batch icon fetcher for visible rows
  const requestIcons = useCallback((ids: string[]) => {
    const fresh = ids.filter((id) => !pendingIconIdsRef.current.has(id));
    if (fresh.length === 0) return;
    fresh.forEach((id) => pendingIconIdsRef.current.add(id));

    void api
      .getAppIcons(fresh.slice(0, 40))
      .then((map) => {
        setIcons((prev) => {
          const next = { ...prev };
          for (const id of fresh.slice(0, 40)) {
            next[id] = map[id] ?? "";
          }
          return next;
        });
      })
      .catch(() => {
        fresh.forEach((id) => pendingIconIdsRef.current.delete(id));
      });
  }, []);

  const handleSaveSettings = useCallback(
    async (next: AppSettings) => {
      try {
        const saved = await api.saveSettings(next);
        setSettings(saved);
        await refreshStatus();
        notify("success", dict.settings.saved);
      } catch (err) {
        notify("error", dict.settings.title, String(err));
      }
    },
    [dict.settings.saved, dict.settings.title, notify, refreshStatus],
  );

  const handleElevate = useCallback(async () => {
    if (status?.isElevated || status?.workerConnected) return;
    try {
      await api.elevateSession();
      await refreshStatus();
      notify("success", dict.status.elevated, "Elevated helper session active");
    } catch (err) {
      notify("warning", dict.status.standard, String(err));
    }
  }, [status, refreshStatus, notify, dict.status.elevated, dict.status.standard]);

  const handleToggleLocale = useCallback(() => {
    const nextLang = locale === "zh-CN" ? "en" : "zh-CN";
    void handleSaveSettings({ ...settings, language: nextLang });
  }, [locale, settings, handleSaveSettings]);

  const handleCycleTheme = useCallback(() => {
    const order = ["system", "dark", "light"];
    const next = order[(order.indexOf(settings.theme) + 1) % order.length];
    void handleSaveSettings({ ...settings, theme: next });
  }, [settings, handleSaveSettings]);

  const handleExport = useCallback(
    async (format: ExportFormat, visibleApps: InstalledApp[]) => {
      const savePath = await api.pickSavePath(
        `installed-apps-${new Date().toISOString().slice(0, 10)}.${format}`,
        format,
      );
      if (!savePath) return;
      try {
        const count = await api.exportApps(format, savePath, visibleApps);
        notify("success", dict.toolbar.export, `Exported ${count} apps to ${savePath}`);
      } catch (err) {
        notify("error", dict.toolbar.export, String(err));
      }
    },
    [dict.toolbar.export, notify],
  );

  const handleCheckLocksForApp = useCallback(
    async (app: InstalledApp) => {
      if (!app.installLocation) return;
      try {
        const locks = await api.checkFileLocks([app.installLocation]);
        if (locks.length === 0) {
          notify("success", app.name, dict.force.noLocks);
        } else {
          setForceModalState({ initialPath: app.installLocation });
        }
      } catch (err) {
        notify("error", app.name, String(err));
      }
    },
    [dict.force.noLocks, notify],
  );

  const counts = useMemo(() => {
    let win32Count = 0;
    let storeCount = 0;
    let totalSizeKb = 0;
    for (const a of apps) {
      if (!settings.showSystemComponents && a.isSystemComponent) continue;
      if (!settings.showWindowsUpdates && a.isWindowsUpdate) continue;
      if (a.kind === "storeApp") storeCount++;
      else win32Count++;
      totalSizeKb += a.estimatedSizeKb;
    }
    return {
      apps: win32Count,
      store: storeCount,
      startup: startupItems.filter((s) => s.enabled).length,
      traces: traces.length,
      activeTracks: activeTracks.length,
      backups: backups.length,
      totalSizeKb,
    };
  }, [apps, settings, startupItems, traces, activeTracks, backups]);

  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-[var(--bg-chrome)] text-[var(--text-primary)]">
      <TitleBar
        dict={dict}
        locale={locale}
        theme={settings.theme}
        status={status}
        onElevate={handleElevate}
        onToggleLocale={handleToggleLocale}
        onCycleTheme={handleCycleTheme}
      />

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <Sidebar
          dict={dict}
          activeTab={activeTab}
          onSelectTab={setActiveTab}
          counts={counts}
          totalSizeKb={counts.totalSizeKb}
        />

        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-tl-[12px] border-t border-l border-[var(--border-hairline)] bg-[var(--bg-canvas)]">
          {(activeTab === "apps" || activeTab === "store") && (
            <AppListView
              dict={dict}
              apps={apps}
              loading={appsLoading}
              storeOnly={activeTab === "store"}
              settings={settings}
              icons={icons}
              onRequestIcons={requestIcons}
              onRefresh={refreshApps}
              onUninstall={(app, quiet) => setWizardState({ app, quiet })}
              onForceUninstall={(app) => {
                if (app) {
                  setWizardState({ app, quiet: false, skipNative: true });
                } else {
                  setForceModalState({});
                }
              }}
              onBatchUninstall={(selected, quiet) =>
                setBatchModalState({ apps: selected, quiet })
              }
              onExport={handleExport}
              onCheckLocks={handleCheckLocksForApp}
              onNotify={notify}
            />
          )}

          {activeTab === "startup" && (
            <StartupManagerView
              dict={dict}
              items={startupItems}
              loading={startupLoading}
              onRefresh={refreshStartup}
              onNotify={notify}
            />
          )}

          {activeTab === "tracker" && (
            <InstallTrackerView
              dict={dict}
              activeSessions={activeTracks}
              traces={traces}
              onRefresh={() => {
                void refreshTracker();
                void refreshApps();
              }}
              onTrackedUninstall={(syntheticApp, leftovers) =>
                setWizardState({
                  app: syntheticApp,
                  quiet: false,
                  skipNative: true,
                  preloadedLeftovers: leftovers,
                })
              }
              onNotify={notify}
            />
          )}

          {activeTab === "backups" && (
            <BackupsView
              dict={dict}
              backups={backups}
              logs={logs}
              onRefresh={refreshBackupsAndLogs}
              onNotify={notify}
            />
          )}

          {activeTab === "settings" && (
            <SettingsView
              dict={dict}
              settings={settings}
              status={status}
              onSave={handleSaveSettings}
            />
          )}
        </main>
      </div>

      {/* Global Drag-and-Drop Overlay */}
      {dragHover && (
        <div className="pointer-events-none fixed inset-0 z-50 flex items-center justify-center bg-[var(--backdrop-scrim)] backdrop-blur-[2px]">
          <div className="rounded-[16px] border border-dashed border-[var(--border-strong)] bg-[var(--bg-surface)] px-10 py-7 text-center shadow-[var(--shadow-modal)]">
            <div className="text-[15px] font-medium text-[var(--text-primary)]">
              {dict.force.title}
            </div>
            <div className="mt-1.5 text-[12.5px] text-[var(--text-secondary)]">
              {dict.appList.dropHint}
            </div>
          </div>
        </div>
      )}

      {/* Uninstall / Leftover Wizard Modal */}
      {wizardState && (
        <UninstallWizardModal
          dict={dict}
          app={wizardState.app}
          initialQuiet={wizardState.quiet}
          skipNative={wizardState.skipNative}
          preloadedLeftovers={wizardState.preloadedLeftovers}
          settings={settings}
          onStatusRefresh={refreshStatus}
          onClose={(didModify) => {
            setWizardState(null);
            if (didModify) {
              void refreshApps();
              void refreshBackupsAndLogs();
              void refreshStartup();
            }
          }}
        />
      )}

      {/* Force Removal Modal */}
      {forceModalState && (
        <ForceUninstallModal
          dict={dict}
          initialPath={forceModalState.initialPath}
          onClose={() => setForceModalState(null)}
          onProceedToReview={(targetApp, leftovers) => {
            setForceModalState(null);
            setWizardState({
              app: targetApp,
              quiet: false,
              skipNative: true,
              preloadedLeftovers: leftovers,
            });
          }}
        />
      )}

      {/* Batch Uninstall Queue Modal */}
      {batchModalState && (
        <BatchUninstallModal
          dict={dict}
          apps={batchModalState.apps}
          initialQuiet={batchModalState.quiet}
          onClose={(didModify) => {
            setBatchModalState(null);
            if (didModify) {
              void refreshApps();
              void refreshBackupsAndLogs();
            }
          }}
          onReviewCombinedLeftovers={(label, leftovers) => {
            const first = batchModalState.apps[0];
            setBatchModalState(null);
            setWizardState({
              app: { ...first, name: label },
              quiet: false,
              skipNative: true,
              preloadedLeftovers: leftovers,
            });
          }}
        />
      )}

      <ToastContainer
        toasts={toasts}
        onDismiss={(id) => setToasts((prev) => prev.filter((t) => t.id !== id))}
      />
    </div>
  );
}
export default App;
