// In-memory Backend for the dev preview (dev.html only; never built). It
// approximates the engine closely enough for UI work: phases, transitions,
// awards, harvest, seq/revision. A fake countdown is allowed only here in
// src/dev. Its sentinel string makes scripts/assert-no-mock.mjs fail the build
// if this module ever reaches dist/.

import type { Backend, BackendEventName, BackendEvents, StartBreakArgs, StartFocusArgs, Unlisten } from "../ipc/backend";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { AwaitingView } from "../ipc/bindings/AwaitingView";
import type { AwardOutcome } from "../ipc/bindings/AwardOutcome";
import type { CommandError } from "../ipc/bindings/CommandError";
import type { PackInfo } from "../ipc/bindings/PackInfo";
import type { PauseReason } from "../ipc/bindings/PauseReason";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { SessionKind } from "../ipc/bindings/SessionKind";
import type { SessionRecord } from "../ipc/bindings/SessionRecord";
import type { Settings } from "../ipc/bindings/Settings";
import type { SettingsTab } from "../ipc/bindings/SettingsTab";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { Transition } from "../ipc/bindings/Transition";
import type { UserSettingsPatch } from "../ipc/bindings/UserSettingsPatch";
import { DEV_LIMITS, DEV_RULES, DEV_UNLOCKS } from "./devData";
import {
  BROKEN_PACK_INFO,
  FIXTURE_PACK,
  MISSING_ART_PACK,
  PETDEX_PACK_INFO,
  resolvePack,
  type DevPack,
} from "./devPacks";

/** Production builds must never contain this string (scripts/assert-no-mock.mjs). */
export const __FOCUS_DEV_MOCK__ = "__FOCUS_DEV_MOCK__";

const MIN = 60_000;

interface MockSession {
  id: string;
  kind: SessionKind;
  taskLabel: string | null;
  plannedMs: number;
  accruedMs: number;
  /** Virtual time when the current run started, or null when paused. */
  since: number | null;
  pauseReason: PauseReason | null;
  startedAt: number;
}

export type DevPreset =
  | "fresh"
  | "idle"
  | "running"
  | "paused"
  | "break"
  | "awaitingFocus"
  | "awaitingBreak"
  | "manyHarvest"
  | "highPoints"
  | "long";

const pad = (n: number) => String(n).padStart(2, "0");
const reject = (e: CommandError) => Promise.reject(e);

function formatCountdown(ms: number): string {
  const s = Math.ceil(Math.max(0, ms) / 1000);
  return `${pad(Math.floor(s / 60))}:${pad(s % 60)}`;
}

function formatFocusLabel(ms: number): string {
  const m = Math.floor(ms / MIN);
  return m >= 60 ? `${Math.floor(m / 60)} h ${pad(m % 60)} min` : `${m} min`;
}

function dateKey(t: number): string {
  const d = new Date(t);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export class MockBackend implements Backend {
  /** Referenced by every instance, so any use of the mock carries the sentinel. */
  readonly kind = __FOCUS_DEV_MOCK__;
  private listeners = new Map<BackendEventName, Set<(p: never) => void>>();
  private seq = 0;
  private revision = 0;
  private ids = 0;
  private virtual = Date.now();
  private lastReal = Date.now();
  private speedFactor = 1;
  private lastLabel = "";
  private ticker: ReturnType<typeof setInterval> | null = null;

  private settings: Settings = {
    lastFocusMinutes: 25,
    sound: true,
    reducedMotion: false,
    hideDuringFocus: false,
    menuBarTitle: true,
    hotkey: "Ctrl+Alt+T",
    activePack: "fixtures",
    palette: "auto",
    widgetPosition: null,
  };
  private history: SessionRecord[] = [];
  private totalPoints = 0;
  private unlocked = new Set<string>(["base"]);
  private awarded = new Set<string>();
  private session: MockSession | null = null;
  private awaiting: AwaitingView | null = null;
  private companionVisible = true;
  private packs: DevPack[] = [FIXTURE_PACK, MISSING_ART_PACK];
  private capture = false;

  constructor() {
    this.ticker = setInterval(() => this.tick(), 200);
    this.preset("idle", false);
  }

  dispose(): void {
    if (this.ticker) clearInterval(this.ticker);
    this.ticker = null;
  }

  // ---------- dev controls ----------

  get speed(): number {
    return this.speedFactor;
  }

  setSpeed(factor: number): void {
    this.now();
    this.speedFactor = factor;
  }

  addPack(pack: DevPack, activate = false): void {
    this.packs = [pack, ...this.packs.filter((p) => p.info.id !== pack.info.id)];
    if (activate) void this.setActivePack(pack.info.id);
  }

  get hotkeyCaptureActive(): boolean {
    return this.capture;
  }

  get currentSettings(): Settings {
    return this.settings;
  }

  get points(): number {
    return this.totalPoints;
  }

  /** Jump to a named state for UI work. */
  preset(name: DevPreset, emit = true): void {
    this.now();
    const today = this.virtual;
    const rec = (minutes: number, completed: boolean, label?: string, ago = 0): SessionRecord => {
      const end = today - ago * MIN;
      const active = completed ? minutes * MIN : Math.round(minutes * 0.6) * MIN + 17_000;
      return {
        id: this.nextId(),
        kind: "focus",
        ...(label ? { taskLabel: label } : {}),
        startedAt: new Date(end - active - 2 * MIN).toISOString(),
        endedAt: new Date(end).toISOString(),
        plannedMs: minutes * MIN,
        activeMs: active,
        outcome: completed ? "completed" : "interrupted",
        endReason: completed ? "completed" : "userEnded",
        endedUtcOffsetMinutes: -new Date(end).getTimezoneOffset(),
      };
    };
    const older = (days: number, minutes: number, completed: boolean, label?: string) =>
      rec(minutes, completed, label, days * 24 * 60 + 90);

    this.session = null;
    this.awaiting = null;
    if (name !== "highPoints" && name !== "manyHarvest") {
      this.setPoints(140);
    }
    this.history = [
      older(2, 25, true, "Read chapter 3"),
      older(1, 45, true, "Problem set"),
      older(1, 25, false),
      rec(25, true, "Review one lecture", 200),
      rec(15, false, undefined, 120),
      rec(25, true, "Outline the essay", 60),
    ];

    switch (name) {
      case "fresh":
        this.history = [];
        this.setPoints(0);
        break;
      case "idle":
        break;
      case "running":
        this.session = this.makeSession("focus", 25, "Review one lecture");
        this.session.accruedMs = 25 * MIN - (24 * 60 + 13) * 1000;
        break;
      case "paused":
        this.session = this.makeSession("focus", 25, "Review one lecture");
        this.session.accruedMs = 25 * MIN - (11 * 60 + 40) * 1000;
        this.session.since = null;
        this.session.pauseReason = "suspend";
        break;
      case "break":
        this.session = this.makeSession("break", 5, null);
        this.session.accruedMs = 5 * MIN - (3 * 60 + 42) * 1000;
        break;
      case "awaitingFocus": {
        const r = rec(25, true, "Review one lecture");
        this.history.push(r);
        this.setPoints(Math.max(this.totalPoints, 20));
        const award = this.award(r.id, r.activeMs, true);
        this.awaiting = { after: "focus", record: r, award, longSession: false };
        if (emit) this.emit("transition", { type: "focusCompleted", record: r, award, longSession: false });
        break;
      }
      case "awaitingBreak":
        this.awaiting = { after: "break", record: null, award: null, longSession: false };
        if (emit) this.emit("transition", { type: "breakFinished", sessionId: this.nextId() });
        break;
      case "manyHarvest":
        for (let i = 0; i < 11; i++) this.history.push(rec(25, i % 4 !== 2, i % 3 ? "Deep work" : undefined, 30 - i));
        this.setPoints(Math.max(this.totalPoints, 620));
        break;
      case "highPoints":
        this.setPoints(30_000);
        break;
      case "long":
        this.session = this.makeSession("focus", 180, "A very long writing block with a long task label that needs truncating");
        this.session.accruedMs = 1000;
        break;
    }
    this.revision += 1;
    if (emit) this.emitSnapshot();
  }

  setReducedMotion(on: boolean): void {
    this.settings = { ...this.settings, reducedMotion: on };
    this.revision += 1;
    this.emitSnapshot();
  }

  /** Dev convenience: also unlocks the palette so it can be previewed at any point total. */
  setPalette(id: string): void {
    if (DEV_UNLOCKS.some((u) => u.id === id && u.kind === "palette")) this.unlocked.add(id);
    this.settings = { ...this.settings, palette: id };
    this.revision += 1;
    this.emitSnapshot();
  }

  /** Simulates the shell's Suspend: a running session pauses with reason suspend. */
  simulateSleep(): void {
    if (!this.session || this.session.since === null) return;
    this.fold();
    this.session.since = null;
    this.session.pauseReason = "suspend";
    this.transition({ type: "paused", sessionId: this.session.id, kind: this.session.kind, reason: "suspend" });
  }

  openPicker(): void {
    this.emit("openPicker", null);
  }

  // ---------- Backend ----------

  getSnapshot(): Promise<Snapshot> {
    this.tick();
    return Promise.resolve(this.snapshot());
  }

  startFocus(a: StartFocusArgs = {}): Promise<Snapshot> {
    if (this.session) return reject({ code: "sessionActive" });
    const minutes = a.minutes ?? this.settings.lastFocusMinutes;
    if (!Number.isInteger(minutes) || minutes < DEV_LIMITS.minMinutes || minutes > DEV_LIMITS.maxMinutes) {
      return reject({ code: "invalidMinutes", min: DEV_LIMITS.minMinutes, max: DEV_LIMITS.maxMinutes });
    }
    const label = a.taskLabel?.replace(/\s+/g, " ").trim().slice(0, DEV_LIMITS.maxTaskLabelChars) || null;
    this.awaiting = null;
    this.session = this.makeSession("focus", minutes, label);
    this.settings = { ...this.settings, lastFocusMinutes: minutes };
    if (this.settings.hideDuringFocus) this.companionVisible = false;
    return this.transition({ type: "focusStarted", sessionId: this.session.id, plannedMs: this.session.plannedMs });
  }

  startBreak(a: StartBreakArgs = {}): Promise<Snapshot> {
    if (this.session) return reject({ code: "sessionActive" });
    const minutes = a.minutes ?? DEV_LIMITS.breakMinutes;
    this.awaiting = null;
    this.session = this.makeSession("break", minutes, null);
    return this.transition({ type: "breakStarted", sessionId: this.session.id, plannedMs: this.session.plannedMs });
  }

  pause(): Promise<Snapshot> {
    const s = this.session;
    if (!s) return reject({ code: "noSession" });
    if (s.since === null) return Promise.resolve(this.snapshot());
    this.fold();
    s.since = null;
    s.pauseReason = "user";
    return this.transition({ type: "paused", sessionId: s.id, kind: s.kind, reason: "user" });
  }

  resume(): Promise<Snapshot> {
    const s = this.session;
    if (!s) return reject({ code: "noSession" });
    if (s.since !== null) return Promise.resolve(this.snapshot());
    s.since = this.now();
    s.pauseReason = null;
    return this.transition({ type: "resumed", sessionId: s.id, kind: s.kind });
  }

  endSession(): Promise<Snapshot> {
    const s = this.session;
    if (!s) return reject({ code: "noSession" });
    this.fold();
    if (s.kind === "break") {
      this.session = null;
      return this.transition({ type: "breakEnded", sessionId: s.id });
    }
    if (s.accruedMs >= s.plannedMs) return Promise.resolve(this.complete());
    const r = this.record(s, false);
    const award = this.award(s.id, r.activeMs, false);
    this.session = null;
    this.companionVisible = true;
    return this.transition({ type: "focusInterrupted", record: r, award });
  }

  dismiss(): Promise<Snapshot> {
    if (!this.awaiting) return reject({ code: "notAwaitingChoice" });
    this.awaiting = null;
    return this.transition({ type: "dismissed" });
  }

  updateSettings(patch: UserSettingsPatch): Promise<Snapshot> {
    if (patch.palette !== undefined && patch.palette !== "auto" && patch.palette !== "forest") {
      const def = DEV_UNLOCKS.find((u) => u.id === patch.palette);
      if (!def || def.kind !== "palette" || !this.unlocked.has(def.id)) {
        return reject({ code: "invalidSettings", reason: `palette ${patch.palette} is not unlocked` });
      }
    }
    const defined = Object.fromEntries(Object.entries(patch).filter(([, v]) => v !== undefined));
    this.settings = { ...this.settings, ...defined };
    if (patch.hideDuringFocus === false) this.companionVisible = true;
    this.revision += 1;
    return Promise.resolve(this.emitSnapshot());
  }

  setHotkey(accelerator: string | null): Promise<Snapshot> {
    if (accelerator !== null) {
      const parts = accelerator.split("+");
      const key = parts[parts.length - 1] ?? "";
      const hasMod = parts.some((p) => ["Ctrl", "Alt", "Super"].includes(p));
      if (!key || (!hasMod && !/^F\d+$/.test(key)) || accelerator.length > 64) {
        return reject({ code: "invalidSettings", reason: `“${accelerator}” can't be used as a shortcut` });
      }
    }
    this.settings = { ...this.settings, hotkey: accelerator };
    this.revision += 1;
    return Promise.resolve(this.emitSnapshot());
  }

  setHotkeyCapture(active: boolean): Promise<void> {
    this.capture = active;
    console.info(`[dev mock] hotkey capture ${active ? "on" : "off"}`);
    return Promise.resolve();
  }

  getHistory(limit = 50): Promise<SessionRecord[]> {
    return Promise.resolve([...this.history].reverse().slice(0, limit));
  }

  listPacks(): Promise<PackInfo[]> {
    return Promise.resolve([...this.packs.map((p) => p.info), PETDEX_PACK_INFO, BROKEN_PACK_INFO]);
  }

  getActivePack(): Promise<ResolvedPack> {
    return Promise.resolve(this.resolvedActive());
  }

  setActivePack(id: string): Promise<ResolvedPack> {
    if (id === BROKEN_PACK_INFO.id) {
      return reject({ code: "packUnavailable", id, reason: BROKEN_PACK_INFO.errors[0] ?? "invalid pack" });
    }
    if (!this.packs.some((p) => p.info.id === id)) {
      return reject({ code: "packUnavailable", id, reason: "not installed in the dev preview" });
    }
    this.settings = { ...this.settings, activePack: id };
    const resolved = this.resolvedActive();
    this.revision += 1;
    this.emit("packChanged", resolved);
    this.emitSnapshot();
    return Promise.resolve(resolved);
  }

  replayAnimation(key: string): Promise<void> {
    if (this.session?.kind === "focus" && this.session.since !== null) return reject({ code: "focusRunning" });
    if (key !== "react" && !this.unlocked.has(key)) return reject({ code: "notUnlocked", id: key });
    this.companionVisible = true;
    this.emitSnapshot();
    this.emit("play", { key });
    return Promise.resolve();
  }

  showCompanion(): Promise<void> {
    this.companionVisible = true;
    this.emitSnapshot();
    return Promise.resolve();
  }

  hideCompanion(): Promise<void> {
    this.companionVisible = false;
    this.emitSnapshot();
    return Promise.resolve();
  }

  resetCompanionPosition(): Promise<void> {
    console.info("[dev mock] reset companion position");
    return Promise.resolve();
  }

  openSettings(tab?: SettingsTab): Promise<void> {
    this.emit("openTab", { tab: tab ?? "general" });
    return Promise.resolve();
  }

  getAppInfo(): Promise<AppInfo> {
    return Promise.resolve({
      name: "Totoro Focus",
      version: "0.1.0-dev",
      userPacksDir: "~/Library/Application Support/io.github.jc011118.totorofocus.dev/packs",
      dataDir: "~/Library/Application Support/io.github.jc011118.totorofocus.dev",
      windowMode: "other",
      debug: true,
      hotkeyError: null,
      limits: DEV_LIMITS,
      rules: DEV_RULES,
      unlocks: DEV_UNLOCKS,
    });
  }

  takeSettingsTab(): Promise<SettingsTab | null> {
    return Promise.resolve(null);
  }

  quitApp(): Promise<void> {
    console.info("[dev mock] quit requested");
    return Promise.resolve();
  }

  on<K extends BackendEventName>(event: K, cb: (payload: BackendEvents[K]) => void): Promise<Unlisten> {
    let set = this.listeners.get(event);
    if (!set) {
      set = new Set();
      this.listeners.set(event, set);
    }
    const fn = cb as (p: never) => void;
    set.add(fn);
    return Promise.resolve(() => {
      set.delete(fn);
    });
  }

  // ---------- internals ----------

  private now(): number {
    const real = Date.now();
    this.virtual += (real - this.lastReal) * this.speedFactor;
    this.lastReal = real;
    return this.virtual;
  }

  private nextId(): string {
    this.ids += 1;
    return `dev-${this.ids}`;
  }

  private makeSession(kind: SessionKind, minutes: number, taskLabel: string | null): MockSession {
    const t = this.now();
    return { id: this.nextId(), kind, taskLabel, plannedMs: minutes * MIN, accruedMs: 0, since: t, pauseReason: null, startedAt: t };
  }

  private activeMs(s: MockSession): number {
    const running = s.since === null ? 0 : this.virtual - s.since;
    return Math.min(s.plannedMs, s.accruedMs + running);
  }

  private fold(): void {
    const s = this.session;
    if (!s || s.since === null) return;
    const t = this.now();
    s.accruedMs = this.activeMs(s);
    s.since = t;
  }

  private tick(): void {
    this.now();
    const s = this.session;
    if (s && s.since !== null && this.activeMs(s) >= s.plannedMs) {
      if (s.kind === "focus") this.complete();
      else {
        this.session = null;
        this.awaiting = { after: "break", record: null, award: null, longSession: false };
        void this.transition({ type: "breakFinished", sessionId: s.id });
      }
      return;
    }
    const label = s ? formatCountdown(s.plannedMs - this.activeMs(s)) : "";
    if (label !== this.lastLabel) this.emitSnapshot();
  }

  private complete(): Snapshot {
    const s = this.session as MockSession;
    s.accruedMs = s.plannedMs;
    const r = this.record(s, true);
    const award = this.award(s.id, r.activeMs, true);
    const longSession = s.plannedMs >= DEV_RULES.longSessionMinutes * MIN;
    this.session = null;
    this.awaiting = { after: "focus", record: r, award, longSession };
    this.companionVisible = true;
    this.revision += 1;
    this.emit("transition", { type: "focusCompleted", record: r, award, longSession });
    return this.emitSnapshot();
  }

  private record(s: MockSession, completed: boolean): SessionRecord {
    const t = this.now();
    const r: SessionRecord = {
      id: s.id,
      kind: "focus",
      ...(s.taskLabel ? { taskLabel: s.taskLabel } : {}),
      startedAt: new Date(s.startedAt).toISOString(),
      endedAt: new Date(t).toISOString(),
      plannedMs: s.plannedMs,
      activeMs: Math.min(s.plannedMs, s.accruedMs),
      outcome: completed ? "completed" : "interrupted",
      endReason: completed ? "completed" : "userEnded",
      endedUtcOffsetMinutes: -new Date(t).getTimezoneOffset(),
    };
    if (!this.history.some((h) => h.id === r.id)) this.history.push(r);
    return r;
  }

  private award(id: string, activeMs: number, completed: boolean): AwardOutcome | null {
    if (this.awarded.has(id)) return null;
    this.awarded.add(id);
    const points = Math.floor(activeMs / MIN) * DEV_RULES.pointsPerActiveMinute;
    const bonus = completed ? DEV_RULES.completionBonus : 0;
    const before = new Set(this.unlocked);
    this.setPoints(this.totalPoints + points + bonus);
    const newlyUnlocked = [...this.unlocked].filter((u) => !before.has(u));
    return { sessionId: id, points, bonus, totalPoints: this.totalPoints, newlyUnlocked };
  }

  private setPoints(total: number): void {
    // Dev presets may lower points; the real engine never does.
    this.totalPoints = total;
    this.unlocked = new Set(DEV_UNLOCKS.filter((u) => u.points <= total).map((u) => u.id));
  }

  private resolvedActive(): ResolvedPack {
    const pack = this.packs.find((p) => p.info.id === this.settings.activePack);
    if (pack) return resolvePack(pack);
    return resolvePack(FIXTURE_PACK, this.settings.activePack);
  }

  private transition(t: Transition): Promise<Snapshot> {
    this.revision += 1;
    this.emit("transition", t);
    return Promise.resolve(this.emitSnapshot());
  }

  private emit<K extends BackendEventName>(event: K, payload: BackendEvents[K]): void {
    for (const cb of [...(this.listeners.get(event) ?? [])]) (cb as (p: BackendEvents[K]) => void)(payload);
  }

  private emitSnapshot(): Snapshot {
    const s = this.snapshot();
    this.emit("snapshot", s);
    return s;
  }

  private snapshot(): Snapshot {
    this.seq += 1;
    const s = this.session;
    const active = s ? this.activeMs(s) : 0;
    const remaining = s ? s.plannedMs - active : 0;
    const remainingLabel = s ? formatCountdown(remaining) : "";
    this.lastLabel = remainingLabel;
    const today = dateKey(this.virtual);
    const todays = this.history.filter((r) => dateKey(Date.parse(r.endedAt)) === today);
    const focusMs = todays.reduce((sum, r) => sum + r.activeMs, 0);
    const activeFocusMs = s?.kind === "focus" ? active : 0;
    const next = DEV_UNLOCKS.find((u) => u.points > this.totalPoints) ?? null;
    return {
      seq: this.seq,
      revision: this.revision,
      phase: s ? s.kind : this.awaiting ? "awaitingChoice" : "idle",
      session: s
        ? {
            id: s.id,
            kind: s.kind,
            status: s.since === null ? "paused" : "running",
            pauseReason: s.since === null ? s.pauseReason : null,
            taskLabel: s.taskLabel,
            plannedMs: s.plannedMs,
            activeMs: active,
            remainingMs: remaining,
            remainingLabel,
            startedAt: new Date(s.startedAt).toISOString(),
          }
        : null,
      awaiting: s ? null : this.awaiting,
      today: {
        date: today,
        focusMs,
        activeFocusMs,
        completed: todays.filter((r) => r.outcome === "completed").length,
        interrupted: todays.filter((r) => r.outcome === "interrupted").length,
        harvest: todays.map((r) => (r.outcome === "completed" ? "acorn" : "leaf")),
        focusLabel: formatFocusLabel(focusMs + activeFocusMs),
      },
      friendship: {
        totalPoints: this.totalPoints,
        unlocked: DEV_UNLOCKS.filter((u) => this.unlocked.has(u.id)).map((u) => u.id),
        next: next ? { id: next.id, label: next.label, points: next.points, remaining: next.points - this.totalPoints } : null,
      },
      settings: this.settings,
      companionVisible: this.companionVisible,
    };
  }
}
