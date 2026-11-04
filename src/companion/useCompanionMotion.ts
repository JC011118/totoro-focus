// Presentation queue for the companion: transition flourishes, replays
// (focus://play) and character-click reactions. Purely cosmetic: it never
// gates, delays or issues commands, and clicks earn nothing (no IPC).

import { useEffect, useRef, useState } from "react";
import type { OneShot } from "../character/CharacterView";
import { flourishesFor } from "../character/mapping";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Transition } from "../ipc/bindings/Transition";
import { useBackendEvent } from "../state/BackendContext";
import { resolvesIn } from "../theme/choose";

const MAX_QUEUE = 4;
export const BUBBLE_MS = 1500;
const BUBBLES = ["hi!", "♪", "hm?", "hello"];

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

export function useCompanionMotion(opts: {
  pack: ResolvedPack | null;
  unlocked: readonly string[];
  reducedMotion: boolean;
}): CompanionMotion {
  const { pack, unlocked, reducedMotion } = opts;
  const [queue, setQueue] = useState<OneShot[]>([]);
  const [hopNonce, setHopNonce] = useState(0);
  const [bubble, setBubble] = useState<{ text: string; nonce: number } | null>(null);
  const counter = useRef(0);

  useEffect(() => {
    if (!bubble) return;
    const t = setTimeout(() => setBubble(null), BUBBLE_MS);
    return () => clearTimeout(t);
  }, [bubble]);

  const next = () => (counter.current += 1);

  const poke = () => {
    const n = next();
    if (reducedMotion) {
      setBubble({ text: BUBBLES[n % BUBBLES.length] as string, nonce: n });
    } else if (resolvesIn(pack, "react")) {
      setQueue((q) => [{ key: "react", nonce: n }, ...q.slice(1)]);
    } else {
      setHopNonce(n);
    }
  };

  const onTransition = (t: Transition) => {
    const keys = flourishesFor(t, unlocked, pack, reducedMotion);
    if (keys.length === 0) return;
    setQueue((q) => [...q, ...keys.map((key) => ({ key, nonce: next() }))].slice(0, MAX_QUEUE));
  };

  const onPlay = (key: string) => {
    if (key === "react") {
      poke();
      return;
    }
    // Replays use the flourish path; under reduced motion CharacterView shows a still instead.
    if (!resolvesIn(pack, key)) return;
    const n = next();
    setQueue([{ key, nonce: n }]);
  };

  useBackendEvent("transition", onTransition);
  useBackendEvent("play", (p) => onPlay(p.key));

  const head = queue[0] ?? null;
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
