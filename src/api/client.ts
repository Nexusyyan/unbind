import { invoke } from "@tauri-apps/api/core";
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
  WingetCheckResult,
  WingetUpgradeResult,
} from "../types";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
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
        appVersion: "0.5.0",
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
        appVersion: "0.5.0",
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
    return invoke<UpdateCheckResult>("check_for_updates");
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
