// Hotkey recording → canonical accelerator (frontend-ipc-petdex-issues #7,
// SPEC §8.2). Keys come from KeyboardEvent.code, never .key: on macOS ⌥T
// reports key "†", which cannot be registered.

export const DEFAULT_HOTKEY = "Ctrl+Alt+T";

/** The KeyboardEvent fields the recorder reads. */
export interface KeyLike {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type RecordResult =
  | { kind: "accelerator"; accelerator: string }
  /** Esc: stop recording, keep the current binding. */
  | { kind: "cancel" }
  /** Backspace/Delete: disable the hotkey. */
  | { kind: "clear" }
  /** Only modifiers are held so far; `display` shows them (e.g. "⌃⌥"). */
  | { kind: "pending"; display: string }
  | { kind: "invalid"; reason: string };

const MODIFIER_CODES = new Set([
  "ShiftLeft",
  "ShiftRight",
  "ControlLeft",
  "ControlRight",
  "AltLeft",
  "AltRight",
  "MetaLeft",
  "MetaRight",
  "OSLeft",
  "OSRight",
  "CapsLock",
  "Fn",
  "FnLock",
]);

const NAMED_CODES = new Set([
  "Space",
  "ArrowUp",
  "ArrowDown",
  "ArrowLeft",
  "ArrowRight",
  "Minus",
  "Equal",
  "Comma",
  "Period",
  "Slash",
  "Semicolon",
  "Quote",
  "BracketLeft",
  "BracketRight",
  "Backquote",
  "Backslash",
]);

const F_KEY = /^F([1-9]|1[0-9]|20)$/;

/** KeyboardEvent.code → accelerator key token, or null if unsupported. */
export function keyFromCode(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (F_KEY.test(code)) return code;
  if (NAMED_CODES.has(code)) return code;
  return null;
}

interface Mods {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  super: boolean;
}

const mods = (e: KeyLike): Mods => ({ ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, super: e.metaKey });

function modifierGlyphs(m: Mods): string {
  return `${m.ctrl ? "⌃" : ""}${m.alt ? "⌥" : ""}${m.shift ? "⇧" : ""}${m.super ? "⌘" : ""}`;
}

export function recordAccelerator(e: KeyLike): RecordResult {
  const m = mods(e);
  if (MODIFIER_CODES.has(e.code)) return { kind: "pending", display: modifierGlyphs(m) };
  const bare = !m.ctrl && !m.alt && !m.shift && !m.super;
  if (e.code === "Escape" && bare) return { kind: "cancel" };
  if ((e.code === "Backspace" || e.code === "Delete") && bare) return { kind: "clear" };
  const key = keyFromCode(e.code);
  if (key === null) return { kind: "invalid", reason: "That key can't be part of a shortcut." };
  if (!F_KEY.test(key) && !(m.ctrl || m.alt || m.super)) {
    return { kind: "invalid", reason: "Include ⌃ Control, ⌥ Option or ⌘ Command." };
  }
  const parts: string[] = [];
  if (m.ctrl) parts.push("Ctrl");
  if (m.alt) parts.push("Alt");
  if (m.shift) parts.push("Shift");
  if (m.super) parts.push("Super");
  parts.push(key);
  return { kind: "accelerator", accelerator: parts.join("+") };
}

const KEY_GLYPHS: Record<string, string> = {
  SPACE: "Space",
  ARROWUP: "↑",
  UP: "↑",
  ARROWDOWN: "↓",
  DOWN: "↓",
  ARROWLEFT: "←",
  LEFT: "←",
  ARROWRIGHT: "→",
  RIGHT: "→",
  MINUS: "-",
  EQUAL: "=",
  COMMA: ",",
  PERIOD: ".",
  SLASH: "/",
  SEMICOLON: ";",
  QUOTE: "'",
  BRACKETLEFT: "[",
  BRACKETRIGHT: "]",
  BACKQUOTE: "`",
  BACKSLASH: "\\",
  ENTER: "↩",
  TAB: "⇥",
  BACKSPACE: "⌫",
  DELETE: "⌦",
  ESCAPE: "⎋",
};

function displayKey(token: string): string {
  const u = token.toUpperCase();
  if (/^KEY[A-Z]$/.test(u)) return u.slice(3);
  if (/^DIGIT[0-9]$/.test(u)) return u.slice(5);
  if (/^F\d{1,2}$/.test(u)) return u;
  if (u.length === 1) return u;
  return KEY_GLYPHS[u] ?? token;
}

/**
 * Accelerator (canonical "Ctrl+Alt+Shift+Super+K", or any order / alias the
 * shell may echo) → macOS display form, modifiers in ⌃⌥⇧⌘ order: "⌃⌥T".
 */
export function displayAccelerator(accelerator: string | null | undefined): string {
  if (!accelerator) return "";
  const m: Mods = { ctrl: false, alt: false, shift: false, super: false };
  let key = "";
  for (const raw of accelerator.split("+")) {
    const t = raw.trim();
    if (!t) continue;
    switch (t.toLowerCase()) {
      case "ctrl":
      case "control":
        m.ctrl = true;
        break;
      case "alt":
      case "option":
        m.alt = true;
        break;
      case "shift":
        m.shift = true;
        break;
      case "super":
      case "cmd":
      case "command":
      case "meta":
      case "cmdorctrl":
      case "commandorcontrol":
        m.super = true;
        break;
      default:
        key = displayKey(t);
    }
  }
  return `${modifierGlyphs(m)}${key}`;
}
