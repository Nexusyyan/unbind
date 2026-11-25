import { describe, expect, it } from "vitest";
import { filterAndSortApps, formatBytes, formatSizeKb, isRecentDate } from "./format";
import type { InstalledApp } from "../types";

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

  it("filters and sorts apps correctly", () => {
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
  });
});
