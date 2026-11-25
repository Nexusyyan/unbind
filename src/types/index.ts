export type AppKind = "win32" | "msi" | "storeApp";
export type InstallScope = "machine" | "user";
export type Arch = "x64" | "x86" | "arm64" | "neutral";

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
  totalSizeBytes: number;
}

export interface ActiveTrackSession {
  sessionId: string;
  appName: string;
  installerPath: string;
  startedAt: string;
  baselineDirCount: number;
  baselineFileCount: number;
  baselineRegCount: number;
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
}

export interface SystemStatus {
  isElevated: boolean;
  workerConnected: boolean;
  portableMode: boolean;
  dataDir: string;
  windowsVersion: string;
  appVersion: string;
}

export type ExportFormat = "html" | "csv" | "json";

export type NavTab =
  | "apps"
  | "store"
  | "startup"
  | "tracker"
  | "backups"
  | "settings";

export type AppFilterMode =
  | "all"
  | "recent"
  | "large"
  | "broken"
  | "system";

export type AppSortField = "name" | "size" | "date" | "publisher";

export interface ToastMessage {
  id: string;
  kind: "info" | "success" | "warning" | "error";
  title: string;
  detail?: string;
}
