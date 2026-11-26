import { describe, expect, it } from "vitest";
import en from "./locales/en.json";
import zhCN from "./locales/zh-CN.json";
import { getDict, resolveLocale } from "./index";

function collectLeafKeys(obj: Record<string, unknown>, prefix = ""): string[] {
  const keys: string[] = [];
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) {
      keys.push(...collectLeafKeys(v as Record<string, unknown>, path));
    } else {
      keys.push(path);
    }
  }
  return keys.sort();
}

describe("bilingual i18n dictionaries", () => {
  it("keeps en.json and zh-CN.json key trees strictly identical", () => {
    const enKeys = collectLeafKeys(en as unknown as Record<string, unknown>);
    const zhKeys = collectLeafKeys(zhCN as unknown as Record<string, unknown>);
    expect(zhKeys).toEqual(enKeys);
  });

  it("provides complete non-empty bilingual tray strings", () => {
    const enDict = getDict("en");
    const zhDict = getDict("zh-CN");

    for (const key of Object.keys(enDict.tray) as Array<keyof typeof enDict.tray>) {
      expect(enDict.tray[key].trim().length).toBeGreaterThan(0);
      expect(zhDict.tray[key].trim().length).toBeGreaterThan(0);
    }
    expect(enDict.tray.openMain).not.toEqual(zhDict.tray.openMain);
    expect(enDict.tray.quit).not.toEqual(zhDict.tray.quit);
    expect(enDict.tray.tooltip).not.toEqual(zhDict.tray.tooltip);
  });

  it("resolves explicit locales cleanly", () => {
    expect(resolveLocale("en")).toBe("en");
    expect(resolveLocale("zh-CN")).toBe("zh-CN");
  });
});
