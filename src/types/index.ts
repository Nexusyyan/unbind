export type AppKind = "win32" | "msi" | "storeApp";
export type InstallScope = "machine" | "user";
export type Arch = "x64" | "x86" | "arm64" | "neutral";
export type PackageManagerKind = "none" | "winget" | "scoop" | "chocolatey";

export interface InstalledApp {
  id: string;
  name: string;
  publisher: string;
  version: string;
  installDate: string;
  estimatedSizeKb: number;
  installLocation: string;
  uninstallString: string;
  quietUninstallString: string;
  displayIcon: string;
  urlInfoAbout: string;
  registryKey: string;
  registryHive: string;
  registryView: string;
  productCode: string;
  packageFullName: string;
  packageFamilyName: string;
  kind: AppKind;
  scope: InstallScope;
  arch: Arch;
  isSystemComponent: boolean;
  isWindowsUpdate: boolean;
  isValidEntry: boolean;
  hasUninstaller: boolean;
  trackedTraceId: string | null;
  packageManager?: PackageManagerKind;
  sizeMeasured?: boolean;
}

export type LeftoverKind =
  | "registryKey"
  | "registryValue"
  | "directory"
  | "file"
  | "shortcut"
  | "scheduledTask"
  | "service";

export type ConfidenceLevel = "high" | "medium" | "protected";

export interface LeftoverItem {
  id: string;
  kind: LeftoverKind;
  path: string;
  valueName: string | null;
  sizeBytes: number;
  confidence: ConfidenceLevel;
  reason: string;
  reasonCode?: string;
  reasonParam?: string | null;
  selectedByDefault: boolean;
  requiresElevation: boolean;
}

export interface CleanupOptions {
  useRecycleBin: boolean;
  backupRegistry: boolean;
  createRestorePoint: boolean;
  scheduleRebootOnLock: boolean;
  terminateLockingProcesses: boolean;
}

export interface CleanupItemResult {
  itemId: string;
  path: string;
  success: boolean;
  scheduledForReboot: boolean;
  error: string | null;
}

export interface CleanupReport {
  succeeded: number;
  failed: number;
  scheduledForReboot: number;
  freedBytes: number;
  regBackupPath: string | null;
  restorePointCreated: boolean;
  details: CleanupItemResult[];
}

export interface UninstallExecutionResult {
  launched: boolean;
  exitCode: number | null;
  timedOut: boolean;
  message: string;
}

export interface LockingProcessInfo {
  pid: number;
  name: string;
  serviceShortName: string;
  appType: string;
  restartable: boolean;
}

export interface RunningProcessInfo {
  pid: number;
  name: string;
  exePath: string;
}

export type StartupSource =
  | "registryCurrentUserRun"
  | "registryCurrentUserRunOnce"
  | "registryLocalMachineRun"
  | "registryLocalMachineWow64Run"
  | "startupFolderUser"
  | "startupFolderCommon"
  | "scheduledTask";

export interface StartupItem {
  id: string;
  name: string;
  command: string;
  executablePath: string;
  publisher: string;
  source: StartupSource;
  location: string;
  enabled: boolean;
  isValidTarget: boolean;
  requiresElevation: boolean;
}

export type TrackMode = "hybridSnapshot" | "etwRealtime";

export interface EtwSessionStats {
  sessionId: string;
  etwActive: boolean;
  trackedPidCount: number;
  capturedFileCount: number;
  capturedRegCount: number;
  totalEvents: number;
}

export interface TrackedRegValue {
  keyPath: string;
  valueName: string;
}

export interface InstallTrace {
  id: string;
  appName: string;
  installerPath: string;
  startedAt: string;
  completedAt: string;
  addedDirs: string[];
  addedFiles: string[];
  addedRegistryKeys: string[];
  addedRegistryValues: TrackedRegValue[];
  addedServices?: string[];
  addedScheduledTasks?: string[];
  watchedRoots?: string[];
  totalSizeBytes: number;
  trackMode?: TrackMode;
  etwEventCount?: number;
}

export interface ActiveTrackSession {
  sessionId: string;
  appName: string;
  installerPath: string;
  startedAt: string;
  baselineDirCount: number;
  baselineFileCount: number;
  baselineRegCount: number;
  watchedRoots?: string[];
  installerPid?: number | null;
  installerExited?: boolean;
  trackMode?: TrackMode;
  etwActive?: boolean;
  trackedPidCount?: number;
  capturedFileCount?: number;
  capturedRegCount?: number;
}

export interface PortableApp {
  id: string;
  name: string;
  publisher: string;
  version: string;
  exePath: string;
  installDir: string;
  rootDir: string;
  sizeBytes: number;
  lastModified: string;
  isSingleFile: boolean;
}

export type ShortcutLocation = "desktop" | "startMenu";

export interface WingetUpdateInfo {
  name: string;
  packageId: string;
  currentVersion: string;
  availableVersion: string;
  source: string;
  matchedAppId: string | null;
}

export interface WingetCheckResult {
  available: boolean;
  wingetVersion: string;
  updates: WingetUpdateInfo[];
  error: string | null;
}

export interface WingetUpgradeResult {
  packageId: string;
  appName: string;
  success: boolean;
  exitCode: number | null;
  message: string;
}

export interface BackupEntry {
  id: string;
  filename: string;
  path: string;
  createdAt: string;
  sizeBytes: number;
  appName: string;
}

export interface OperationLogEntry {
  timestamp: string;
  action: string;
  target: string;
  summary: string;
  success: boolean;
}

export interface WindowBounds {
  width: number;
  height: number;
  x: number;
  y: number;
  maximized: boolean;
}

export interface AppSettings {
  portableMode: boolean;
  dataDir: string;
  useRecycleBin: boolean;
  autoBackupRegistry: boolean;
  createRestorePoint: boolean;
  showSystemComponents: boolean;
  showWindowsUpdates: boolean;
  language: string;
  theme: string;
  customTrackRoots?: string[];
  portableAppRoots?: string[];
  defaultTrackMode?: TrackMode;
  closeToTray?: boolean;
  windowBounds?: WindowBounds | null;
}

export interface TraySnapshot {
  appVersion: string;
  isElevated: boolean;
  workerConnected: boolean;
  portableMode: boolean;
  mainWindowVisible: boolean;
  installedCount: number;
  activeTrackCount: number;
  language: string;
  resolvedLocale: string;
  theme: string;
  closeToTray: boolean;
}

export interface TrayNavigatePayload {
  target: "apps" | "tools" | "tracker" | "settings" | "forceRemove";
}

export interface SystemStatus {
  isElevated: boolean;
  workerConnected: boolean;
  portableMode: boolean;
  dataDir: string;
  windowsVersion: string;
  appVersion: string;
}

export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string;
  hasUpdate: boolean;
  releaseUrl: string;
  publishedAt: string;
  notesPreview: string;
}

export interface ShellIntegrationStatus {
  forceRemoveEnabled: boolean;
  trackInstallEnabled: boolean;
  registeredExe: string;
}

export interface CliAction {
  action: "forceRemove" | "trackInstall";
  path: string;
}

export interface InstallerExitedPayload {
  sessionId: string;
  exitCode: number | null;
}

export type ExportFormat = "html" | "csv" | "json";

export type NavTab =
  | "apps"
  | "store"
  | "tools"
  | "tracker"
  | "backups"
  | "settings";

export type ToolsSubTab = "startup" | "portable";

export type AppFilterMode =
  | "all"
  | "updatable"
  | "recent"
  | "large"
  | "broken"
  | "system";

export type AppSortField = "name" | "size" | "date" | "publisher";

export interface AppListViewState {
  query: string;
  filterMode: AppFilterMode;
  sortField: AppSortField;
  sortDesc: boolean;
}

export interface ToastMessage {
  id: string;
  kind: "info" | "success" | "warning" | "error";
  title: string;
  detail?: string;
}
