// The one IPC abstraction components see. Production entries use
// `createTauriBackend()` (src/ipc/tauriBackend.ts); the dev preview uses an
// in-memory mock (src/dev/mockBackend.ts). The backend owns all timing: these
// calls send intents and return the backend's fresh Snapshot.

import type { AppInfo } from "./bindings/AppInfo";
import type { OpenTabEvent } from "./bindings/OpenTabEvent";
import type { PackInfo } from "./bindings/PackInfo";
import type { PlayEvent } from "./bindings/PlayEvent";
import type { ResolvedPack } from "./bindings/ResolvedPack";
import type { SessionRecord } from "./bindings/SessionRecord";
import type { SettingsTab } from "./bindings/SettingsTab";
import type { Snapshot } from "./bindings/Snapshot";
import type { Transition } from "./bindings/Transition";
import type { UserSettingsPatch } from "./bindings/UserSettingsPatch";
import type { EventKey } from "./events";

export type BackendEvents = {
  snapshot: Snapshot;
  transition: Transition;
  packChanged: ResolvedPack;
  play: PlayEvent;
  openTab: OpenTabEvent;
  openPicker: null;
};

// Compile-time check: every name in EVENTS has a payload type here, and back.
type Assert<T extends true> = T;
type Same<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false;
export type EventsMatchPayloads = Assert<Same<EventKey, keyof BackendEvents>>;

export type BackendEventName = keyof BackendEvents;
export type Unlisten = () => void;

export interface StartFocusArgs {
  minutes?: number;
  taskLabel?: string;
}

export interface StartBreakArgs {
  minutes?: number;
}

/**
 * Commands per docs/SPEC.md §10. Each window's capability allows only the
 * commands that window uses; calling a command the window lacks rejects.
 * Rejections are `BackendError`s (see ./errors.ts).
 */
export interface Backend {
  getSnapshot(): Promise<Snapshot>;
  startFocus(args?: StartFocusArgs): Promise<Snapshot>;
  startBreak(args?: StartBreakArgs): Promise<Snapshot>;
  pause(): Promise<Snapshot>;
  resume(): Promise<Snapshot>;
  endSession(): Promise<Snapshot>;
  dismiss(): Promise<Snapshot>;
  /** Settings window only. */
  updateSettings(patch: UserSettingsPatch): Promise<Snapshot>;
  /** Settings window only. `null` disables the hotkey. */
  setHotkey(accelerator: string | null): Promise<Snapshot>;
  /** Settings window only. Unregisters the live shortcut while recording. */
  setHotkeyCapture(active: boolean): Promise<void>;
  /** Settings window only. Newest first. */
  getHistory(limit?: number): Promise<SessionRecord[]>;
  /** Settings window only. Rescans pack directories. */
  listPacks(): Promise<PackInfo[]>;
  getActivePack(): Promise<ResolvedPack>;
  /** Settings window only. */
  setActivePack(id: string): Promise<ResolvedPack>;
  replayAnimation(key: string): Promise<void>;
  /** Settings window only. */
  showCompanion(): Promise<void>;
  hideCompanion(): Promise<void>;
  /** Settings window only. */
  resetCompanionPosition(): Promise<void>;
  /** Companion only. */
  openSettings(tab?: SettingsTab): Promise<void>;
  getAppInfo(): Promise<AppInfo>;
  /** Settings window only. */
  quitApp(): Promise<void>;
  /** Subscribe to a backend event. Resolves once the listener is registered. */
  on<K extends BackendEventName>(event: K, cb: (payload: BackendEvents[K]) => void): Promise<Unlisten>;
}
