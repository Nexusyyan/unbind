import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isTauriRuntime } from "./api/client";
import { AppListView } from "./components/AppListView";
import { BackupsView } from "./components/BackupsView";
import { BatchUninstallModal } from "./components/BatchUninstallModal";
import { DropActionModal } from "./components/DropActionModal";
import { ForceUninstallModal } from "./components/ForceUninstallModal";
import { InstallTrackerView } from "./components/InstallTrackerView";
import { SettingsView } from "./components/SettingsView";
import { Sidebar } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { ToastContainer } from "./components/ToastContainer";
import { ToolsView } from "./components/ToolsView";
import { UninstallWizardModal } from "./components/UninstallWizardModal";
import { getDict, resolveLocale } from "./i18n";
import type {
  ActiveTrackSession,
  AppListViewState,
  AppSettings,
  BackupEntry,
  CliAction,
  ExportFormat,
  InstallTrace,
  InstalledApp,
  InstallerExitedPayload,
  LeftoverItem,
  NavTab,
  OperationLogEntry,
  PortableApp,
  StartupItem,
  SystemStatus,
  ToastMessage,
  ToolsSubTab,
  WingetUpdateInfo,
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
  customTrackRoots: [],
  portableAppRoots: [],
  defaultTrackMode: "hybridSnapshot",
};

const DEFAULT_LIST_STATE: AppListViewState = {
  query: "",
  filterMode: "all",
  sortField: "name",
  sortDesc: false,
};

function isInstallerFilePath(path: string): boolean {
  return /\.(exe|msi|msix)$/i.test(path.trim());
}

export function App() {
  const [activeTab, setActiveTab] = useState<NavTab>("apps");
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);

  // Lifted filter/sort/search states per list tab so switching tabs never loses context.
  const [appsViewState, setAppsViewState] = useState<AppListViewState>(DEFAULT_LIST_STATE);
  const [storeViewState, setStoreViewState] = useState<AppListViewState>(DEFAULT_LIST_STATE);

  const [apps, setApps] = useState<InstalledApp[]>([]);
  const [appsLoading, setAppsLoading] = useState(true);
  const [resolvingSizes, setResolvingSizes] = useState(false);
  const [icons, setIcons] = useState<Record<string, string>>({});
  const pendingIconIdsRef = useRef<Set<string>>(new Set());

  const [startupItems, setStartupItems] = useState<StartupItem[]>([]);
  const [startupLoading, setStartupLoading] = useState(false);

  const [toolsSubTab, setToolsSubTab] = useState<ToolsSubTab>("startup");
  const [portableApps, setPortableApps] = useState<PortableApp[]>([]);
  const [portableLoading, setPortableLoading] = useState(false);
  const [portableIcons, setPortableIcons] = useState<Record<string, string>>({});

  const [wingetUpdates, setWingetUpdates] = useState<Record<string, WingetUpdateInfo>>({});
  const [wingetChecked, setWingetChecked] = useState(false);
  const [checkingWinget, setCheckingWinget] = useState(false);
  const [upgradingIds, setUpgradingIds] = useState<Set<string>>(new Set());
  const [batchUpgradingLabel, setBatchUpgradingLabel] = useState<string | null>(null);

  const [activeTracks, setActiveTracks] = useState<ActiveTrackSession[]>([]);
  const [traces, setTraces] = useState<InstallTrace[]>([]);
  const [prefillTrackerPath, setPrefillTrackerPath] = useState<string | null>(null);

  const [backups, setBackups] = useState<BackupEntry[]>([]);
  const [logs, setLogs] = useState<OperationLogEntry[]>([]);

  const [toasts, setToasts] = useState<ToastMessage[]>([]);
  const [dragHover, setDragHover] = useState(false);
  const [droppedInstallerPath, setDroppedInstallerPath] = useState<string | null>(null);

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
  const dictRef = useRef(dict);
  dictRef.current = dict;

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

  const handleCliAction = useCallback((action: CliAction) => {
    if (action.action === "forceRemove") {
      setForceModalState({ initialPath: action.path });
    } else if (action.action === "trackInstall") {
      setPrefillTrackerPath(action.path);
      setActiveTab("tracker");
    }
  }, []);

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
      const needsMeasure = list.some(
        (a) =>
          a.estimatedSizeKb === 0 &&
          (Boolean(a.installLocation) || (a.kind === "storeApp" && Boolean(a.packageFamilyName))),
      );
      if (needsMeasure && isTauriRuntime()) {
        setResolvingSizes(true);
        void api.resolveAppSizes().catch(() => setResolvingSizes(false));
      } else {
        setResolvingSizes(false);
      }
    } catch (err) {
      notify("error", dictRef.current.common.loadAppsFailed, String(err));
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
      notify("error", dictRef.current.common.loadStartupFailed, String(err));
    } finally {
      setStartupLoading(false);
    }
  }, [notify]);

  const refreshPortableApps = useCallback(async (roots: string[]) => {
    if (!roots || roots.length === 0) {
      setPortableApps([]);
      setPortableIcons({});
      return;
    }
    setPortableLoading(true);
    try {
      const list = await api.listPortableApps(roots);
      setPortableApps(list);
      if (list.length > 0 && isTauriRuntime()) {
        const ids = list.map((p) => p.id);
        void api
          .getPortableAppIcons(ids)
          .then((map) => setPortableIcons(map))
          .catch(() => {});
      } else {
        setPortableIcons({});
      }
    } catch (err) {
      notify("error", dictRef.current.portable.title, String(err));
    } finally {
      setPortableLoading(false);
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

  // Initial load + pending CLI action check
  useEffect(() => {
    void (async () => {
      const [st, cfg, pendingAction] = await Promise.all([
        api.getSystemStatus(),
        api.getSettings(),
        api.takePendingCliAction(),
      ]);
      setStatus(st);
      setSettings(cfg);
      if (pendingAction) {
        handleCliAction(pendingAction);
      }
      void refreshApps();
      void refreshStartup();
      void refreshPortableApps(cfg.portableAppRoots ?? []);
      void refreshTracker();
      void refreshBackupsAndLogs();
    })();
  }, [
    refreshApps,
    refreshStartup,
    refreshPortableApps,
    refreshTracker,
    refreshBackupsAndLogs,
    handleCliAction,
  ]);

  // Listen to backend streaming events (async real sizes, installer exit, single-instance CLI)
  useEffect(() => {
    if (!isTauriRuntime()) return;
    const unsubs: Array<() => void> = [];

    void listen<Record<string, number>>("apps://size-batch", (event) => {
      const batch = event.payload;
      if (!batch || Object.keys(batch).length === 0) return;
      setApps((prev) =>
        prev.map((app) => {
          const kb = batch[app.id];
          if (kb !== undefined && kb > 0) {
            return { ...app, estimatedSizeKb: kb, sizeMeasured: true };
          }
          return app;
        }),
      );
    }).then((u) => unsubs.push(u));

    void listen<boolean>("apps://size-done", () => {
      setResolvingSizes(false);
    }).then((u) => unsubs.push(u));

    void listen<InstallerExitedPayload>("tracker://installer-exited", (event) => {
      const { sessionId } = event.payload;
      setActiveTracks((prev) => {
        const hit = prev.find((s) => s.sessionId === sessionId);
        if (hit) {
          notify("success", dictRef.current.tracker.installerExitedToast, hit.appName);
        }
        return prev.map((s) =>
          s.sessionId === sessionId ? { ...s, installerExited: true } : s,
        );
      });
    }).then((u) => unsubs.push(u));

    void listen<CliAction>("cli://action", (event) => {
      if (event.payload) {
        handleCliAction(event.payload);
      }
    }).then((u) => unsubs.push(u));

    return () => {
      unsubs.forEach((fn) => fn());
    };
  }, [notify, handleCliAction]);

  // Listen for native OS drag-and-drop (smart routing for installers vs folders/shortcuts)
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
            if (isInstallerFilePath(dropped)) {
              setDroppedInstallerPath(dropped);
            } else {
              setForceModalState({ initialPath: dropped });
            }
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
    async (next: AppSettings, silent = false) => {
      try {
        const prevRootsKey = (settings.portableAppRoots ?? []).join("|");
        const saved = await api.saveSettings(next);
        setSettings(saved);
        await refreshStatus();
        const nextRootsKey = (saved.portableAppRoots ?? []).join("|");
        if (prevRootsKey !== nextRootsKey) {
          void refreshPortableApps(saved.portableAppRoots ?? []);
        }
        if (!silent) {
          notify("success", dictRef.current.settings.saved);
        }
      } catch (err) {
        notify("error", dictRef.current.settings.title, String(err));
      }
    },
    [settings.portableAppRoots, notify, refreshStatus, refreshPortableApps],
  );

  const handleAddPortableRoot = useCallback(async () => {
    const picked = await api.pickFolder();
    if (!picked) return;
    const current = settings.portableAppRoots ?? [];
    if (current.some((r) => r.toLowerCase() === picked.toLowerCase())) return;
    await handleSaveSettings({ ...settings, portableAppRoots: [...current, picked] }, true);
  }, [settings, handleSaveSettings]);

  const handleRemovePortableRoot = useCallback(
    async (root: string) => {
      const current = settings.portableAppRoots ?? [];
      await handleSaveSettings(
        { ...settings, portableAppRoots: current.filter((r) => r !== root) },
        true,
      );
    },
    [settings, handleSaveSettings],
  );

  const handleDeepRemovePortable = useCallback(
    async (targetPath: string) => {
      try {
        const [targetApp, leftovers] = await api.scanForceTarget(targetPath);
        setWizardState({
          app: targetApp,
          quiet: false,
          skipNative: true,
          preloadedLeftovers: leftovers,
        });
      } catch (err) {
        notify("error", dictRef.current.portable.deepRemove, String(err));
      }
    },
    [notify],
  );

  const handleCheckWingetUpdates = useCallback(async () => {
    setCheckingWinget(true);
    try {
      const res = await api.checkWingetUpdates();
      if (!res.available) {
        notify("warning", dictRef.current.winget.unavailableTitle, res.error ?? dictRef.current.winget.unavailableDesc);
        return;
      }
      if (res.error) {
        notify("error", dictRef.current.toolbar.checkUpdates, res.error);
        return;
      }
      const mapped: Record<string, WingetUpdateInfo> = {};
      for (const upd of res.updates) {
        if (upd.matchedAppId) {
          mapped[upd.matchedAppId] = upd;
        }
      }
      setWingetUpdates(mapped);
      setWingetChecked(true);
      const count = Object.keys(mapped).length;
      if (count === 0) {
        notify("success", dictRef.current.winget.upToDateToast, res.wingetVersion);
      } else {
        notify("info", `${count} ${dictRef.current.winget.updatesFoundToast}`, res.wingetVersion);
      }
    } catch (err) {
      notify("error", dictRef.current.toolbar.checkUpdates, String(err));
    } finally {
      setCheckingWinget(false);
    }
  }, [notify]);

  const handleUpgradeApp = useCallback(
    async (app: InstalledApp, update: WingetUpdateInfo) => {
      setUpgradingIds((prev) => new Set(prev).add(app.id));
      try {
        const res = await api.runWingetUpgrade(update.packageId, app.name);
        if (res.success) {
          setWingetUpdates((prev) => {
            const next = { ...prev };
            delete next[app.id];
            return next;
          });
          notify("success", `${dictRef.current.winget.upgradeDone}: ${app.name}`, res.message);
          void refreshApps();
          void refreshBackupsAndLogs();
        } else {
          notify("warning", `${dictRef.current.winget.upgradeFailed}: ${app.name}`, res.message);
          void refreshBackupsAndLogs();
        }
      } catch (err) {
        notify("error", `${dictRef.current.winget.upgradeFailed}: ${app.name}`, String(err));
      } finally {
        setUpgradingIds((prev) => {
          const next = new Set(prev);
          next.delete(app.id);
          return next;
        });
      }
    },
    [notify, refreshApps, refreshBackupsAndLogs],
  );

  const handleUpgradeAllApps = useCallback(
    async (targets: Array<{ app: InstalledApp; update: WingetUpdateInfo }>) => {
      if (targets.length === 0) return;
      let okCount = 0;
      let failCount = 0;
      for (let i = 0; i < targets.length; i++) {
        const { app, update } = targets[i];
        setBatchUpgradingLabel(
          dictRef.current.winget.batchProgress
            .replace("{current}", String(i + 1))
            .replace("{total}", String(targets.length))
            .replace("{name}", app.name),
        );
        setUpgradingIds((prev) => new Set(prev).add(app.id));
        try {
          const res = await api.runWingetUpgrade(update.packageId, app.name);
          if (res.success) {
            okCount++;
            setWingetUpdates((prev) => {
              const next = { ...prev };
              delete next[app.id];
              return next;
            });
          } else {
            failCount++;
          }
        } catch {
          failCount++;
        } finally {
          setUpgradingIds((prev) => {
            const next = new Set(prev);
            next.delete(app.id);
            return next;
          });
        }
      }
      setBatchUpgradingLabel(null);
      notify(
        failCount === 0 ? "success" : "warning",
        dictRef.current.toolbar.upgradeAll,
        `${dictRef.current.wizard.succeeded}: ${okCount}, ${dictRef.current.wizard.failed}: ${failCount}`,
      );
      void refreshApps();
      void refreshBackupsAndLogs();
    },
    [notify, refreshApps, refreshBackupsAndLogs],
  );

  const handleElevate = useCallback(async () => {
    if (status?.isElevated || status?.workerConnected) return;
    try {
      await api.elevateSession();
      await refreshStatus();
      notify("success", dictRef.current.status.elevated, dictRef.current.status.elevatedDetail);
    } catch (err) {
      notify("warning", dictRef.current.status.standard, String(err));
    }
  }, [status, refreshStatus, notify]);

  const handleToggleLocale = useCallback(() => {
    const nextLang = locale === "zh-CN" ? "en" : "zh-CN";
    void handleSaveSettings({ ...settings, language: nextLang }, true);
  }, [locale, settings, handleSaveSettings]);

  const handleCycleTheme = useCallback(() => {
    const order = ["system", "dark", "light"];
    const next = order[(order.indexOf(settings.theme) + 1) % order.length];
    void handleSaveSettings({ ...settings, theme: next }, true);
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
        notify(
          "success",
          dictRef.current.toolbar.export,
          `${dictRef.current.common.exportedPrefix} ${count} ${dictRef.current.common.itemsTo} ${savePath}`,
        );
      } catch (err) {
        notify("error", dictRef.current.toolbar.export, String(err));
      }
    },
    [notify],
  );

  const handleCheckLocksForApp = useCallback(
    async (app: InstalledApp) => {
      if (!app.installLocation) return;
      try {
        const locks = await api.checkFileLocks([app.installLocation]);
        if (locks.length === 0) {
          notify("success", app.name, dictRef.current.force.noLocks);
        } else {
          setForceModalState({ initialPath: app.installLocation });
        }
      } catch (err) {
        notify("error", app.name, String(err));
      }
    },
    [notify],
  );

  const handleCleanBrokenEntries = useCallback(
    async (brokenApps: InstalledApp[]) => {
      const items: LeftoverItem[] = brokenApps
        .filter((a) => Boolean(a.registryKey))
        .map((a) => ({
          id: `broken-${a.id}`,
          kind: "registryKey",
          path: a.registryKey,
          valueName: null,
          sizeBytes: 0,
          confidence: "high",
          reason: "Invalid Uninstall registry entry",
          reasonCode: "broken_entry",
          reasonParam: null,
          selectedByDefault: true,
          requiresElevation: a.registryHive.toUpperCase() === "HKLM",
        }));
      if (items.length === 0) return;

      try {
        const report = await api.cleanLeftovers("InvalidEntries", items, {
          useRecycleBin: settings.useRecycleBin,
          backupRegistry: settings.autoBackupRegistry,
          createRestorePoint: false,
          scheduleRebootOnLock: false,
          terminateLockingProcesses: false,
        });
        notify(
          report.failed === 0 ? "success" : "warning",
          dictRef.current.toolbar.cleanAllBrokenTitle,
          `${dictRef.current.wizard.succeeded}: ${report.succeeded}, ${dictRef.current.wizard.failed}: ${report.failed}`,
        );
        void refreshApps();
        void refreshBackupsAndLogs();
        void refreshStatus();
      } catch (err) {
        notify("error", dictRef.current.toolbar.cleanAllBrokenTitle, String(err));
      }
    },
    [settings, notify, refreshApps, refreshBackupsAndLogs, refreshStatus],
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
      tools: startupItems.filter((s) => s.enabled).length + portableApps.length,
      traces: traces.length,
      activeTracks: activeTracks.length,
      backups: backups.length,
      totalSizeKb,
    };
  }, [apps, settings, startupItems, portableApps, traces, activeTracks, backups]);

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
          resolvingSizes={resolvingSizes}
        />

        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-tl-[12px] border-t border-l border-[var(--border-hairline)] bg-[var(--bg-canvas)]">
          {(activeTab === "apps" || activeTab === "store") && (
            <AppListView
              dict={dict}
              apps={apps}
              loading={appsLoading}
              storeOnly={activeTab === "store"}
              settings={settings}
              viewState={activeTab === "store" ? storeViewState : appsViewState}
              onChangeViewState={activeTab === "store" ? setStoreViewState : setAppsViewState}
              icons={icons}
              onRequestIcons={requestIcons}
              wingetUpdates={wingetUpdates}
              wingetChecked={wingetChecked}
              checkingWinget={checkingWinget}
              upgradingIds={upgradingIds}
              batchUpgradingLabel={batchUpgradingLabel}
              onCheckWingetUpdates={handleCheckWingetUpdates}
              onUpgradeApp={handleUpgradeApp}
              onUpgradeAllApps={handleUpgradeAllApps}
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
              onCleanBrokenEntries={handleCleanBrokenEntries}
              onExport={handleExport}
              onCheckLocks={handleCheckLocksForApp}
              onNotify={notify}
            />
          )}

          {activeTab === "tools" && (
            <ToolsView
              dict={dict}
              subTab={toolsSubTab}
              onChangeSubTab={setToolsSubTab}
              startupItems={startupItems}
              startupLoading={startupLoading}
              onRefreshStartup={refreshStartup}
              portableRoots={settings.portableAppRoots ?? []}
              portableApps={portableApps}
              portableLoading={portableLoading}
              portableIcons={portableIcons}
              onAddPortableRoot={handleAddPortableRoot}
              onRemovePortableRoot={handleRemovePortableRoot}
              onRefreshPortable={() => void refreshPortableApps(settings.portableAppRoots ?? [])}
              onDeepRemovePortable={handleDeepRemovePortable}
              onNotify={notify}
            />
          )}

          {activeTab === "tracker" && (
            <InstallTrackerView
              dict={dict}
              activeSessions={activeTracks}
              traces={traces}
              defaultExtraRoots={settings.customTrackRoots ?? []}
              defaultTrackMode={settings.defaultTrackMode ?? "hybridSnapshot"}
              prefillInstallerPath={prefillTrackerPath}
              onConsumePrefill={() => setPrefillTrackerPath(null)}
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
              locale={locale}
              settings={settings}
              status={status}
              onSave={(next) => handleSaveSettings(next, true)}
              onNotify={notify}
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

      {/* Smart Installer Drop Choice Modal */}
      {droppedInstallerPath && (
        <DropActionModal
          dict={dict}
          path={droppedInstallerPath}
          onClose={() => setDroppedInstallerPath(null)}
          onForceRemove={() => {
            const p = droppedInstallerPath;
            setDroppedInstallerPath(null);
            setForceModalState({ initialPath: p });
          }}
          onTrackInstall={() => {
            const p = droppedInstallerPath;
            setDroppedInstallerPath(null);
            setPrefillTrackerPath(p);
            setActiveTab("tracker");
          }}
        />
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
          onNotify={notify}
          onClose={(didModify) => {
            setWizardState(null);
            if (didModify) {
              void refreshApps();
              void refreshBackupsAndLogs();
              void refreshStartup();
              void refreshPortableApps(settings.portableAppRoots ?? []);
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
