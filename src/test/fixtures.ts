// Test builders: plain data in the generated IPC shapes, plus a scriptable
// fake Backend. Tests never import the dev mock or @tauri-apps/api.

import { vi } from "vitest";
import type { Backend, BackendEventName, BackendEvents, Unlisten } from "../ipc/backend";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { ResolvedAnimation } from "../ipc/bindings/ResolvedAnimation";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { SessionView } from "../ipc/bindings/SessionView";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";
import { DEFAULT_LIMITS } from "../format/minutes";

export const UNLOCKS: UnlockDef[] = [
  { id: "base", points: 0, kind: "states", trigger: "always", label: "Old friends", description: "" },
  { id: "wave", points: 25, kind: "animation", trigger: "focusStart", label: "A small wave", description: "" },
  { id: "stretch", points: 60, kind: "animation", trigger: "breakStart", label: "Big stretch", description: "" },
  { id: "umbrella", points: 100, kind: "idleVariant", trigger: "rainyIdle", label: "Rainy day", description: "" },
  {
    id: "evening",
    points: 160,
    kind: "palette",
    trigger: "afterSunset",
    label: "Evening light",
    description: "",
    palette: { background: "#2c2a31", text: "#f3e9da", accent: "#e8a964", characterFilter: "brightness(0.9)" },
  },
  { id: "sleepy", points: 240, kind: "idleVariant", trigger: "lateNight", label: "Sleepy", description: "" },
  { id: "celebrate", points: 350, kind: "animation", trigger: "focusComplete", label: "Little celebration", description: "" },
  { id: "acorn_gift", points: 500, kind: "animation", trigger: "harvest", label: "Acorn gift", description: "" },
  {
    id: "autumn",
    points: 700,
    kind: "palette",
    trigger: "autumnSeason",
    label: "Autumn leaves",
    description: "",
    palette: { background: "#f6ede1", accent: "#a8502b" },
  },
  { id: "long_session", points: 1000, kind: "animation", trigger: "longSessionComplete", label: "Long haul", description: "" },
  { id: "dawn", points: 1450, kind: "palette", trigger: "earlyMorning", label: "Dawn", description: "", palette: { background: "#f8f1ee" } },
  { id: "firefly", points: 2100, kind: "idleVariant", trigger: "summerEvening", label: "Fireflies", description: "" },
  { id: "snow", points: 3050, kind: "palette", trigger: "winterSeason", label: "First snow", description: "", palette: { background: "#f1f4f6" } },
  { id: "hum", points: 4400, kind: "animation", trigger: "breakEnd", label: "A little tune", description: "" },
  { id: "moss", points: 6400, kind: "palette", trigger: "manual", label: "Deep moss", description: "", palette: { background: "#1e2a21" } },
  { id: "lantern", points: 9300, kind: "idleVariant", trigger: "evening", label: "Lantern", description: "" },
  { id: "bloom", points: 13500, kind: "palette", trigger: "springSeason", label: "Spring bloom", description: "", palette: { background: "#fbf3f4" } },
  { id: "stargaze", points: 19500, kind: "idleVariant", trigger: "lateNightClear", label: "Stargazing", description: "" },
];

export const ALL_IDS = UNLOCKS.map((u) => u.id);

export function anim(key: string, extra: Partial<ResolvedAnimation> = {}): ResolvedAnimation {
  return {
    key,
    sourceKey: key,
    url: `pack://localhost/test/${key}.svg`,
    frameWidth: 96,
    frameHeight: 96,
    frames: 4,
    fps: 4,
    loop: true,
    row: 0,
    column: 0,
    sheetColumns: 4,
    sheetRows: 1,
    stillFrame: 0,
    frameDurationsMs: null,
    ...extra,
  };
}

/** A pack whose `keys` resolve; every other render key is present and null. */
export function makePack(keys: string[], extra: Partial<ResolvedPack> = {}): ResolvedPack {
  const all = new Set(["idle", "focus", "paused", "completed", "break", "react", "blink", ...ALL_IDS, ...keys]);
  const animations: Record<string, ResolvedAnimation | null> = {};
  for (const k of all) animations[k] = keys.includes(k) ? anim(k) : null;
  return {
    info: {
      id: "test",
      name: "Test pack",
      author: null,
      license: "CC0-1.0",
      description: null,
      source: "bundled",
      formatKind: "native",
      valid: true,
      errors: [],
      warnings: [],
    },
    rendering: "smooth",
    animations,
    palettes: {},
    fallbackFrom: null,
    ...extra,
  };
}

export function session(extra: Partial<SessionView> = {}): SessionView {
  return {
    id: "s1",
    kind: "focus",
    status: "running",
    pauseReason: null,
    taskLabel: null,
    plannedMs: 1_500_000,
    activeMs: 47_000,
    remainingMs: 1_453_000,
    remainingLabel: "24:13",
    startedAt: "2026-11-04T09:00:00Z",
    ...extra,
  };
}

export function makeSnapshot(extra: Partial<Snapshot> = {}): Snapshot {
  return {
    seq: 1,
    revision: 1,
    phase: "idle",
    session: null,
    awaiting: null,
    today: {
      date: "2026-11-04",
      focusMs: 0,
      activeFocusMs: 0,
      completed: 0,
      interrupted: 0,
      harvest: [],
      focusLabel: "0 min",
    },
    friendship: { totalPoints: 0, unlocked: ["base"], next: { id: "wave", label: "A small wave", points: 25, remaining: 25 } },
    settings: {
      lastFocusMinutes: 25,
      sound: true,
      reducedMotion: false,
      hideDuringFocus: false,
      menuBarTitle: true,
      hotkey: "Ctrl+Alt+T",
      activePack: "placeholder",
      palette: "auto",
      widgetPosition: null,
    },
    companionVisible: true,
    ...extra,
  };
}

export const APP_INFO: AppInfo = {
  name: "Totoro Focus",
  version: "0.1.0",
  userPacksDir: "/packs",
  dataDir: "/data",
  windowMode: "panel",
  debug: false,
  hotkeyError: null,
  limits: DEFAULT_LIMITS,
  rules: { pointsPerActiveMinute: 1, completionBonus: 5, longSessionMinutes: 45 },
  unlocks: UNLOCKS,
};

/**
 * A Backend whose calls are vi.fn()s. `emit` pushes events to subscribers;
 * `calls` records the order of on()/getSnapshot() for ordering tests.
 */
export class FakeBackend implements Backend {
  calls: string[] = [];
  private subs = new Map<string, Set<(p: unknown) => void>>();
  snapshot: Snapshot = makeSnapshot();
  pack: ResolvedPack = makePack(["idle"]);

  getSnapshot = vi.fn(async () => {
    this.calls.push("getSnapshot");
    return this.snapshot;
  });
  startFocus = vi.fn(async () => this.snapshot);
  startBreak = vi.fn(async () => this.snapshot);
  pause = vi.fn(async () => this.snapshot);
  resume = vi.fn(async () => this.snapshot);
  endSession = vi.fn(async () => this.snapshot);
  dismiss = vi.fn(async () => this.snapshot);
  updateSettings = vi.fn(async () => this.snapshot);
  setHotkey = vi.fn(async () => this.snapshot);
  setHotkeyCapture = vi.fn(async () => {});
  getHistory = vi.fn(async () => []);
  listPacks = vi.fn(async () => [this.pack.info]);
  getActivePack = vi.fn(async () => {
    this.calls.push("getActivePack");
    return this.pack;
  });
  setActivePack = vi.fn(async () => this.pack);
  replayAnimation = vi.fn(async () => {});
  showCompanion = vi.fn(async () => {});
  hideCompanion = vi.fn(async () => {});
  resetCompanionPosition = vi.fn(async () => {});
  openSettings = vi.fn(async () => {});
  getAppInfo = vi.fn(async () => APP_INFO);
  quitApp = vi.fn(async () => {});

  on<K extends BackendEventName>(event: K, cb: (payload: BackendEvents[K]) => void): Promise<Unlisten> {
    this.calls.push(`on:${event}`);
    let set = this.subs.get(event);
    if (!set) {
      set = new Set();
      this.subs.set(event, set);
    }
    const fn = cb as (p: unknown) => void;
    set.add(fn);
    return Promise.resolve(() => {
      set.delete(fn);
    });
  }

  emit<K extends BackendEventName>(event: K, payload: BackendEvents[K]): void {
    for (const cb of [...(this.subs.get(event) ?? [])]) cb(payload);
  }

  listenerCount(event: BackendEventName): number {
    return this.subs.get(event)?.size ?? 0;
  }
}

/** Lets pending promise callbacks run. */
export const flush = () => new Promise<void>((r) => setTimeout(r, 0));
