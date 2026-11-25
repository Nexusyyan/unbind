import type { Dict } from "../i18n";
import type { LeftoverItem } from "../types";

/**
 * Resolve a localized human-readable explanation for a scanned LeftoverItem
 * using its structured `reasonCode` + optional `reasonParam`, with safe fallback
 * to the raw `reason` string.
 */
export function formatLeftoverReason(item: LeftoverItem, dict: Dict): string {
  const code = item.reasonCode;
  if (!code) return item.reason;

  const map = dict.leftoverReasons as Record<string, string>;
  const template = map[code];
  if (!template) return item.reason;

  const param = item.reasonParam ?? "";
  return template.replace("{param}", param);
}
