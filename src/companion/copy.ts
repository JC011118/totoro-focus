// Companion wording. Gentle and plain: nothing scolds, nothing judges.

import type { PauseReason } from "../ipc/bindings/PauseReason";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";

export function phaseLabel(s: Snapshot | null): string {
  if (!s) return "Hello";
  switch (s.phase) {
    case "idle":
      return "Ready";
    case "focus":
      return s.session?.status === "paused" ? "Paused" : "Focus";
    case "break":
      return s.session?.status === "paused" ? "Break · paused" : "Break";
    case "awaitingChoice":
      return s.awaiting?.after === "break" ? "Break over" : "Complete";
  }
}

export function pauseLine(reason: PauseReason | null | undefined): string {
  switch (reason) {
    case "suspend":
      return "Paused while your Mac slept";
    case "clockJump":
      return "Paused when the clock changed";
    case "quit":
      return "Paused when the app closed";
    case "restored":
      return "Paused · picked up where you left off";
    default:
      return "Paused · take your time";
  }
}

export function unlockLabel(unlocks: readonly UnlockDef[], id: string): string {
  return unlocks.find((u) => u.id === id)?.label ?? id.replace(/_/g, " ");
}

export function wholeMinutes(ms: number): number {
  return Math.floor(Math.max(0, ms) / 60_000);
}
