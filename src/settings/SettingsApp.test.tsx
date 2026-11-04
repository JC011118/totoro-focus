import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SessionRecord } from "../ipc/bindings/SessionRecord";
import { BackendProvider } from "../state/BackendContext";
import { APP_INFO, FakeBackend, flush, makePack, makeSnapshot } from "../test/fixtures";
import { friendshipProgress, replayMessage } from "./CompanionTab";
import { SettingsApp } from "./SettingsApp";
import { tabFromHash } from "./tabs";
import { UNLOCKS } from "../test/fixtures";

async function mount(backend = new FakeBackend(), hash = "") {
  window.history.replaceState(null, "", `/settings.html${hash}`);
  const view = render(
    <BackendProvider backend={backend}>
      <SettingsApp />
    </BackendProvider>,
  );
  await act(async () => {
    await flush();
    await flush();
  });
  return { backend, ...view };
}

const tab = (name: string) => screen.getByRole("tab", { name });

describe("tabFromHash", () => {
  it("reads settings.html#<tab>, defaulting to general", () => {
    expect(tabFromHash("#start")).toBe("start");
    expect(tabFromHash("#history")).toBe("history");
    expect(tabFromHash("#/companion")).toBe("companion");
    expect(tabFromHash("#settings/start")).toBe("start");
    expect(tabFromHash("")).toBe("general");
    expect(tabFromHash("#bogus")).toBe("general");
  });
});

describe("SettingsApp", () => {
  it("opens the tab named in location.hash and follows focus://open-tab", async () => {
    const { backend } = await mount(new FakeBackend(), "#history");
    expect(tab("History").getAttribute("aria-selected")).toBe("true");
    act(() => backend.emit("openTab", { tab: "start" }));
    expect(tab("Start").getAttribute("aria-selected")).toBe("true");
    expect(window.location.hash).toBe("#start");
  });

  it("Start tab sends the typed label and minutes", async () => {
    const { backend } = await mount(new FakeBackend(), "#start");
    const label = screen.getByPlaceholderText("e.g. Review one lecture") as HTMLInputElement;
    expect(label.maxLength).toBe(80);
    fireEvent.change(label, { target: { value: "  Review one lecture " } });
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "40" } });
    fireEvent.click(screen.getByRole("button", { name: /Start focus · 40 min/ }));
    await act(flush);
    expect(backend.startFocus).toHaveBeenCalledWith({ minutes: 40, taskLabel: "Review one lecture" });
  });

  it("Start tab refuses out-of-range minutes before calling the backend", async () => {
    const { backend } = await mount(new FakeBackend(), "#start");
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "181" } });
    expect((screen.getByRole("button", { name: /Start focus/ }) as HTMLButtonElement).disabled).toBe(true);
    expect(backend.startFocus).not.toHaveBeenCalled();
  });

  it("General tab toggles settings through update_settings", async () => {
    const { backend } = await mount(new FakeBackend(), "#general");
    fireEvent.click(screen.getByRole("switch", { name: "Reduced motion" }));
    await act(flush);
    expect(backend.updateSettings).toHaveBeenCalledWith({ reducedMotion: true });
    fireEvent.click(screen.getByRole("switch", { name: "Countdown in the menu bar" }));
    await act(flush);
    expect(backend.updateSettings).toHaveBeenLastCalledWith({ menuBarTitle: false });
  });

  it("hotkey recorder: capture on, record from KeyboardEvent.code, save, capture off", async () => {
    const { backend } = await mount(new FakeBackend(), "#general");
    expect(screen.getByText("⌃⌥T")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    await act(flush);
    expect(backend.setHotkeyCapture).toHaveBeenLastCalledWith(true);
    fireEvent.keyDown(window, { code: "AltLeft", key: "Alt", altKey: true, ctrlKey: true });
    expect(screen.getByText("⌃⌥")).toBeTruthy();
    fireEvent.keyDown(window, { code: "KeyK", key: "˚", altKey: true, ctrlKey: true });
    await act(flush);
    expect(backend.setHotkey).toHaveBeenCalledWith("Ctrl+Alt+K");
    expect(backend.setHotkeyCapture).toHaveBeenLastCalledWith(false);
  });

  it("hotkey recorder: Esc cancels and shows InvalidSettings reasons", async () => {
    const backend = new FakeBackend();
    backend.setHotkey.mockRejectedValueOnce({ code: "invalidSettings", reason: "that shortcut is taken by another app" });
    await mount(backend, "#general");
    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    await act(flush);
    fireEvent.keyDown(window, { code: "Escape", key: "Escape" });
    await act(flush);
    expect(backend.setHotkey).not.toHaveBeenCalled();
    expect(backend.setHotkeyCapture).toHaveBeenLastCalledWith(false);

    fireEvent.click(screen.getByRole("button", { name: "Change…" }));
    await act(flush);
    fireEvent.keyDown(window, { code: "KeyT", key: "t" });
    expect(screen.getByText("Include ⌃ Control, ⌥ Option or ⌘ Command.")).toBeTruthy();
    fireEvent.keyDown(window, { code: "KeyJ", key: "j", metaKey: true });
    await act(flush);
    expect(screen.getByText("That shortcut is taken by another app.")).toBeTruthy();
  });

  describe("hotkey registration error (AppInfo.hotkeyError)", () => {
    const STARTUP_ERROR = "⌃⌥T is used by another app";
    const shown = () => screen.queryByText(/The saved shortcut couldn't be registered/);
    const withHotkey = (seq: number, hotkey: string | null) =>
      makeSnapshot({ seq, settings: { ...makeSnapshot().settings, hotkey } });
    const failingBackend = () => {
      const backend = new FakeBackend();
      // A shell that keeps reporting the startup error: the recorder must not rely on a refetch alone.
      backend.getAppInfo.mockResolvedValue({ ...APP_INFO, hotkeyError: STARTUP_ERROR });
      return backend;
    };
    const record = async (code: string, key: string) => {
      fireEvent.click(screen.getByRole("button", { name: /Change…|Set…/ }));
      await act(flush);
      fireEvent.keyDown(window, { code, key, altKey: true, ctrlKey: true });
      await act(flush);
      await act(flush);
    };

    it("is cleared by recording a shortcut that set_hotkey accepts (review scenario)", async () => {
      const backend = failingBackend();
      await mount(backend, "#general");
      expect(shown()?.textContent).toContain(STARTUP_ERROR);
      backend.setHotkey.mockResolvedValueOnce(withHotkey(2, "Ctrl+Alt+K"));
      await record("KeyK", "˚");
      expect(backend.setHotkey).toHaveBeenCalledWith("Ctrl+Alt+K");
      expect(screen.getByText("⌃⌥K")).toBeTruthy();
      expect(shown()).toBeNull();
      expect(backend.getAppInfo.mock.calls.length).toBeGreaterThan(1); // asked the shell again
    });

    it("is cleared by Disable shortcut", async () => {
      const backend = failingBackend();
      await mount(backend, "#general");
      backend.setHotkey.mockResolvedValueOnce(withHotkey(2, null));
      fireEvent.click(screen.getByRole("button", { name: "Disable shortcut" }));
      await act(flush);
      await act(flush);
      expect(backend.setHotkey).toHaveBeenCalledWith(null);
      expect(screen.getByText("Off")).toBeTruthy();
      expect(shown()).toBeNull();
    });

    it("stays while set_hotkey fails, and follows the shell after a capture", async () => {
      const backend = failingBackend();
      await mount(backend, "#general");
      backend.setHotkey.mockRejectedValueOnce({ code: "invalidSettings", reason: "that shortcut is taken by another app" });
      await record("KeyJ", "∆");
      expect(screen.getByText("That shortcut is taken by another app.")).toBeTruthy();
      expect(shown()).toBeNull(); // the fresher error wins while it is shown

      // Cancelling a capture re-registers the saved binding; the shell now reports it working.
      backend.getAppInfo.mockResolvedValue({ ...APP_INFO, hotkeyError: null });
      fireEvent.click(screen.getByRole("button", { name: "Change…" }));
      await act(flush);
      fireEvent.keyDown(window, { code: "Escape", key: "Escape" });
      await act(flush);
      await act(flush);
      expect(backend.setHotkeyCapture).toHaveBeenLastCalledWith(false);
      expect(shown()).toBeNull();
      expect(screen.queryByText("That shortcut is taken by another app.")).toBeNull();
    });

    it("shows a new error the shell reports after a successful save", async () => {
      const backend = failingBackend();
      await mount(backend, "#general");
      backend.setHotkey.mockResolvedValueOnce(withHotkey(2, "Ctrl+Alt+K"));
      backend.getAppInfo.mockResolvedValue({ ...APP_INFO, hotkeyError: "⌃⌥K stopped working" });
      await record("KeyK", "˚");
      expect(shown()?.textContent).toContain("⌃⌥K stopped working");
    });
  });

  it("History refetches on mount and on focusCompleted / focusInterrupted only", async () => {
    const record: SessionRecord = {
      id: "s1",
      kind: "focus",
      taskLabel: "Read",
      startedAt: "2026-11-04T09:00:00Z",
      endedAt: "2026-11-04T09:25:00Z",
      plannedMs: 1_500_000,
      activeMs: 1_500_000,
      outcome: "completed",
      endReason: "completed",
      endedUtcOffsetMinutes: 0,
    };
    const backend = new FakeBackend();
    backend.getHistory.mockResolvedValue([record] as never);
    await mount(backend, "#history");
    expect(backend.getHistory).toHaveBeenCalledTimes(1);
    expect(backend.getHistory).toHaveBeenCalledWith(50);
    expect(screen.getByText("Read")).toBeTruthy();
    expect(screen.getByText("25 min")).toBeTruthy();
    act(() => backend.emit("transition", { type: "paused", sessionId: "s", kind: "focus", reason: "user" }));
    expect(backend.getHistory).toHaveBeenCalledTimes(1);
    act(() => backend.emit("transition", { type: "focusCompleted", record, award: null, longSession: false }));
    act(() => backend.emit("transition", { type: "focusInterrupted", record, award: null }));
    await act(flush);
    expect(backend.getHistory).toHaveBeenCalledTimes(3);
  });

  it("Companion tab picks palettes and shows locked unlocks", async () => {
    const backend = new FakeBackend();
    backend.snapshot = makeSnapshot({ friendship: { totalPoints: 200, unlocked: ["base", "wave", "stretch", "umbrella", "evening"], next: null } });
    await mount(backend, "#companion");
    fireEvent.click(screen.getByRole("radio", { name: /Evening light/ }));
    await act(flush);
    expect(backend.updateSettings).toHaveBeenCalledWith({ palette: "evening" });
    expect(screen.getAllByText("700 pts").length).toBeGreaterThan(0); // autumn, still locked
    expect(screen.getByText("/packs")).toBeTruthy();
  });

  it("gallery replays say what the companion will do under reduced motion", async () => {
    const backend = new FakeBackend();
    backend.pack = makePack(["idle", "wave"]);
    const base = makeSnapshot({ friendship: { totalPoints: 30, unlocked: ["base", "wave"], next: null } });
    backend.snapshot = { ...base, settings: { ...base.settings, reducedMotion: true } };
    await mount(backend, "#companion");
    fireEvent.click(screen.getByRole("button", { name: "Replay" }));
    await act(flush);
    expect(backend.replayAnimation).toHaveBeenCalledWith("wave");
    expect(screen.getByText(/Reduced motion is on, so “A small wave” stays still/)).toBeTruthy();
    expect(replayMessage("A small wave", false)).toBe("Playing “A small wave” on the companion.");
  });

  it("friendship progress runs from the last threshold to the next", () => {
    expect(friendshipProgress(30, UNLOCKS, 60)).toBeCloseTo(5 / 35);
    expect(friendshipProgress(25, UNLOCKS, 60)).toBe(0);
    expect(friendshipProgress(30000, UNLOCKS, null)).toBe(1);
  });
});
