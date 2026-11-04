// Presentation queue for the companion: transition flourishes, replays
// (focus://play) and character-click reactions. Purely cosmetic: it never
// gates, delays or issues commands, and clicks earn nothing (no IPC).
//
// A one-shot plays at its moment or not at all (SPEC §7.1). Each queued shot
// carries the scope it belongs to and is dropped, never deferred, once a newer
// snapshot shows that moment has passed or the companion is hidden, and when
// the page itself is hidden. So nothing queued while the companion was away
// plays late, e.g. a session-start wave in the middle of focus.

import { useEffect, useRef, useState } from "react";
import type { OneShot } from "../character/CharacterView";
import { flourishesFor, scopeIsCurrent, scopeOf, type ScopeInput, type ShotScope } from "../character/mapping";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import type { Transition } from "../ipc/bindings/Transition";
import { useBackendEvent, useRuntime, useSnapshot } from "../state/BackendContext";
import { resolvesIn } from "../theme/choose";

const MAX_QUEUE = 4;
export const BUBBLE_MS = 1500;
const BUBBLES = ["hi!", "♪", "hm?", "hello"];

export interface QueuedShot extends OneShot {
  scope: ShotScope;
  /** `seq` of the newest applied snapshot when queued; only newer snapshots judge it. */
  afterSeq: number;
}

/**
 * Drops one-shots whose moment has passed (see `scopeIsCurrent`), and all of
 * them under reduced motion. Returns `queue` itself when nothing is dropped.
 */
export function pruneShots(
  queue: readonly QueuedShot[],
  snapshot: (ScopeInput & Pick<Snapshot, "seq">) | null,
  reducedMotion: boolean,
): readonly QueuedShot[] {
  if (queue.length === 0) return queue;
  if (reducedMotion) return [];
  if (!snapshot) return queue;
  const kept = queue.filter((e) => snapshot.seq <= e.afterSeq || scopeIsCurrent(e.scope, snapshot));
  return kept.length === queue.length ? queue : kept;
}

export interface CompanionMotion {
  oneShot: OneShot | null;
  onOneShotEnd: () => void;
  hopNonce: number;
  bubble: string | null;
  /** Character click → `react` (or the hop / a static bubble). */
  poke: () => void;
  /** Handle a transition (exported for tests; also wired to focus://transition). */
  onTransition: (t: Transition) => void;
  /** Handle a validated replay key (exported for tests; also wired to focus://play). */
  onPlay: (key: string) => void;
}

export function useCompanionMotion(opts: { pack: ResolvedPack | null; reducedMotion: boolean }): CompanionMotion {
  const { pack, reducedMotion } = opts;
  const { snapshots } = useRuntime();
  const snapshot = useSnapshot();
  const [queue, setQueue] = useState<readonly QueuedShot[]>([]);
  const [hopNonce, setHopNonce] = useState(0);
  const [bubble, setBubble] = useState<{ text: string; nonce: number } | null>(null);
  const counter = useRef(0);

  useEffect(() => {
    if (!bubble) return;
    const t = setTimeout(() => setBubble(null), BUBBLE_MS);
    return () => clearTimeout(t);
  }, [bubble]);

  // A hidden page freezes sprite clocks; drop what is in flight rather than resume it later.
  useEffect(() => {
    const onVisibility = () => {
      if (document.hidden) setQueue([]);
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => document.removeEventListener("visibilitychange", onVisibility);
  }, []);

  // Drop shots whose moment has passed as soon as a snapshot says so.
  const live = pruneShots(queue, snapshot, reducedMotion);
  if (live !== queue) setQueue((q) => pruneShots(q, snapshot, reducedMotion));

  const next = () => (counter.current += 1);
  /** The newest applied snapshot (may be ahead of this render). */
  const latest = () => snapshots.get();

  const sayHi = () => {
    const n = next();
    setBubble({ text: BUBBLES[n % BUBBLES.length] as string, nonce: n });
  };

  const poke = () => {
    if (reducedMotion) {
      sayHi();
    } else if (resolvesIn(pack, "react")) {
      const shot: QueuedShot = { key: "react", nonce: next(), scope: { kind: "react" }, afterSeq: latest()?.seq ?? 0 };
      setQueue((q) => [shot, ...q.slice(1)]);
    } else {
      setHopNonce(next());
    }
  };

  const onTransition = (t: Transition) => {
    const scope = scopeOf(t);
    if (!scope) return;
    const s = latest();
    // The transition's own snapshot (with any new unlock) arrives after it; flourishesFor adds the award.
    const keys = flourishesFor(t, s?.friendship.unlocked ?? [], pack, reducedMotion);
    if (keys.length === 0) return;
    const afterSeq = s?.seq ?? 0;
    const shots = keys.map((key): QueuedShot => ({ key, nonce: next(), scope, afterSeq }));
    setQueue((q) => [...q, ...shots].slice(0, MAX_QUEUE));
  };

  const onPlay = (key: string) => {
    if (key === "react") {
      poke();
      return;
    }
    // Replays use the flourish path, which never plays under reduced motion:
    // acknowledge with the same still bubble as a click instead.
    if (reducedMotion) {
      sayHi();
      return;
    }
    if (!resolvesIn(pack, key)) return;
    setQueue([{ key, nonce: next(), scope: { kind: "replay" }, afterSeq: latest()?.seq ?? 0 }]);
  };

  useBackendEvent("transition", onTransition);
  useBackendEvent("play", (p) => onPlay(p.key));

  const head = live[0] ?? null;
  return {
    oneShot: head,
    // Keyed to the current head, so a late "ended" can never drop a newer one-shot.
    onOneShotEnd: () => {
      if (head) setQueue((q) => (q[0]?.nonce === head.nonce ? q.slice(1) : q));
    },
    hopNonce,
    bubble: bubble?.text ?? null,
    poke,
    onTransition,
    onPlay,
  };
}
