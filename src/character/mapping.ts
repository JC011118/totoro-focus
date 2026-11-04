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
 * Ids unlocked as of `t`. The shell emits a transition before the snapshot of
 * the same call (SPEC §4.9), so the renderer's cached `friendship.unlocked`
 * misses whatever this session's award just unlocked; the award lists it.
 */
export function unlockedAsOf(t: Transition, unlocked: readonly string[]): Set<string> {
  const have = new Set(unlocked);
  if (t.type === "focusCompleted" || t.type === "focusInterrupted") {
    for (const id of t.award?.newlyUnlocked ?? []) have.add(id);
  }
  return have;
}

/**
 * The flourish keys to play for `t`: only ids that are unlocked AND resolve in
 * the pack, never under reduced motion. focusCompleted plays `long_session`
 * (long sessions) or else `celebrate`, then `acorn_gift`. `unlocked` may be the
 * pre-award set: ids in the transition's own award count as unlocked.
 */
export function flourishesFor(
  t: Transition,
  unlocked: readonly string[],
  pack: ResolvedPack | null | undefined,
  reducedMotion: boolean,
): string[] {
  if (reducedMotion) return [];
  const have = unlockedAsOf(t, unlocked);
  const out: string[] = [];
  for (const options of candidatesFor(t)) {
    const pick = options.find((k) => have.has(k) && resolvesIn(pack, k));
    if (pick) out.push(pick);
  }
  return out;
}

/**
 * The moment a queued one-shot belongs to. A flourish plays at its transition
 * or not at all: once that moment has passed it is dropped, never deferred
 * (e.g. a focusStarted wave queued while the companion was hidden must not
 * play when the companion is shown again mid-session).
 */
export type ShotScope =
  /** focusStarted / breakStarted of this session. */
  | { kind: "sessionStart"; sessionId: string }
  /** focusCompleted of this record. */
  | { kind: "focusDone"; recordId: string }
  /** breakFinished. */
  | { kind: "breakDone" }
  /** A replay (focus://play): any time except running focus (the shell refuses it then). */
  | { kind: "replay" }
  /** A character click. */
  | { kind: "react" };

/** A start flourish (wave, stretch) is current only during its session's first seconds. */
export const START_SHOT_WINDOW_MS = FLOURISH_MAX_MS;

/** The scope of `t`'s flourishes (null: the transition has none). */
export function scopeOf(t: Transition): ShotScope | null {
  switch (t.type) {
    case "focusStarted":
    case "breakStarted":
      return { kind: "sessionStart", sessionId: t.sessionId };
    case "focusCompleted":
      return { kind: "focusDone", recordId: t.record.id };
    case "breakFinished":
      return { kind: "breakDone" };
    default:
      return null;
  }
}

export type ScopeInput = MappingInput & Pick<Snapshot, "companionVisible">;

/**
 * Whether a one-shot of `scope` may still play over snapshot `s`. Callers judge
 * a one-shot only against snapshots newer than its arrival, because the shell
 * emits a transition before the snapshot of its own call (SPEC §4.9). Nothing
 * survives a snapshot that reports the companion hidden.
 */
export function scopeIsCurrent(scope: ShotScope, s: ScopeInput): boolean {
  if (!s.companionVisible) return false;
  switch (scope.kind) {
    case "sessionStart":
      return (
        s.session?.id === scope.sessionId && s.session.status === "running" && s.session.activeMs < START_SHOT_WINDOW_MS
      );
    case "focusDone":
      return (
        s.phase === "awaitingChoice" &&
        s.awaiting?.after === "focus" &&
        (s.awaiting.record?.id ?? scope.recordId) === scope.recordId
      );
    case "breakDone":
      return s.phase === "awaitingChoice" && s.awaiting?.after === "break";
    case "replay":
      return !(s.phase === "focus" && s.session?.status === "running");
    case "react":
      return true;
  }
}

/** Keys that blink may briefly replace (SPEC §7.1). */
export function canBlinkOver(baseKey: string): boolean {
  return baseKey === "idle" || baseKey === "focus";
}
