// Event names exchanged with the shell. Mirrors `focus_core::events`
// (crates/focus-core/src/events.rs); keep both in sync.

export const EVENTS = {
  /** Snapshot, after every state change and on display ticks. */
  snapshot: "focus://snapshot",
  /** Transition, for presentation flourishes only. */
  transition: "focus://transition",
  /** ResolvedPack, after `set_active_pack` or a pack rescan. */
  packChanged: "focus://pack-changed",
  /** PlayEvent{key}, a validated replay; sent to the companion only. */
  play: "focus://play",
  /** OpenTabEvent{tab}, only when the settings window already exists. */
  openTab: "focus://open-tab",
  /** No payload; the tray's "Custom duration…" item. */
  openPicker: "focus://open-picker",
} as const;

export type EventKey = keyof typeof EVENTS;

/** Window labels (must match src-tauri/capabilities/*.json). */
export const WINDOW_LABELS = {
  companion: "companion",
  settings: "settings",
} as const;
