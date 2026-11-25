import { describe, expect, it } from "vitest";
import { getDict } from "../i18n";
import {
  computeFilterCounts,
  filterAndSortApps,
  formatBytes,
  formatSizeKb,
  isRecentDate,
} from "./format";
import { formatLeftoverReason } from "./leftoverReason";
import type { InstalledApp, LeftoverItem } from "../types";

const sampleApps: InstalledApp[] = [
  {
    id: "1",
    name: "7-Zip 24.08",
    publisher: "Igor Pavlov",
    version: "24.08",
    installDate: "2026-11-20",
    estimatedSizeKb: 5120,
    installLocation: "C:\\Program Files\\7-Zip",
    uninstallString: "C:\\Program Files\\7-Zip\\Uninstall.exe",
    quietUninstallString: "\"C:\\Program Files\\7-Zip\\Uninstall.exe\" /S",
    displayIcon: "",
    urlInfoAbout: "",
    registryKey: "HKLM\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\7-Zip",
    registryHive: "HKLM",
    registryView: "64",
    productCode: "",
    packageFullName: "",
    packageFamilyName: "",
    kind: "win32",
    scope: "machine",
    arch: "x64",
    isSystemComponent: false,
    isWindowsUpdate: false,
    isValidEntry: true,
    hasUninstaller: true,
    trackedTraceId: null,
    packageManager: "scoop",
    sizeMeasured: false,
  },
  {
    id: "2",
    name: "Heavy Game Engine",
    publisher: "Studio X",
    version: "5.4",
    installDate: "2025-01-10",
    estimatedSizeKb: 250 * 1024,
    installLocation: "C:\\Program Files\\Heavy",
    uninstallString: "",
    quietUninstallString: "",
    displayIcon: "",
    urlInfoAbout: "",
    registryKey: "",
    registryHive: "HKLM",
    registryView: "64",
    productCode: "",
    packageFullName: "",
    packageFamilyName: "",
    kind: "win32",
    scope: "machine",
    arch: "x64",
    isSystemComponent: false,
    isWindowsUpdate: false,
    isValidEntry: false,
    hasUninstaller: false,
    trackedTraceId: null,
    packageManager: "none",
    sizeMeasured: true,
  },
];

describe("format utilities", () => {
  it("formats bytes and kilobytes accurately", () => {
    expect(formatBytes(0)).toBe("-");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatSizeKb(1024)).toBe("1.0 MB");
    expect(formatSizeKb(1024 * 1024 * 2)).toBe("2.0 GB");
  });

  it("detects recent dates within window", () => {
    const ref = Date.parse("2026-11-24");
    expect(isRecentDate("2026-11-20", 30, ref)).toBe(true);
    expect(isRecentDate("2025-01-10", 30, ref)).toBe(false);
  });

  it("filters and sorts apps with multi-token query and counts", () => {
    const large = filterAndSortApps(sampleApps, {
      storeOnly: false,
      query: "",
      filterMode: "large",
      sortField: "size",
      sortDesc: true,
      showSystemComponents: false,
      showWindowsUpdates: false,
    });
    expect(large).toHaveLength(1);
    expect(large[0].id).toBe("2");

    const broken = filterAndSortApps(sampleApps, {
      storeOnly: false,
      query: "",
      filterMode: "broken",
      sortField: "name",
      sortDesc: false,
      showSystemComponents: false,
      showWindowsUpdates: false,
    });
    expect(broken).toHaveLength(1);
    expect(broken[0].name).toBe("Heavy Game Engine");

    const multiToken = filterAndSortApps(sampleApps, {
      storeOnly: false,
      query: "igor scoop",
      filterMode: "all",
      sortField: "name",
      sortDesc: false,
      showSystemComponents: false,
      showWindowsUpdates: false,
    });
    expect(multiToken).toHaveLength(1);
    expect(multiToken[0].id).toBe("1");

    const counts = computeFilterCounts(sampleApps, {
      storeOnly: false,
      query: "",
      showSystemComponents: false,
      showWindowsUpdates: false,
    });
    expect(counts.all).toBe(2);
    expect(counts.large).toBe(1);
    expect(counts.broken).toBe(1);
  });

  it("localizes structured leftover reason codes in both locales", () => {
    const en = getDict("en");
    const zh = getDict("zh-CN");
    const sample: LeftoverItem = {
      id: "l1",
      kind: "service",
      path: "HKLM\\SYSTEM\\CurrentControlSet\\Services\\DemoSvc",
      valueName: null,
      sizeBytes: 0,
      confidence: "medium",
      reason: "Fallback reason",
      reasonCode: "service_ref",
      reasonParam: "DemoSvc",
      selectedByDefault: false,
      requiresElevation: true,
    };
    expect(formatLeftoverReason(sample, en)).toContain("DemoSvc");
    expect(formatLeftoverReason(sample, zh)).toContain("DemoSvc");
    expect(formatLeftoverReason(sample, zh)).not.toBe("Fallback reason");
  });
});
