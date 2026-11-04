// SPEC §7.1: Snapshot → base animation key, Transition → one-shot flourishes.
// Presentation only: nothing here gates, delays or issues commands.

import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { Transition } from "../ipc/bindings/Transition";
import { resolvesIn } from "../theme/choose";

/** One-shots never run longer than this, whatever their frames say. */
export const FLOURISH_MAX_MS = 4000;

export type MappingInput = Pick<Snapshot, "phase" | "session" | "awaiting">;

export function baseKeyFor(s: MappingInput, idleVariant: string | null): string {
  switch (s.phase) {
    case "idle":
      return idleVariant ?? "idle";
    case "focus":
      return s.session?.status === "paused" ? "paused" : "focus";
    case "break":
      return s.session?.status === "paused" ? "paused" : "break";
    case "awaitingChoice":
      return s.awaiting?.after === "break" ? "idle" : "completed";
  }
}

/** Flourish candidates per transition, in play order; each entry is "first of" its options. */
function candidatesFor(t: Transition): string[][] {
  switch (t.type) {
    case "focusStarted":
      return [["wave"]];
    case "breakStarted":
      return [["stretch"]];
    case "focusCompleted":
      return [t.longSession ? ["long_session", "celebrate"] : ["celebrate"], ["acorn_gift"]];
    case "breakFinished":
      return [["hum"]];
    default:
      return [];
  }
}

/**
 * The flourish keys to play for `t`: only ids that are unlocked AND resolve in
 * the pack, never under reduced motion. focusCompleted plays `long_session`
 * (long sessions) or else `celebrate`, then `acorn_gift`.
 */
export function flourishesFor(
  t: Transition,
  unlocked: readonly string[],
  pack: ResolvedPack | null | undefined,
  reducedMotion: boolean,
): string[] {
  if (reducedMotion) return [];
  const have = new Set(unlocked);
  const out: string[] = [];
  for (const options of candidatesFor(t)) {
    const pick = options.find((k) => have.has(k) && resolvesIn(pack, k));
    if (pick) out.push(pick);
  }
  return out;
}

/** Keys that blink may briefly replace (SPEC §7.1). */
export function canBlinkOver(baseKey: string): boolean {
  return baseKey === "idle" || baseKey === "focus";
}
