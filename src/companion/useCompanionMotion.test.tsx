// Flourish timing in the companion: a one-shot plays at its transition or not
// at all (SPEC §7.1, §4.9 event order). Mounted through CompanionApp over the
// fake backend, so these follow the real event and snapshot paths.

import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { SessionRecord } from "../ipc/bindings/SessionRecord";
import type { SessionView } from "../ipc/bindings/SessionView";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { BackendProvider } from "../state/BackendContext";
import { FakeBackend, flush, makePack, makeSnapshot, session } from "../test/fixtures";
import { CompanionApp } from "./CompanionApp";
import { pruneShots, type QueuedShot } from "./useCompanionMotion";

const UP_TO_SLEEPY = ["base", "wave", "stretch", "umbrella", "evening", "sleepy"];
const EVERYTHING_PLAYABLE = [...UP_TO_SLEEPY, "celebrate", "acorn_gift", "hum"];

function friends(unlocked: string[]): Snapshot["friendship"] {
  return { totalPoints: 300, unlocked, next: null };
}

function focusRecord(id: string, activeMs = 1_500_000): SessionRecord {
  return {
    id,
    kind: "focus",
    startedAt: "2026-11-04T09:00:00Z",
    endedAt: "2026-11-04T09:25:00Z",
    plannedMs: 1_500_000,
    activeMs,
    outcome: "completed",
    endReason: "completed",
  };
}

function running(seq: number, extra: Partial<SessionView> = {}, snap: Partial<Snapshot> = {}): Snapshot {
  return makeSnapshot({
    seq,
    phase: "focus",
    session: session({ id: "s2", activeMs: 0, remainingMs: 1_500_000, remainingLabel: "25:00", ...extra }),
    friendship: friends(EVERYTHING_PLAYABLE),
    ...snap,
  });
}

function completedSnap(seq: number, id: string, snap: Partial<Snapshot> = {}, newlyUnlocked: string[] = []): Snapshot {
  return makeSnapshot({
    seq,
    phase: "awaitingChoice",
    awaiting: {
      after: "focus",
      record: focusRecord(id),
      award: { sessionId: id, points: 25, bonus: 5, totalPoints: 330, newlyUnlocked },
      longSession: false,
    },
    friendship: friends(EVERYTHING_PLAYABLE),
    ...snap,
  });
}

async function mount(snapshot: Snapshot, keys: string[]) {
  const backend = new FakeBackend();
  backend.snapshot = snapshot;
  backend.pack = makePack(keys);
  const view = render(
    <BackendProvider backend={backend}>
      <CompanionApp />
    </BackendProvider>,
  );
  await act(async () => {
    await flush();
    await flush();
  });
  const sprite = () => view.container.querySelector(".character .sprite")?.getAttribute("data-anim") ?? null;
  return { backend, sprite, ...view };
}

/** Stubs document.hidden and fires visibilitychange (WKWebView reports an ordered-out or occluded page this way). */
let hiddenStub: PropertyDescriptor | undefined;
function setPageHidden(hidden: boolean) {
  hiddenStub ??= Object.getOwnPropertyDescriptor(Document.prototype, "hidden");
  Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
  document.dispatchEvent(new Event("visibilitychange"));
}
afterEach(() => {
  if (hiddenStub) delete (document as { hidden?: boolean }).hidden;
  hiddenStub = undefined;
});

describe("companion flourishes", () => {
  it("plays celebrate for the session whose award unlocks it (transition precedes its snapshot)", async () => {
    const { backend, sprite } = await mount(running(1, { activeMs: 1_499_000 }, { friendship: friends(UP_TO_SLEEPY) }), [
      "idle",
      "focus",
      "completed",
      "celebrate",
    ]);
    expect(sprite()).toBe("focus");
    act(() =>
      backend.emit("transition", {
        type: "focusCompleted",
        record: focusRecord("s2"),
        award: { sessionId: "s2", points: 25, bonus: 5, totalPoints: 355, newlyUnlocked: ["celebrate"] },
        longSession: false,
      }),
    );
    expect(sprite()).toBe("celebrate");
    act(() =>
      backend.emit("snapshot", completedSnap(2, "s2", { friendship: friends([...UP_TO_SLEEPY, "celebrate"]) }, ["celebrate"])),
    );
    expect(sprite()).toBe("celebrate");
    expect(screen.getByText("New: Little celebration")).toBeTruthy();
  });

  it("hide during focus: the start wave is dropped, the completion still celebrates", async () => {
    const keys = ["idle", "focus", "completed", "wave", "celebrate"];
    const { backend, sprite } = await mount(makeSnapshot({ friendship: friends(EVERYTHING_PLAYABLE) }), keys);
    // StartFocus: HideCompanion, FocusStarted, then the snapshot (companionVisible false).
    act(() => backend.emit("transition", { type: "focusStarted", sessionId: "s2", plannedMs: 1_500_000 }));
    expect(sprite()).toBe("wave");
    act(() => backend.emit("snapshot", running(2, {}, { companionVisible: false })));
    expect(sprite()).toBe("focus");
    // 25 minutes of ticks while hidden.
    act(() => backend.emit("snapshot", running(3, { activeMs: 1_499_000 }, { companionVisible: false })));
    // Completion: ShowCompanion, FocusCompleted, then the snapshot (companionVisible true).
    act(() =>
      backend.emit("transition", { type: "focusCompleted", record: focusRecord("s2"), award: null, longSession: false }),
    );
    expect(sprite()).toBe("celebrate"); // not judged by the older (hidden) snapshot
    act(() => backend.emit("snapshot", completedSnap(4, "s2")));
    expect(sprite()).toBe("celebrate");
  });

  it("a flourish queued while the user had hidden the companion never plays when it is shown again", async () => {
    const keys = ["idle", "focus", "wave"];
    const { backend, sprite } = await mount(
      makeSnapshot({ friendship: friends(EVERYTHING_PLAYABLE), companionVisible: false }),
      keys,
    );
    act(() => backend.emit("transition", { type: "focusStarted", sessionId: "s2", plannedMs: 1_500_000 }));
    act(() => backend.emit("snapshot", running(2, {}, { companionVisible: false })));
    expect(sprite()).toBe("focus");
    // Tray → Show, two minutes in.
    act(() => backend.emit("snapshot", running(3, { activeMs: 120_000 })));
    expect(sprite()).toBe("focus");
  });

  it("a page hidden while the session starts does not replay the wave later (review scenario)", async () => {
    const keys = ["idle", "focus", "wave", "stretch"];
    const { backend, sprite } = await mount(makeSnapshot({ friendship: friends(EVERYTHING_PLAYABLE) }), keys);
    act(() => setPageHidden(true));
    act(() => backend.emit("transition", { type: "focusStarted", sessionId: "s2", plannedMs: 1_500_000 }));
    act(() => backend.emit("snapshot", running(2)));
    // Ticks keep arriving while the page is hidden; the sprite clock is frozen.
    for (let i = 1; i <= 5; i++) act(() => backend.emit("snapshot", running(2 + i, { activeMs: i * 1000 })));
    act(() => setPageHidden(false));
    expect(sprite()).toBe("focus");
  });

  it("drops an in-flight flourish when the page goes hidden instead of resuming it later", async () => {
    const keys = ["idle", "completed", "celebrate"];
    const { backend, sprite } = await mount(running(1, { activeMs: 1_499_000 }), keys);
    act(() =>
      backend.emit("transition", { type: "focusCompleted", record: focusRecord("s2"), award: null, longSession: false }),
    );
    act(() => backend.emit("snapshot", completedSnap(2, "s2")));
    expect(sprite()).toBe("celebrate");
    act(() => setPageHidden(true));
    expect(sprite()).toBe("completed");
    act(() => setPageHidden(false));
    expect(sprite()).toBe("completed");
  });

  it("a leftover flourish does not run into the next focus session", async () => {
    const keys = ["idle", "focus", "break", "hum", "wave"];
    const { backend, sprite } = await mount(
      makeSnapshot({
        phase: "break",
        session: session({ id: "b1", kind: "break", activeMs: 299_000 }),
        friendship: friends([...EVERYTHING_PLAYABLE]),
      }),
      keys,
    );
    act(() => backend.emit("transition", { type: "breakFinished", sessionId: "b1" }));
    act(() =>
      backend.emit(
        "snapshot",
        makeSnapshot({
          seq: 2,
          phase: "awaitingChoice",
          awaiting: { after: "break", record: null, award: null, longSession: false },
          friendship: friends(EVERYTHING_PLAYABLE),
        }),
      ),
    );
    expect(sprite()).toBe("hum");
    // Start again right away: the hum's moment is over once focus runs.
    act(() => backend.emit("transition", { type: "focusStarted", sessionId: "s2", plannedMs: 1_500_000 }));
    expect(sprite()).toBe("hum");
    act(() => backend.emit("snapshot", running(3)));
    expect(sprite()).toBe("wave");
  });

  it("a replay is dropped if focus starts before it played", async () => {
    const keys = ["idle", "focus", "celebrate"];
    const { backend, sprite } = await mount(makeSnapshot({ friendship: friends(EVERYTHING_PLAYABLE) }), keys);
    act(() => backend.emit("play", { key: "celebrate" }));
    expect(sprite()).toBe("celebrate");
    act(() => backend.emit("snapshot", running(2)));
    expect(sprite()).toBe("focus");
  });

  it("under reduced motion a replay keeps the still pose and only says hi", async () => {
    const keys = ["idle", "wave"];
    const base = makeSnapshot({ friendship: friends(EVERYTHING_PLAYABLE) });
    const { backend, sprite, container } = await mount(
      makeSnapshot({ settings: { ...base.settings, reducedMotion: true }, friendship: base.friendship }),
      keys,
    );
    act(() => backend.emit("play", { key: "wave" }));
    expect(sprite()).toBe("idle");
    expect(container.querySelector('.sprite[data-anim="wave"]')).toBeNull();
    expect(container.querySelector(".character__bubble")?.textContent).toBeTruthy();
  });

  it("turning reduced motion on drops a playing flourish", async () => {
    const keys = ["idle", "celebrate"];
    const base = makeSnapshot({ friendship: friends(EVERYTHING_PLAYABLE) });
    const { backend, sprite } = await mount(base, keys);
    act(() => backend.emit("play", { key: "celebrate" }));
    expect(sprite()).toBe("celebrate");
    act(() => backend.emit("snapshot", makeSnapshot({ ...base, seq: 2, settings: { ...base.settings, reducedMotion: true } })));
    expect(sprite()).toBe("idle");
  });
});

describe("pruneShots", () => {
  const shot = (scope: QueuedShot["scope"], afterSeq: number, nonce = 1): QueuedShot => ({ key: "k", nonce, scope, afterSeq });

  it("keeps a shot until a newer snapshot judges it, and returns the same queue when nothing changes", () => {
    const q = [shot({ kind: "sessionStart", sessionId: "s2" }, 1)];
    expect(pruneShots(q, makeSnapshot({ seq: 1 }), false)).toBe(q);
    expect(pruneShots(q, null, false)).toBe(q);
    expect(pruneShots(q, makeSnapshot({ seq: 2 }), false)).toEqual([]);
    expect(pruneShots(q, running(2), false)).toBe(q);
  });

  it("drops only the shots whose moment has passed, in order", () => {
    const q = [shot({ kind: "breakDone" }, 1, 1), shot({ kind: "sessionStart", sessionId: "s2" }, 1, 2), shot({ kind: "react" }, 1, 3)];
    expect(pruneShots(q, running(2), false).map((e) => e.nonce)).toEqual([2, 3]);
  });

  it("drops everything under reduced motion", () => {
    expect(pruneShots([shot({ kind: "react" }, 5)], makeSnapshot({ seq: 1 }), true)).toEqual([]);
  });
});
