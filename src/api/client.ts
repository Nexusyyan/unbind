import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  ActiveTrackSession,
  AppSettings,
  BackupEntry,
  CleanupOptions,
  CleanupReport,
  CliAction,
  EtwSessionStats,
  ExportFormat,
  InstallTrace,
  InstalledApp,
  LeftoverItem,
  LockingProcessInfo,
  OperationLogEntry,
  PendingUpdateOutcome,
  PortableApp,
  RunningProcessInfo,
  ShellIntegrationStatus,
  ShortcutLocation,
  StartupItem,
  SystemStatus,
  TrackMode,
  TraySnapshot,
  UninstallExecutionResult,
  UpdateCheckResult,
  UpdateProgressEvent,
  UpdateStateSnapshot,
  WingetCheckResult,
  WingetUpgradeResult,
} from "../types";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const PREVIEW_MOCK_UPDATE: UpdateCheckResult = {
  currentVersion: "0.5.1",
  latestVersion: "0.6.0",
  latestTag: "v0.6.0",
  hasUpdate: true,
  releaseUrl: "https://github.com/Nexusyyan/unbind/releases/tag/v0.6.0",
  publishedAt: "2026-11-26T08:00:00Z",
  notesPreview:
    "### Highlights\n- In-app verified self-updater with SHA-256 + PE integrity checks\n- One-click Restart to Update preserving portable data and settings\n- Silent startup update check with subtle titlebar status pill",
  exeAssetName: "unbind-v0.6.0-x86_64-windows.exe",
  exeAssetUrl:
    "https://github.com/Nexusyyan/unbind/releases/download/v0.6.0/unbind-v0.6.0-x86_64-windows.exe",
  exeAssetSize: 7_340_032,
  checksumsUrl:
    "https://github.com/Nexusyyan/unbind/releases/download/v0.6.0/SHA256SUMS.txt",
};

const EMPTY_UPDATE_PROGRESS: UpdateProgressEvent = {
  stage: "idle",
  version: "",
  tag: "",
  downloadedBytes: 0,
  totalBytes: 0,
  percent: 0,
  error: null,
  staged: null,
};

let previewUpdateCheck: UpdateCheckResult | null = null;
let previewUpdateProgress: UpdateProgressEvent = { ...EMPTY_UPDATE_PROGRESS };
let previewDownloadTimer: number | null = null;
const previewProgressListeners = new Set<(event: UpdateProgressEvent) => void>();

function emitPreviewProgress(next: UpdateProgressEvent) {
  previewUpdateProgress = next;
  previewProgressListeners.forEach((fn) => fn(next));
}

function getPreviewUpdateMode(): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get("previewUpdate");
}

export const api = {
  async getSystemStatus(): Promise<SystemStatus> {
    if (!isTauriRuntime()) {
      return {
        isElevated: false,
        workerConnected: false,
        portableMode: true,
        dataDir: "C:\\Portable\\Unbind\\unbind-data",
        windowsVersion: "Windows 11 Preview",
        appVersion: "0.5.1",
      };
    }
    return invoke<SystemStatus>("get_system_status");
  },

  async getSettings(): Promise<AppSettings> {
    if (!isTauriRuntime()) {
      return {
        portableMode: true,
        dataDir: "C:\\Portable\\Unbind\\unbind-data",
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
        closeToTray: false,
        autoCheckUpdates: true,
      };
    }
    return invoke<AppSettings>("get_settings");
  },

  async saveSettings(settings: AppSettings): Promise<AppSettings> {
    if (!isTauriRuntime()) return settings;
    return invoke<AppSettings>("save_settings", { settings });
  },

  async getTraySnapshot(): Promise<TraySnapshot> {
    if (!isTauriRuntime()) {
      return {
        appVersion: "0.5.1",
        isElevated: false,
        workerConnected: false,
        portableMode: true,
        mainWindowVisible: true,
        installedCount: 42,
        activeTrackCount: 0,
        language: "auto",
        resolvedLocale: "en",
        theme: "system",
        closeToTray: false,
      };
    }
    return invoke<TraySnapshot>("get_tray_snapshot");
  },

  async resizeTrayMenu(height: number): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("resize_tray_menu", { height });
  },

  async hideTrayMenu(): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("hide_tray_menu");
  },

  async previewTrayMenu(): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("preview_tray_menu");
  },

  async executeTrayAction(action: string): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("execute_tray_action", { action });
  },

  async listApps(): Promise<InstalledApp[]> {
    if (!isTauriRuntime()) return [];
    return invoke<InstalledApp[]>("list_apps");
  },

  async resolveAppSizes(): Promise<void> {
    if (!isTauriRuntime()) return;
    return invoke<void>("resolve_app_sizes");
  },

  async getAppIcons(appIds: string[]): Promise<Record<string, string>> {
    if (!isTauriRuntime() || appIds.length === 0) return {};
    return invoke<Record<string, string>>("get_app_icons", { appIds });
  },

  async elevateSession(): Promise<boolean> {
    if (!isTauriRuntime()) return true;
    return invoke<boolean>("elevate_session");
  },

  async runUninstall(app: InstalledApp, quiet: boolean): Promise<UninstallExecutionResult> {
    return invoke<UninstallExecutionResult>("run_uninstall", { app, quiet });
  },

  async scanAppLeftovers(app: InstalledApp): Promise<LeftoverItem[]> {
    return invoke<LeftoverItem[]>("scan_app_leftovers", { app });
  },

  async scanForceTarget(path: string): Promise<[InstalledApp, LeftoverItem[]]> {
    return invoke<[InstalledApp, LeftoverItem[]]>("scan_force_target", { path });
  },

  async cleanLeftovers(
    appName: string,
    items: LeftoverItem[],
    options: CleanupOptions,
  ): Promise<CleanupReport> {
    return invoke<CleanupReport>("clean_leftovers", { appName, items, options });
  },

  async findRunningAppProcesses(installLocation: string): Promise<RunningProcessInfo[]> {
    if (!isTauriRuntime() || !installLocation.trim()) return [];
    return invoke<RunningProcessInfo[]>("find_running_app_processes", { installLocation });
  },

  async checkFileLocks(paths: string[]): Promise<LockingProcessInfo[]> {
    return invoke<LockingProcessInfo[]>("check_file_locks", { paths });
  },

  async terminateLockingProcesses(
    pids: number[],
  ): Promise<Array<[number, boolean, string | null]>> {
    return invoke<Array<[number, boolean, string | null]>>("terminate_locking_processes", { pids });
  },

  async listStartupItems(): Promise<StartupItem[]> {
    if (!isTauriRuntime()) return [];
    return invoke<StartupItem[]>("list_startup_items");
  },

  async setStartupEnabled(item: StartupItem, enabled: boolean): Promise<void> {
    return invoke<void>("set_startup_enabled", { item, enabled });
  },

  async deleteStartupItem(item: StartupItem): Promise<void> {
    return invoke<void>("delete_startup_item", { item });
  },

  async startInstallTrack(
    appName: string,
    installerPath: string,
    launchInstaller: boolean,
    extraRoots?: string[],
    trackMode?: TrackMode,
  ): Promise<ActiveTrackSession> {
    return invoke<ActiveTrackSession>("start_install_track", {
      appName,
      installerPath,
      launchInstaller,
      extraRoots: extraRoots ?? [],
      trackMode: trackMode ?? "hybridSnapshot",
    });
  },

  async pollInstallTrackStats(sessionId: string): Promise<EtwSessionStats | null> {
    if (!isTauriRuntime()) return null;
    return invoke<EtwSessionStats | null>("poll_install_track_stats", { sessionId });
  },

  async finishInstallTrack(sessionId: string): Promise<InstallTrace> {
    return invoke<InstallTrace>("finish_install_track", { sessionId });
  },

  async cancelInstallTrack(sessionId: string): Promise<boolean> {
    return invoke<boolean>("cancel_install_track", { sessionId });
  },

  async listActiveTrackSessions(): Promise<ActiveTrackSession[]> {
    if (!isTauriRuntime()) return [];
    return invoke<ActiveTrackSession[]>("list_active_track_sessions");
  },

  async listInstallTraces(): Promise<InstallTrace[]> {
    if (!isTauriRuntime()) return [];
    return invoke<InstallTrace[]>("list_install_traces");
  },

  async getTraceLeftovers(traceId: string): Promise<LeftoverItem[]> {
    return invoke<LeftoverItem[]>("get_trace_leftovers", { traceId });
  },

  async deleteInstallTrace(traceId: string): Promise<void> {
    return invoke<void>("delete_install_trace", { traceId });
  },

  async exportInstallTrace(traceId: string, outputPath: string): Promise<void> {
    return invoke<void>("export_install_trace", { traceId, outputPath });
  },

  async importInstallTrace(inputPath: string): Promise<InstallTrace> {
    return invoke<InstallTrace>("import_install_trace", { inputPath });
  },

  async listRegistryBackups(): Promise<BackupEntry[]> {
    if (!isTauriRuntime()) return [];
    return invoke<BackupEntry[]>("list_registry_backups");
  },

  async previewRegistryBackup(path: string): Promise<string> {
    return invoke<string>("preview_registry_backup", { path });
  },

  async restoreRegistryBackup(path: string): Promise<void> {
    return invoke<void>("restore_registry_backup", { path });
  },

  async deleteRegistryBackup(path: string): Promise<void> {
    return invoke<void>("delete_registry_backup", { path });
  },

  async listOperationLogs(limit = 200): Promise<OperationLogEntry[]> {
    if (!isTauriRuntime()) return [];
    return invoke<OperationLogEntry[]>("list_operation_logs", { limit });
  },

  async clearOperationLogs(): Promise<void> {
    return invoke<void>("clear_operation_logs");
  },

  async createRestorePoint(description: string): Promise<boolean> {
    return invoke<boolean>("create_restore_point", { description });
  },

  async openBackupsFolder(): Promise<void> {
    return invoke<void>("open_backups_folder");
  },

  async openPathInExplorer(path: string): Promise<void> {
    return invoke<void>("open_path_in_explorer", { path });
  },

  async openKeyInRegedit(keyPath: string): Promise<void> {
    return invoke<void>("open_key_in_regedit", { keyPath });
  },

  async openExternalUrl(url: string): Promise<void> {
    return invoke<void>("open_external_url", { url });
  },

  async exportApps(
    format: ExportFormat,
    outputPath: string,
    apps?: InstalledApp[],
  ): Promise<number> {
    return invoke<number>("export_apps", { format, outputPath, apps });
  },

  async getShellIntegrationStatus(): Promise<ShellIntegrationStatus> {
    if (!isTauriRuntime()) {
      return {
        forceRemoveEnabled: false,
        trackInstallEnabled: false,
        registeredExe: "",
      };
    }
    return invoke<ShellIntegrationStatus>("get_shell_integration_status");
  },

  async setShellIntegration(
    forceRemove: boolean,
    trackInstall: boolean,
    locale: string,
  ): Promise<ShellIntegrationStatus> {
    return invoke<ShellIntegrationStatus>("set_shell_integration", {
      forceRemove,
      trackInstall,
      locale,
    });
  },

  async checkForUpdates(): Promise<UpdateCheckResult> {
    if (!isTauriRuntime()) {
      const mode = getPreviewUpdateMode();
      if (mode === "uptodate") {
        previewUpdateCheck = {
          ...PREVIEW_MOCK_UPDATE,
          latestVersion: "0.5.1",
          latestTag: "v0.5.1",
          hasUpdate: false,
        };
      } else {
        previewUpdateCheck = { ...PREVIEW_MOCK_UPDATE };
      }
      return previewUpdateCheck;
    }
    return invoke<UpdateCheckResult>("check_for_updates");
  },

  async getUpdateState(): Promise<UpdateStateSnapshot> {
    if (!isTauriRuntime()) {
      const mode = getPreviewUpdateMode();
      if (mode === "available" && !previewUpdateCheck) {
        previewUpdateCheck = { ...PREVIEW_MOCK_UPDATE };
      } else if (mode === "ready" && !previewUpdateCheck) {
        previewUpdateCheck = { ...PREVIEW_MOCK_UPDATE };
        previewUpdateProgress = {
          stage: "ready",
          version: PREVIEW_MOCK_UPDATE.latestVersion,
          tag: PREVIEW_MOCK_UPDATE.latestTag ?? "v0.6.0",
          downloadedBytes: PREVIEW_MOCK_UPDATE.exeAssetSize ?? 7_340_032,
          totalBytes: PREVIEW_MOCK_UPDATE.exeAssetSize ?? 7_340_032,
          percent: 100,
          error: null,
          staged: {
            version: PREVIEW_MOCK_UPDATE.latestVersion,
            tag: PREVIEW_MOCK_UPDATE.latestTag ?? "v0.6.0",
            assetName: PREVIEW_MOCK_UPDATE.exeAssetName ?? "unbind-v0.6.0-x86_64-windows.exe",
            sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            fileSize: PREVIEW_MOCK_UPDATE.exeAssetSize ?? 7_340_032,
            stagedExePath: "C:\\Temp\\unbind-updater\\stage-0.6.0\\unbind.exe",
            stagingDir: "C:\\Temp\\unbind-updater\\stage-0.6.0",
          },
        };
      }
      return {
        latestCheck: previewUpdateCheck,
        progress: previewUpdateProgress,
      };
    }
    return invoke<UpdateStateSnapshot>("get_update_state");
  },

  async downloadUpdate(): Promise<UpdateProgressEvent> {
    if (!isTauriRuntime()) {
      const target = previewUpdateCheck ?? PREVIEW_MOCK_UPDATE;
      const total = target.exeAssetSize ?? 7_340_032;
      if (previewDownloadTimer !== null) {
        window.clearInterval(previewDownloadTimer);
      }
      let pct = 12;
      const startEv: UpdateProgressEvent = {
        stage: "downloading",
        version: target.latestVersion,
        tag: target.latestTag ?? `v${target.latestVersion}`,
        downloadedBytes: Math.round((total * pct) / 100),
        totalBytes: total,
        percent: pct,
        error: null,
        staged: null,
      };
      emitPreviewProgress(startEv);
      previewDownloadTimer = window.setInterval(() => {
        pct += 22;
        if (pct < 100) {
          emitPreviewProgress({
            stage: "downloading",
            version: target.latestVersion,
            tag: target.latestTag ?? `v${target.latestVersion}`,
            downloadedBytes: Math.round((total * pct) / 100),
            totalBytes: total,
            percent: pct,
            error: null,
            staged: null,
          });
        } else {
          if (previewDownloadTimer !== null) {
            window.clearInterval(previewDownloadTimer);
            previewDownloadTimer = null;
          }
          emitPreviewProgress({
            stage: "verifying",
            version: target.latestVersion,
            tag: target.latestTag ?? `v${target.latestVersion}`,
            downloadedBytes: total,
            totalBytes: total,
            percent: 100,
            error: null,
            staged: null,
          });
          window.setTimeout(() => {
            emitPreviewProgress({
              stage: "ready",
              version: target.latestVersion,
              tag: target.latestTag ?? `v${target.latestVersion}`,
              downloadedBytes: total,
              totalBytes: total,
              percent: 100,
              error: null,
              staged: {
                version: target.latestVersion,
                tag: target.latestTag ?? `v${target.latestVersion}`,
                assetName: target.exeAssetName ?? "unbind-v0.6.0-x86_64-windows.exe",
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
                fileSize: total,
                stagedExePath: "C:\\Temp\\unbind-updater\\stage-0.6.0\\unbind.exe",
                stagingDir: "C:\\Temp\\unbind-updater\\stage-0.6.0",
              },
            });
          }, 420);
        }
      }, 240);
      return startEv;
    }
    return invoke<UpdateProgressEvent>("download_update");
  },

  async cancelUpdateDownload(): Promise<boolean> {
    if (!isTauriRuntime()) {
      if (previewDownloadTimer !== null) {
        window.clearInterval(previewDownloadTimer);
        previewDownloadTimer = null;
      }
      emitPreviewProgress({ ...EMPTY_UPDATE_PROGRESS });
      return true;
    }
    return invoke<boolean>("cancel_update_download");
  },

  async installStagedUpdate(): Promise<void> {
    if (!isTauriRuntime()) {
      emitPreviewProgress({
        ...previewUpdateProgress,
        stage: "installing",
      });
      return;
    }
    return invoke<void>("install_staged_update");
  },

  async consumeUpdateOutcome(): Promise<PendingUpdateOutcome | null> {
    if (!isTauriRuntime()) return null;
    return invoke<PendingUpdateOutcome | null>("consume_update_outcome");
  },

  async subscribeUpdateProgress(
    handler: (event: UpdateProgressEvent) => void,
  ): Promise<UnlistenFn> {
    if (!isTauriRuntime()) {
      previewProgressListeners.add(handler);
      return () => {
        previewProgressListeners.delete(handler);
      };
    }
    return listen<UpdateProgressEvent>("update://progress", (e) => {
      if (e.payload) {
        handler(e.payload);
      }
    });
  },

  async takePendingCliAction(): Promise<CliAction | null> {
    if (!isTauriRuntime()) return null;
    return invoke<CliAction | null>("take_pending_cli_action");
  },

  async listPortableApps(roots: string[]): Promise<PortableApp[]> {
    if (!isTauriRuntime() || roots.length === 0) return [];
    return invoke<PortableApp[]>("list_portable_apps", { roots });
  },

  async getPortableAppIcons(appIds: string[]): Promise<Record<string, string>> {
    if (!isTauriRuntime() || appIds.length === 0) return {};
    return invoke<Record<string, string>>("get_portable_app_icons", { appIds });
  },

  async launchPortableApp(exePath: string, workingDir: string): Promise<void> {
    return invoke<void>("launch_portable_app", { exePath, workingDir });
  },

  async createPortableShortcut(
    exePath: string,
    name: string,
    location: ShortcutLocation,
  ): Promise<string> {
    return invoke<string>("create_portable_shortcut", { exePath, name, location });
  },

  async checkWingetUpdates(): Promise<WingetCheckResult> {
    if (!isTauriRuntime()) {
      return {
        available: false,
        wingetVersion: "",
        updates: [],
        error: "Unavailable in browser preview",
      };
    }
    return invoke<WingetCheckResult>("check_winget_updates");
  },

  async runWingetUpgrade(packageId: string, appName: string): Promise<WingetUpgradeResult> {
    return invoke<WingetUpgradeResult>("run_winget_upgrade", { packageId, appName });
  },

  async pickFile(filters?: Array<{ name: string; extensions: string[] }>): Promise<string | null> {
    if (!isTauriRuntime()) return null;
    const selected = await open({
      multiple: false,
      directory: false,
      filters,
    });
    return typeof selected === "string" ? selected : null;
  },

  async pickFolder(): Promise<string | null> {
    if (!isTauriRuntime()) return null;
    const selected = await open({
      multiple: false,
      directory: true,
    });
    return typeof selected === "string" ? selected : null;
  },

  async pickSavePath(defaultName: string, ext: string): Promise<string | null> {
    if (!isTauriRuntime()) return null;
    const selected = await save({
      defaultPath: defaultName,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    return selected ?? null;
  },

  windowControls: {
    minimize: () => (isTauriRuntime() ? getCurrentWindow().minimize() : Promise.resolve()),
    toggleMaximize: () =>
      isTauriRuntime() ? getCurrentWindow().toggleMaximize() : Promise.resolve(),
    close: () => (isTauriRuntime() ? getCurrentWindow().close() : Promise.resolve()),
    isMaximized: () =>
      isTauriRuntime() ? getCurrentWindow().isMaximized() : Promise.resolve(false),
    onResized: (handler: () => void) =>
      isTauriRuntime()
        ? getCurrentWindow().onResized(handler)
        : Promise.resolve(() => {}),
  },
};
