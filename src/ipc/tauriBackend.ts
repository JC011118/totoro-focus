// The production Backend: the only module that imports @tauri-apps/api.
// Command names and argument keys follow src-tauri/src/commands.rs (Tauri maps
// snake_case Rust parameters to camelCase keys, e.g. `taskLabel`).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Backend, BackendEventName, BackendEvents, StartBreakArgs, StartFocusArgs } from "./backend";
import type { AppInfo } from "./bindings/AppInfo";
import type { PackInfo } from "./bindings/PackInfo";
import type { ResolvedPack } from "./bindings/ResolvedPack";
import type { SessionRecord } from "./bindings/SessionRecord";
import type { SettingsTab } from "./bindings/SettingsTab";
import type { Snapshot } from "./bindings/Snapshot";
import type { UserSettingsPatch } from "./bindings/UserSettingsPatch";
import { toBackendError } from "./errors";
import { EVENTS } from "./events";

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toBackendError(e);
  }
}

/** Drops undefined keys so absent options reach Rust as `None`. */
function defined(args: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(args).filter(([, v]) => v !== undefined));
}

export function createTauriBackend(): Backend {
  return {
    getSnapshot: () => call<Snapshot>("get_snapshot"),
    startFocus: (a: StartFocusArgs = {}) =>
      call<Snapshot>("start_focus", defined({ minutes: a.minutes, taskLabel: a.taskLabel })),
    startBreak: (a: StartBreakArgs = {}) => call<Snapshot>("start_break", defined({ minutes: a.minutes })),
    pause: () => call<Snapshot>("pause"),
    resume: () => call<Snapshot>("resume"),
    endSession: () => call<Snapshot>("end_session"),
    dismiss: () => call<Snapshot>("dismiss"),
    updateSettings: (patch: UserSettingsPatch) => call<Snapshot>("update_settings", { patch }),
    setHotkey: (accelerator: string | null) => call<Snapshot>("set_hotkey", { accelerator }),
    setHotkeyCapture: (active: boolean) => call<void>("set_hotkey_capture", { active }),
    getHistory: (limit?: number) => call<SessionRecord[]>("get_history", defined({ limit })),
    listPacks: () => call<PackInfo[]>("list_packs"),
    getActivePack: () => call<ResolvedPack>("get_active_pack"),
    setActivePack: (id: string) => call<ResolvedPack>("set_active_pack", { id }),
    replayAnimation: (key: string) => call<void>("replay_animation", { key }),
    showCompanion: () => call<void>("show_companion"),
    hideCompanion: () => call<void>("hide_companion"),
    resetCompanionPosition: () => call<void>("reset_companion_position"),
    openSettings: (tab?: SettingsTab) => call<void>("open_settings", defined({ tab })),
    getAppInfo: () => call<AppInfo>("get_app_info"),
    quitApp: () => call<void>("quit_app"),
    on: <K extends BackendEventName>(event: K, cb: (payload: BackendEvents[K]) => void) =>
      // Global listener (target Any). Each window subscribes only to the events
      // it renders, so events aimed at the other window are never acted on.
      listen<BackendEvents[K]>(EVENTS[event], (e) => cb(e.payload)),
  };
}
