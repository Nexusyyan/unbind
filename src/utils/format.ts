import type { AppFilterMode, AppSortField, InstalledApp } from "../types";

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return "-";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let idx = 0;
  while (value >= 1024 && idx < units.length - 1) {
    value /= 1024;
    idx++;
  }
  return idx === 0
    ? `${Math.round(value)} B`
    : `${value.toFixed(value >= 100 ? 0 : 1)} ${units[idx]}`;
}

export function formatSizeKb(kb: number): string {
  if (!kb || kb <= 0) return "-";
  return formatBytes(kb * 1024);
}

export function isRecentDate(isoDate: string, days = 30, nowMs = Date.now()): boolean {
  if (!isoDate || isoDate.length < 10) return false;
  const parsed = Date.parse(isoDate.slice(0, 10));
  if (Number.isNaN(parsed)) return false;
  const diffDays = (nowMs - parsed) / (1000 * 60 * 60 * 24);
  return diffDays >= -1 && diffDays <= days;
}

export function formatDateTime(iso: string): string {
  if (!iso) return "-";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function matchesMultiTokenQuery(app: InstalledApp, tokens: string[]): boolean {
  if (tokens.length === 0) return true;
  const pkgTag = app.packageManager && app.packageManager !== "none" ? app.packageManager : "";
  const hay = `${app.name} ${app.publisher} ${app.version} ${app.installLocation} ${app.packageFamilyName} ${pkgTag}`.toLowerCase();
  return tokens.every((tok) => hay.includes(tok));
}

export function computeFilterCounts(
  apps: InstalledApp[],
  options: {
    storeOnly: boolean;
    query: string;
    showSystemComponents: boolean;
    showWindowsUpdates: boolean;
    updatableAppIds?: Set<string>;
  },
): Record<AppFilterMode, number> {
  const tokens = options.query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  const counts: Record<AppFilterMode, number> = {
    all: 0,
    updatable: 0,
    recent: 0,
    large: 0,
    broken: 0,
    system: 0,
  };

  for (const app of apps) {
    if (options.storeOnly ? app.kind !== "storeApp" : app.kind === "storeApp") {
      continue;
    }
    if (!matchesMultiTokenQuery(app, tokens)) {
      continue;
    }

    if (app.isSystemComponent || app.isWindowsUpdate) {
      counts.system++;
    }

    const visibleInStandard =
      (options.showSystemComponents || !app.isSystemComponent) &&
      (options.showWindowsUpdates || !app.isWindowsUpdate);

    if (visibleInStandard) {
      counts.all++;
      if (options.updatableAppIds?.has(app.id)) counts.updatable++;
      if (isRecentDate(app.installDate, 30)) counts.recent++;
      if (app.estimatedSizeKb >= 100 * 1024) counts.large++;
      if (!app.isValidEntry) counts.broken++;
    }
  }

  return counts;
}

export function filterAndSortApps(
  apps: InstalledApp[],
  options: {
    storeOnly: boolean;
    query: string;
    filterMode: AppFilterMode;
    sortField: AppSortField;
    sortDesc: boolean;
    showSystemComponents: boolean;
    showWindowsUpdates: boolean;
    updatableAppIds?: Set<string>;
  },
): InstalledApp[] {
  const tokens = options.query
    .trim()
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean);

  const filtered = apps.filter((app) => {
    if (options.storeOnly ? app.kind !== "storeApp" : app.kind === "storeApp") {
      return false;
    }

    if (options.filterMode === "system") {
      if (!app.isSystemComponent && !app.isWindowsUpdate) return false;
    } else {
      if (!options.showSystemComponents && app.isSystemComponent) return false;
      if (!options.showWindowsUpdates && app.isWindowsUpdate) return false;
    }

    if (options.filterMode === "updatable" && !options.updatableAppIds?.has(app.id)) {
      return false;
    }
    if (options.filterMode === "recent" && !isRecentDate(app.installDate, 30)) {
      return false;
    }
    if (options.filterMode === "large" && app.estimatedSizeKb < 100 * 1024) {
      return false;
    }
    if (options.filterMode === "broken" && app.isValidEntry) {
      return false;
    }

    return matchesMultiTokenQuery(app, tokens);
  });

  const dir = options.sortDesc ? -1 : 1;
  return [...filtered].sort((a, b) => {
    switch (options.sortField) {
      case "size":
        return (a.estimatedSizeKb - b.estimatedSizeKb) * dir || a.name.localeCompare(b.name);
      case "date":
        return a.installDate.localeCompare(b.installDate) * dir || a.name.localeCompare(b.name);
      case "publisher":
        return (
          (a.publisher || "zzz").toLowerCase().localeCompare((b.publisher || "zzz").toLowerCase()) *
            dir || a.name.localeCompare(b.name)
        );
      case "name":
      default:
        return a.name.toLowerCase().localeCompare(b.name.toLowerCase()) * dir;
    }
  });
}
