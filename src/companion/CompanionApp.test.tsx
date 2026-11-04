import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { BackendProvider } from "../state/BackendContext";
import { ALL_IDS, FakeBackend, flush, makePack, makeSnapshot, session } from "../test/fixtures";
import { CompanionApp } from "./CompanionApp";

async function mount(snapshot: Snapshot, backend = new FakeBackend()) {
  backend.snapshot = snapshot;
  const view = render(
    <BackendProvider backend={backend}>
      <CompanionApp />
    </BackendProvider>,
  );
  await act(async () => {
    await flush();
    await flush();
  });
  return { backend, ...view };
}

const button = (name: string | RegExp) => screen.getByRole("button", { name });

describe("CompanionApp", () => {
  it("uses only tabIndex=-1 buttons, no inputs, and a drag grip without controls", async () => {
    const { container } = await mount(makeSnapshot());
    expect(container.querySelectorAll("input, select, textarea, [contenteditable]")).toHaveLength(0);
    const buttons = [...container.querySelectorAll("button")];
    expect(buttons.length).toBeGreaterThan(5);
    for (const b of buttons) expect(b.tabIndex).toBe(-1);
    const grip = container.querySelector("[data-tauri-drag-region]");
    expect(grip?.getAttribute("data-tauri-drag-region")).toBe("deep");
    expect(grip?.querySelector("button, a, input, [tabindex], [role=button]")).toBeNull();
    // [grip][menu][hide], in that order.
    const header = grip?.parentElement;
    expect([...(header?.children ?? [])].map((c) => c.getAttribute("aria-label") ?? "grip")).toEqual([
      "grip",
      "Companion menu",
      "Hide companion",
    ]);
  });

  it("idle: starts the last duration and presets with one click", async () => {
    const { backend } = await mount(makeSnapshot({ settings: { ...makeSnapshot().settings, lastFocusMinutes: 45 } }));
    fireEvent.click(button("Start 45"));
    await act(flush);
    expect(backend.startFocus).toHaveBeenCalledWith({ minutes: 45 });
    fireEvent.click(button("Start 15 minutes"));
    await act(flush);
    expect(backend.startFocus).toHaveBeenLastCalledWith({ minutes: 15 });
  });

  it("idle: the task button opens Settings → Start", async () => {
    const { backend } = await mount(makeSnapshot());
    fireEvent.click(button("Add a task in Settings"));
    await act(flush);
    expect(backend.openSettings).toHaveBeenCalledWith("start");
  });

  it("custom stepper opens from the button and from focus://open-picker", async () => {
    const { backend } = await mount(makeSnapshot());
    fireEvent.click(button("Custom length"));
    expect(screen.getByText("Custom")).toBeTruthy();
    const more = button("More minutes");
    fireEvent.click(more); // 25 → 30
    fireEvent.click(more); // 30 → 35
    fireEvent.click(button("Start 35"));
    await act(flush);
    expect(backend.startFocus).toHaveBeenCalledWith({ minutes: 35 });
    expect(screen.queryByText("Custom")).toBeNull(); // starting closes the stepper

    act(() => backend.emit("openPicker", null));
    expect(screen.getByText("Custom")).toBeTruthy();
    fireEvent.click(button("Fewer minutes")); // 25 → 20
    expect(button("Start 20")).toBeTruthy();
    fireEvent.click(button("Back"));
    expect(screen.queryByText("Custom")).toBeNull();
  });

  it("running: shows remainingLabel and sends pause / end", async () => {
    const running = makeSnapshot({ phase: "focus", session: session({ taskLabel: "Review one lecture" }) });
    const { backend } = await mount(running);
    expect(screen.getByRole("timer").textContent).toBe("24:13");
    expect(screen.getByText("Review one lecture")).toBeTruthy();
    fireEvent.click(button("Pause"));
    await act(flush);
    expect(backend.pause).toHaveBeenCalled();
    fireEvent.click(button("End session"));
    await act(flush);
    expect(backend.endSession).toHaveBeenCalled();
  });

  it("paused: offers resume with a gentle reason", async () => {
    const paused = makeSnapshot({ phase: "focus", session: session({ status: "paused", pauseReason: "suspend" }) });
    const { backend } = await mount(paused);
    expect(screen.getByText("Paused while your Mac slept")).toBeTruthy();
    fireEvent.click(button("Resume"));
    await act(flush);
    expect(backend.resume).toHaveBeenCalled();
  });

  it("break: a separate look with only End break", async () => {
    const { container } = await mount(
      makeSnapshot({ phase: "break", session: session({ kind: "break", remainingLabel: "03:42" }) }),
    );
    expect(container.querySelector(".ring--break")).toBeTruthy();
    expect(screen.getByText("Resting")).toBeTruthy();
    expect(button("End break")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
  });

  it("awaiting after focus: break, another, done — and the earned points", async () => {
    const record = {
      id: "s1",
      kind: "focus" as const,
      startedAt: "2026-11-04T09:00:00Z",
      endedAt: "2026-11-04T09:25:00Z",
      plannedMs: 1_500_000,
      activeMs: 1_500_000,
      outcome: "completed" as const,
      endReason: "completed" as const,
    };
    const done = makeSnapshot({
      phase: "awaitingChoice",
      awaiting: {
        after: "focus",
        record,
        award: { sessionId: "s1", points: 25, bonus: 5, totalPoints: 30, newlyUnlocked: ["wave"] },
        longSession: false,
      },
    });
    const { backend } = await mount(done);
    expect(screen.getByText("25 min together")).toBeTruthy();
    expect(screen.getByText("+30 friendship")).toBeTruthy();
    expect(screen.getByText("New: A small wave")).toBeTruthy();
    fireEvent.click(button("5-min break"));
    await act(flush);
    expect(backend.startBreak).toHaveBeenCalled();
    fireEvent.click(button("Another 25"));
    await act(flush);
    expect(backend.startFocus).toHaveBeenCalledWith({ minutes: 25 });
    fireEvent.click(button("Done"));
    await act(flush);
    expect(backend.dismiss).toHaveBeenCalled();
  });

  it("awaiting after a break: the presets again", async () => {
    await mount(
      makeSnapshot({ phase: "awaitingChoice", awaiting: { after: "break", record: null, award: null, longSession: false } }),
    );
    expect(screen.getByText("Break's over")).toBeTruthy();
    expect(button("Start 25")).toBeTruthy();
    expect(button("Start 45 minutes")).toBeTruthy();
  });

  it("applies snapshots only in seq order", async () => {
    const { backend } = await mount(makeSnapshot({ seq: 5 }));
    act(() => backend.emit("snapshot", makeSnapshot({ seq: 4, phase: "focus", session: session() })));
    expect(screen.queryByRole("timer")).toBeNull();
    act(() => backend.emit("snapshot", makeSnapshot({ seq: 6, phase: "focus", session: session({ remainingLabel: "12:00" }) })));
    expect(screen.getByRole("timer").textContent).toBe("12:00");
  });

  it("a character click reacts locally and makes no IPC call", async () => {
    const backend = new FakeBackend();
    backend.pack = makePack(["idle"]); // no react art → CSS hop
    const { container } = await mount(makeSnapshot(), backend);
    const before = Object.values(backend).filter((v) => typeof v === "function" && "mock" in v).map((f) => f.mock.calls.length);
    fireEvent.click(button(/Your companion/));
    const after = Object.values(backend).filter((v) => typeof v === "function" && "mock" in v).map((f) => f.mock.calls.length);
    expect(after).toEqual(before);
    expect(container.querySelector(".static-companion, .sprite")).toBeTruthy();
  });

  it("plays an unlocked flourish on its transition", async () => {
    const backend = new FakeBackend();
    backend.pack = makePack(["idle", "focus", "wave"]);
    const snap = makeSnapshot({ friendship: { totalPoints: 30, unlocked: ["base", "wave"], next: null } });
    const { container } = await mount(snap, backend);
    act(() => backend.emit("transition", { type: "focusStarted", sessionId: "s2", plannedMs: 1_500_000 }));
    expect(container.querySelector(".sprite")?.getAttribute("data-anim")).toBe("wave");
  });

  it("character menu lists unlocked replays, dims missing art, and opens companion settings", async () => {
    const backend = new FakeBackend();
    backend.pack = makePack(["idle", "wave"]);
    const snap = makeSnapshot({ friendship: { totalPoints: 100, unlocked: ["base", "wave", "stretch", "umbrella"], next: null } });
    await mount(snap, backend);
    fireEvent.click(button("Companion menu"));
    const menu = screen.getByRole("dialog", { name: "Character menu" });
    const wave = within(menu).getByRole("button", { name: "Replay A small wave" });
    const stretch = within(menu).getByRole("button", { name: "Big stretch (not in this pack)" });
    expect((stretch as HTMLButtonElement).disabled).toBe(true);
    expect(within(menu).getByRole("button", { name: "Rainy day (not in this pack)" })).toBeTruthy();
    fireEvent.click(wave);
    await act(flush);
    expect(backend.replayAnimation).toHaveBeenCalledWith("wave");

    fireEvent.click(button("Companion menu"));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Companion settings…" }));
    await act(flush);
    expect(backend.openSettings).toHaveBeenCalledWith("companion");
  });

  it("disables replays while focus runs", async () => {
    const backend = new FakeBackend();
    backend.pack = makePack(["idle", "wave"]);
    await mount(
      makeSnapshot({ phase: "focus", session: session(), friendship: { totalPoints: 9e4, unlocked: ALL_IDS, next: null } }),
      backend,
    );
    fireEvent.click(button("Companion menu"));
    expect((screen.getByRole("button", { name: "Replay A small wave" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("Replays wait until this session ends.")).toBeTruthy();
  });

  it("shows an interruption as saved minutes, never as a failure", async () => {
    const { backend } = await mount(makeSnapshot());
    act(() =>
      backend.emit("transition", {
        type: "focusInterrupted",
        record: {
          id: "s3",
          kind: "focus",
          startedAt: "2026-11-04T09:00:00Z",
          endedAt: "2026-11-04T09:22:30Z",
          plannedMs: 1_500_000,
          activeMs: 22 * 60_000 + 30_000,
          outcome: "interrupted",
          endReason: "userEnded",
        },
        award: { sessionId: "s3", points: 22, bonus: 0, totalPoints: 22, newlyUnlocked: [] },
      }),
    );
    expect(screen.getByText("Saved 22 min · +22")).toBeTruthy();
  });

  it("shows command errors calmly", async () => {
    const { backend } = await mount(makeSnapshot());
    backend.startFocus.mockRejectedValueOnce({ code: "sessionActive" });
    fireEvent.click(button("Start 25"));
    await act(flush);
    expect(screen.getByRole("status").textContent).toBe("A session is already going.");
  });
});
