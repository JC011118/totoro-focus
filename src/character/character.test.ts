import { describe, expect, it } from "vitest";
import type { SessionRecord } from "../ipc/bindings/SessionRecord";
import type { SessionView } from "../ipc/bindings/SessionView";
import type { Transition } from "../ipc/bindings/Transition";
import { ALL_IDS, anim, makePack, makeSnapshot, session, UNLOCKS } from "../test/fixtures";
import {
  baseKeyFor,
  canBlinkOver,
  flourishesFor,
  scopeIsCurrent,
  scopeOf,
  START_SHOT_WINDOW_MS,
  unlockedAsOf,
  type ShotScope,
} from "./mapping";
import {
  backgroundPosition,
  backgroundSize,
  cssUrl,
  fitFrame,
  frameAt,
  frameDurations,
  imageRendering,
  stillFrameOf,
} from "./spriteMath";

const record: SessionRecord = {
  id: "s1",
  kind: "focus",
  startedAt: "2026-11-04T09:00:00Z",
  endedAt: "2026-11-04T09:25:00Z",
  plannedMs: 1_500_000,
  activeMs: 1_500_000,
  outcome: "completed",
  endReason: "completed",
};

describe("baseKeyFor (SPEC §7.1)", () => {
  it("maps every phase", () => {
    expect(baseKeyFor(makeSnapshot(), null)).toBe("idle");
    expect(baseKeyFor(makeSnapshot(), "sleepy")).toBe("sleepy");
    expect(baseKeyFor(makeSnapshot({ phase: "focus", session: session() }), "sleepy")).toBe("focus");
    expect(baseKeyFor(makeSnapshot({ phase: "focus", session: session({ status: "paused" }) }), null)).toBe("paused");
    expect(baseKeyFor(makeSnapshot({ phase: "break", session: session({ kind: "break" }) }), null)).toBe("break");
    expect(baseKeyFor(makeSnapshot({ phase: "break", session: session({ kind: "break", status: "paused" }) }), null)).toBe(
      "paused",
    );
    const done = makeSnapshot({ phase: "awaitingChoice", awaiting: { after: "focus", record, award: null, longSession: false } });
    expect(baseKeyFor(done, "sleepy")).toBe("completed");
    const afterBreak = makeSnapshot({ phase: "awaitingChoice", awaiting: { after: "break", record: null, award: null, longSession: false } });
    expect(baseKeyFor(afterBreak, "sleepy")).toBe("idle");
  });

  it("lets blink replace only idle and focus", () => {
    expect(canBlinkOver("idle")).toBe(true);
    expect(canBlinkOver("focus")).toBe(true);
    for (const k of ["paused", "break", "completed", "sleepy"]) expect(canBlinkOver(k)).toBe(false);
  });
});

describe("flourishesFor (SPEC §7.1)", () => {
  const everything = makePack(["idle", ...ALL_IDS]);
  const completed = (longSession: boolean): Transition => ({ type: "focusCompleted", record, award: null, longSession });

  it("maps each transition to its flourish when unlocked and drawable", () => {
    expect(flourishesFor({ type: "focusStarted", sessionId: "s", plannedMs: 1 }, ALL_IDS, everything, false)).toEqual(["wave"]);
    expect(flourishesFor({ type: "breakStarted", sessionId: "s", plannedMs: 1 }, ALL_IDS, everything, false)).toEqual(["stretch"]);
    expect(flourishesFor({ type: "breakFinished", sessionId: "s" }, ALL_IDS, everything, false)).toEqual(["hum"]);
    expect(flourishesFor(completed(false), ALL_IDS, everything, false)).toEqual(["celebrate", "acorn_gift"]);
    expect(flourishesFor(completed(true), ALL_IDS, everything, false)).toEqual(["long_session", "acorn_gift"]);
  });

  it("plays nothing for other transitions (no scolding, no interruption flourish)", () => {
    const quiet: Transition[] = [
      { type: "focusInterrupted", record: { ...record, outcome: "interrupted", endReason: "userEnded" }, award: null },
      { type: "paused", sessionId: "s", kind: "focus", reason: "user" },
      { type: "resumed", sessionId: "s", kind: "focus" },
      { type: "breakEnded", sessionId: "s" },
      { type: "dismissed" },
      { type: "restored", sessionId: "s", kind: "focus" },
    ];
    for (const t of quiet) expect(flourishesFor(t, ALL_IDS, everything, false)).toEqual([]);
  });

  it("requires the id to be unlocked", () => {
    expect(flourishesFor({ type: "focusStarted", sessionId: "s", plannedMs: 1 }, ["base"], everything, false)).toEqual([]);
    expect(flourishesFor(completed(true), ["base", "acorn_gift"], everything, false)).toEqual(["acorn_gift"]);
  });

  it("requires the pack to resolve it; long_session falls back to celebrate", () => {
    const noLong = makePack(["idle", "celebrate", "acorn_gift"]);
    expect(flourishesFor(completed(true), ALL_IDS, noLong, false)).toEqual(["celebrate", "acorn_gift"]);
    const bare = makePack(["idle"]);
    expect(flourishesFor(completed(true), ALL_IDS, bare, false)).toEqual([]);
    expect(flourishesFor(completed(false), ALL_IDS, null, false)).toEqual([]);
  });

  it("never plays under reduced motion", () => {
    expect(flourishesFor(completed(false), ALL_IDS, everything, true)).toEqual([]);
    expect(flourishesFor({ type: "focusStarted", sessionId: "s", plannedMs: 1 }, ALL_IDS, everything, true)).toEqual([]);
  });

  it("counts the completing session's own award as unlocked (its snapshot arrives after the transition)", () => {
    const upToSleepy = ["base", "wave", "stretch", "umbrella", "evening", "sleepy"];
    const award = (newlyUnlocked: string[]) => ({ sessionId: "s1", points: 25, bonus: 5, totalPoints: 355, newlyUnlocked });
    const t = (newlyUnlocked: string[], longSession = false): Transition => ({
      type: "focusCompleted",
      record,
      award: award(newlyUnlocked),
      longSession,
    });
    expect(flourishesFor(t([]), upToSleepy, everything, false)).toEqual([]);
    expect(flourishesFor(t(["celebrate"]), upToSleepy, everything, false)).toEqual(["celebrate"]);
    expect(flourishesFor(t(["acorn_gift"]), [...upToSleepy, "celebrate"], everything, false)).toEqual([
      "celebrate",
      "acorn_gift",
    ]);
    const before1000 = UNLOCKS.filter((u) => u.points < 1000).map((u) => u.id);
    expect(flourishesFor(t(["long_session"], true), before1000, everything, false)).toEqual(["long_session", "acorn_gift"]);
    // Still subject to the pack and to reduced motion.
    expect(flourishesFor(t(["celebrate"]), upToSleepy, makePack(["idle"]), false)).toEqual([]);
    expect(flourishesFor(t(["celebrate"]), upToSleepy, everything, true)).toEqual([]);
  });
});

describe("unlockedAsOf", () => {
  it("adds award unlocks only for focus endings", () => {
    const award = { sessionId: "s1", points: 1, bonus: 0, totalPoints: 1, newlyUnlocked: ["celebrate"] };
    expect([...unlockedAsOf({ type: "focusCompleted", record, award, longSession: false }, ["base"])].sort()).toEqual([
      "base",
      "celebrate",
    ]);
    expect([
      ...unlockedAsOf({ type: "focusInterrupted", record: { ...record, outcome: "interrupted" }, award }, ["base"]),
    ]).toContain("celebrate");
    expect([...unlockedAsOf({ type: "focusCompleted", record, award: null, longSession: false }, ["base"])]).toEqual([
      "base",
    ]);
    expect([...unlockedAsOf({ type: "focusStarted", sessionId: "s", plannedMs: 1 }, ["base"])]).toEqual(["base"]);
  });
});

describe("one-shot scopes: a flourish plays at its moment or not at all", () => {
  const visible = { companionVisible: true };
  const running = (extra: Partial<SessionView> = {}) =>
    makeSnapshot({ phase: "focus", session: session({ id: "s2", activeMs: 0, ...extra }) });
  const doneAfterFocus = (id: string) =>
    makeSnapshot({
      phase: "awaitingChoice",
      awaiting: { after: "focus", record: { ...record, id }, award: null, longSession: false },
    });
  const doneAfterBreak = makeSnapshot({
    phase: "awaitingChoice",
    awaiting: { after: "break", record: null, award: null, longSession: false },
  });

  it("scopes each transition with flourishes", () => {
    expect(scopeOf({ type: "focusStarted", sessionId: "s2", plannedMs: 1 })).toEqual({ kind: "sessionStart", sessionId: "s2" });
    expect(scopeOf({ type: "breakStarted", sessionId: "b1", plannedMs: 1 })).toEqual({ kind: "sessionStart", sessionId: "b1" });
    expect(scopeOf({ type: "focusCompleted", record, award: null, longSession: false })).toEqual({
      kind: "focusDone",
      recordId: "s1",
    });
    expect(scopeOf({ type: "breakFinished", sessionId: "b1" })).toEqual({ kind: "breakDone" });
    expect(scopeOf({ type: "dismissed" })).toBeNull();
    expect(scopeOf({ type: "paused", sessionId: "s", kind: "focus", reason: "user" })).toBeNull();
  });

  it("a start flourish is current only in its own session's first seconds", () => {
    const start: ShotScope = { kind: "sessionStart", sessionId: "s2" };
    expect(scopeIsCurrent(start, running())).toBe(true);
    expect(scopeIsCurrent(start, running({ activeMs: START_SHOT_WINDOW_MS - 1 }))).toBe(true);
    expect(scopeIsCurrent(start, running({ activeMs: START_SHOT_WINDOW_MS }))).toBe(false);
    expect(scopeIsCurrent(start, running({ activeMs: 600_000 }))).toBe(false);
    expect(scopeIsCurrent(start, running({ status: "paused" }))).toBe(false);
    expect(scopeIsCurrent(start, running({ id: "s3" }))).toBe(false);
    expect(scopeIsCurrent(start, makeSnapshot())).toBe(false);
    expect(scopeIsCurrent(start, doneAfterFocus("s2"))).toBe(false);
    const breakStart: ShotScope = { kind: "sessionStart", sessionId: "b1" };
    const breakRunning = makeSnapshot({ phase: "break", session: session({ id: "b1", kind: "break", activeMs: 1000 }) });
    expect(scopeIsCurrent(breakStart, breakRunning)).toBe(true);
  });

  it("completion flourishes are current while that completion is on screen", () => {
    const done: ShotScope = { kind: "focusDone", recordId: "s2" };
    expect(scopeIsCurrent(done, doneAfterFocus("s2"))).toBe(true);
    expect(scopeIsCurrent(done, doneAfterFocus("s9"))).toBe(false);
    expect(scopeIsCurrent(done, makeSnapshot())).toBe(false);
    expect(scopeIsCurrent(done, running({ id: "s3" }))).toBe(false);
    expect(scopeIsCurrent(done, doneAfterBreak)).toBe(false);
    const hum: ShotScope = { kind: "breakDone" };
    expect(scopeIsCurrent(hum, doneAfterBreak)).toBe(true);
    expect(scopeIsCurrent(hum, running())).toBe(false);
    expect(scopeIsCurrent(hum, makeSnapshot())).toBe(false);
  });

  it("replays never run into active focus; clicks always may", () => {
    expect(scopeIsCurrent({ kind: "replay" }, makeSnapshot())).toBe(true);
    expect(scopeIsCurrent({ kind: "replay" }, running({ status: "paused" }))).toBe(true);
    expect(scopeIsCurrent({ kind: "replay" }, running())).toBe(false);
    expect(scopeIsCurrent({ kind: "react" }, running({ activeMs: 600_000 }))).toBe(true);
  });

  it("nothing is current while the shell reports the companion hidden", () => {
    const hidden = { companionVisible: false };
    expect(scopeIsCurrent({ kind: "sessionStart", sessionId: "s2" }, { ...running(), ...hidden })).toBe(false);
    expect(scopeIsCurrent({ kind: "focusDone", recordId: "s2" }, { ...doneAfterFocus("s2"), ...hidden })).toBe(false);
    expect(scopeIsCurrent({ kind: "react" }, { ...makeSnapshot(), ...hidden })).toBe(false);
    expect(scopeIsCurrent({ kind: "react" }, { ...makeSnapshot(), ...visible })).toBe(true);
  });
});

describe("sprite math (SPEC §7, percentages)", () => {
  it("sizes the sheet as columns×100% rows×100%", () => {
    expect(backgroundSize({ sheetColumns: 4, sheetRows: 1 })).toBe("400% 100%");
    expect(backgroundSize({ sheetColumns: 8, sheetRows: 9 })).toBe("800% 900%");
    expect(backgroundSize({ sheetColumns: 0, sheetRows: 0 })).toBe("100% 100%");
  });

  it("positions frame c of row r at c/(cols−1) and r/(rows−1)", () => {
    const strip = anim("idle", { frames: 4, sheetColumns: 4, sheetRows: 1 });
    expect(backgroundPosition(strip, 0)).toBe("0% 0%");
    expect(backgroundPosition(strip, 1)).toBe("33.3333% 0%");
    expect(backgroundPosition(strip, 3)).toBe("100% 0%");
    // Petdex-style 8×9 atlas: row 3, 4 frames from column 0.
    const atlas = anim("wave", { frames: 4, row: 3, column: 0, sheetColumns: 8, sheetRows: 9 });
    expect(backgroundPosition(atlas, 2)).toBe("28.5714% 37.5%");
    // An animation that starts mid-row.
    const mid = anim("blink", { frames: 2, row: 1, column: 2, sheetColumns: 4, sheetRows: 4 });
    expect(backgroundPosition(mid, 1)).toBe("100% 33.3333%");
  });

  it("uses 0% when a count is 1 and clamps the frame index", () => {
    const single = anim("paused", { frames: 1, sheetColumns: 1, sheetRows: 1 });
    expect(backgroundPosition(single, 0)).toBe("0% 0%");
    const strip = anim("idle", { frames: 4, sheetColumns: 4 });
    expect(backgroundPosition(strip, 99)).toBe("100% 0%");
    expect(backgroundPosition(strip, -2)).toBe("0% 0%");
  });

  it("uses pixelated sampling only for pixelated packs at scale ≥ 1", () => {
    expect(imageRendering("pixelated", 96, 96)).toBe("pixelated");
    expect(imageRendering("pixelated", 192, 96)).toBe("pixelated");
    expect(imageRendering("pixelated", 74, 192)).toBe("auto");
    expect(imageRendering("smooth", 192, 96)).toBe("auto");
  });

  it("fits the frame aspect into the box", () => {
    expect(fitFrame(96, 96, 74, 74)).toEqual({ width: 74, height: 74 });
    expect(fitFrame(192, 208, 74, 74)).toEqual({ width: 68, height: 74 });
    expect(fitFrame(0, 10, 50, 40)).toEqual({ width: 50, height: 40 });
  });

  it("uses frameDurationsMs when it has one entry per frame, else 1000/fps", () => {
    expect(frameDurations({ frames: 2, fps: 4, frameDurationsMs: [100, 300] })).toEqual([100, 300]);
    expect(frameDurations({ frames: 3, fps: 4, frameDurationsMs: [100, 300] })).toEqual([250, 250, 250]);
    expect(frameDurations({ frames: 2, fps: 0, frameDurationsMs: null })).toEqual([250, 250]);
    expect(stillFrameOf({ frames: 6, stillFrame: 5 })).toBe(5);
    expect(stillFrameOf({ frames: 3, stillFrame: 9 })).toBe(2);
  });

  it("steps frames: loops wrap, one-shots hold the last frame, the cap ends early", () => {
    const d = [100, 200, 100];
    expect(frameAt(d, 0, true)).toEqual({ frame: 0, done: false, nextIn: 100 });
    expect(frameAt(d, 150, true)).toEqual({ frame: 1, done: false, nextIn: 150 });
    expect(frameAt(d, 420, true)).toMatchObject({ frame: 0, done: false });
    expect(frameAt(d, 350, false)).toMatchObject({ frame: 2, done: false });
    expect(frameAt(d, 400, false)).toEqual({ frame: 2, done: true, nextIn: Infinity });
    expect(frameAt(d, 9999, false)).toMatchObject({ frame: 2, done: true });
    expect(frameAt(d, 250, false, 250)).toMatchObject({ frame: 1, done: true });
    expect(frameAt(d, 50, false, 80).nextIn).toBe(30);
    expect(frameAt([500], 900, true)).toEqual({ frame: 0, done: false, nextIn: Infinity });
  });

  it("quotes urls safely", () => {
    expect(cssUrl("pack://localhost/p/idle.svg")).toBe('url("pack://localhost/p/idle.svg")');
    expect(cssUrl('a"b')).toBe('url("a\\22 b")');
  });
});
