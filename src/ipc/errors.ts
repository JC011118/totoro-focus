import type { CommandError } from "./bindings/CommandError";

/** A rejected backend call: a typed CommandError, or anything else (IPC failure). */
export type BackendError = CommandError | { code: "unknown"; message: string };

const CODES: ReadonlySet<string> = new Set<CommandError["code"]>([
  "sessionActive",
  "noSession",
  "notAwaitingChoice",
  "invalidMinutes",
  "invalidSettings",
  "focusRunning",
  "notUnlocked",
  "packUnavailable",
]);

export function isCommandError(e: unknown): e is CommandError {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    typeof (e as { code: unknown }).code === "string" &&
    CODES.has((e as { code: string }).code)
  );
}

export function toBackendError(e: unknown): BackendError {
  if (isCommandError(e)) return e;
  if (typeof e === "object" && e !== null && (e as { code?: unknown }).code === "unknown") {
    return e as BackendError;
  }
  if (typeof e === "string") return { code: "unknown", message: e };
  if (e instanceof Error) return { code: "unknown", message: e.message };
  return { code: "unknown", message: "" };
}

/** Calm, plain-language copy for an error. Never scolding. */
export function errorMessage(err: unknown): string {
  const e = toBackendError(err);
  switch (e.code) {
    case "sessionActive":
      return "A session is already going.";
    case "noSession":
      return "There's no session right now.";
    case "notAwaitingChoice":
      return "That choice was already made.";
    case "invalidMinutes":
      return `Choose between ${e.min} and ${e.max} minutes.`;
    case "invalidSettings":
      return e.reason ? sentence(e.reason) : "That setting couldn't be saved.";
    case "focusRunning":
      return "Replays wait until this focus session ends.";
    case "notUnlocked":
      return "That one isn't unlocked yet.";
    case "packUnavailable":
      return `“${e.id}” can't be used${e.reason ? `: ${e.reason}` : "."}`;
    case "unknown":
      return e.message ? sentence(e.message) : "Something didn't go through. Please try again.";
  }
}

function sentence(s: string): string {
  const t = s.trim();
  if (!t) return t;
  const first = t.charAt(0).toUpperCase() + t.slice(1);
  return /[.!?…]$/.test(first) ? first : `${first}.`;
}
