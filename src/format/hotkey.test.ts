import { describe, expect, it } from "vitest";
import { DEFAULT_HOTKEY, displayAccelerator, keyFromCode, recordAccelerator, type KeyLike } from "./hotkey";

const key = (code: string, mods: Partial<Omit<KeyLike, "code">> = {}): KeyLike => ({
  code,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

describe("recordAccelerator (issues #7)", () => {
  it("records ⌃⌥T from KeyboardEvent.code, ignoring the option-mangled key", () => {
    // On macOS ⌥T reports key "†"; only .code matters.
    const e = { ...key("KeyT", { ctrlKey: true, altKey: true }), key: "†" };
    expect(recordAccelerator(e)).toEqual({ kind: "accelerator", accelerator: "Ctrl+Alt+T" });
  });

  it("emits modifiers in canonical Ctrl+Alt+Shift+Super order", () => {
    expect(recordAccelerator(key("KeyK", { metaKey: true, shiftKey: true, altKey: true, ctrlKey: true }))).toEqual({
      kind: "accelerator",
      accelerator: "Ctrl+Alt+Shift+Super+K",
    });
    expect(recordAccelerator(key("Digit5", { metaKey: true, shiftKey: true }))).toEqual({
      kind: "accelerator",
      accelerator: "Shift+Super+5",
    });
  });

  it("maps the supported codes", () => {
    expect(keyFromCode("KeyA")).toBe("A");
    expect(keyFromCode("Digit0")).toBe("0");
    expect(keyFromCode("F1")).toBe("F1");
    expect(keyFromCode("F20")).toBe("F20");
    expect(keyFromCode("F21")).toBeNull();
    for (const c of ["Space", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Minus", "Equal", "Comma", "Period", "Slash", "Semicolon", "Quote", "BracketLeft", "BracketRight", "Backquote", "Backslash"]) {
      expect(keyFromCode(c)).toBe(c);
    }
    expect(keyFromCode("Enter")).toBeNull();
    expect(keyFromCode("Numpad1")).toBeNull();
    expect(keyFromCode("IntlBackslash")).toBeNull();
  });

  it("requires Ctrl, Alt or Super, except for F-keys", () => {
    expect(recordAccelerator(key("KeyT")).kind).toBe("invalid");
    expect(recordAccelerator(key("KeyT", { shiftKey: true })).kind).toBe("invalid");
    expect(recordAccelerator(key("F5"))).toEqual({ kind: "accelerator", accelerator: "F5" });
    expect(recordAccelerator(key("F5", { shiftKey: true }))).toEqual({ kind: "accelerator", accelerator: "Shift+F5" });
    expect(recordAccelerator(key("Space", { altKey: true }))).toEqual({ kind: "accelerator", accelerator: "Alt+Space" });
  });

  it("treats bare Esc as cancel and bare Backspace/Delete as clear", () => {
    expect(recordAccelerator(key("Escape"))).toEqual({ kind: "cancel" });
    expect(recordAccelerator(key("Backspace"))).toEqual({ kind: "clear" });
    expect(recordAccelerator(key("Delete"))).toEqual({ kind: "clear" });
    // With modifiers they are just unsupported keys.
    expect(recordAccelerator(key("Escape", { ctrlKey: true })).kind).toBe("invalid");
    expect(recordAccelerator(key("Backspace", { metaKey: true })).kind).toBe("invalid");
  });

  it("reports held modifiers while waiting for a key", () => {
    expect(recordAccelerator(key("ControlLeft", { ctrlKey: true }))).toEqual({ kind: "pending", display: "⌃" });
    expect(recordAccelerator(key("AltRight", { ctrlKey: true, altKey: true }))).toEqual({ kind: "pending", display: "⌃⌥" });
    expect(recordAccelerator(key("MetaLeft", { metaKey: true, shiftKey: true }))).toEqual({ kind: "pending", display: "⇧⌘" });
    expect(recordAccelerator(key("ShiftLeft"))).toEqual({ kind: "pending", display: "" });
  });
});

describe("displayAccelerator", () => {
  it("shows the macOS ⌃⌥⇧⌘ form", () => {
    expect(displayAccelerator(DEFAULT_HOTKEY)).toBe("⌃⌥T");
    expect(displayAccelerator("Ctrl+Alt+Shift+Super+K")).toBe("⌃⌥⇧⌘K");
    expect(displayAccelerator("Super+Shift+Digit1")).toBe("⇧⌘1");
    expect(displayAccelerator("Alt+Space")).toBe("⌥Space");
    expect(displayAccelerator("Ctrl+ArrowUp")).toBe("⌃↑");
    expect(displayAccelerator("Control+Option+KeyT")).toBe("⌃⌥T");
    expect(displayAccelerator("CmdOrCtrl+Comma")).toBe("⌘,");
    expect(displayAccelerator("F5")).toBe("F5");
  });

  it("shows nothing for a disabled hotkey", () => {
    expect(displayAccelerator(null)).toBe("");
    expect(displayAccelerator("")).toBe("");
  });

  it("round-trips every recorded accelerator", () => {
    const r = recordAccelerator(key("Slash", { ctrlKey: true, metaKey: true }));
    expect(r).toEqual({ kind: "accelerator", accelerator: "Ctrl+Super+Slash" });
    if (r.kind === "accelerator") expect(displayAccelerator(r.accelerator)).toBe("⌃⌘/");
  });
});
