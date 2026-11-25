import en from "./locales/en.json";
import zhCN from "./locales/zh-CN.json";

export type Locale = "en" | "zh-CN";
export type Dict = typeof en;

const DICTS: Record<Locale, Dict> = {
  en,
  "zh-CN": zhCN,
};

export function resolveLocale(setting: string): Locale {
  if (setting === "en" || setting === "zh-CN") {
    return setting;
  }
  const nav = typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "en";
  return nav.startsWith("zh") ? "zh-CN" : "en";
}

export function getDict(locale: Locale): Dict {
  return DICTS[locale] ?? en;
}
