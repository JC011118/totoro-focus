import type { SettingsTab } from "../ipc/bindings/SettingsTab";

export const TABS: readonly { id: SettingsTab; label: string }[] = [
  { id: "start", label: "Start" },
  { id: "general", label: "General" },
  { id: "history", label: "History" },
  { id: "companion", label: "Companion" },
];

const IDS = new Set<string>(TABS.map((t) => t.id));

/** `settings.html#<tab>` (SPEC §8.2); tolerates "#/start" and "#settings/start". Default: general. */
export function tabFromHash(hash: string): SettingsTab {
  const last = hash.replace(/^#/, "").split(/[/?&]/).filter(Boolean).pop() ?? "";
  return IDS.has(last) ? (last as SettingsTab) : "general";
}
