import { describe, expect, it } from "vitest";
import { EVENTS } from "./events";
import { errorMessage, isCommandError, toBackendError } from "./errors";

describe("errors", () => {
  it("recognizes CommandError payloads", () => {
    expect(isCommandError({ code: "noSession" })).toBe(true);
    expect(isCommandError({ code: "invalidMinutes", min: 1, max: 180 })).toBe(true);
    expect(isCommandError({ code: "nope" })).toBe(false);
    expect(isCommandError("noSession")).toBe(false);
    expect(isCommandError(null)).toBe(false);
  });

  it("normalizes anything else to unknown", () => {
    expect(toBackendError("command start_focus not allowed")).toEqual({ code: "unknown", message: "command start_focus not allowed" });
    expect(toBackendError(new Error("boom"))).toEqual({ code: "unknown", message: "boom" });
    expect(toBackendError(42)).toEqual({ code: "unknown", message: "" });
  });

  it("writes calm messages", () => {
    expect(errorMessage({ code: "invalidMinutes", min: 1, max: 180 })).toBe("Choose between 1 and 180 minutes.");
    expect(errorMessage({ code: "focusRunning" })).toBe("Replays wait until this focus session ends.");
    expect(errorMessage({ code: "invalidSettings", reason: "shortcut needs a modifier" })).toBe("Shortcut needs a modifier.");
    expect(errorMessage({ code: "packUnavailable", id: "totoro", reason: "missing pack.json" })).toBe(
      "“totoro” can't be used: missing pack.json",
    );
    expect(errorMessage(undefined)).toBe("Something didn't go through. Please try again.");
    for (const code of ["sessionActive", "noSession", "notAwaitingChoice", "notUnlocked"] as const) {
      const m = errorMessage(code === "notUnlocked" ? { code, id: "x" } : { code });
      expect(m.length).toBeGreaterThan(0);
      expect(m).not.toMatch(/fail|wrong|error/i);
    }
  });

  it("mirrors focus_core::events names", () => {
    expect(EVENTS).toEqual({
      snapshot: "focus://snapshot",
      transition: "focus://transition",
      packChanged: "focus://pack-changed",
      play: "focus://play",
      openTab: "focus://open-tab",
      openPicker: "focus://open-picker",
    });
  });
});
