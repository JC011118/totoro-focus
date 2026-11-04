// Frame stepping for sprites. One of the two renderer files allowed to read a
// clock (performance.now): it schedules a wake-up only at frame boundaries
// (a few per second, not every display frame) and stops entirely while the
// page is hidden, so an idle companion stays cheap. It never touches timer state.

import { useEffect, useEffectEvent, useState } from "react";
import { frameAt } from "./spriteMath";

export type SpriteMode = "loop" | "once" | "still";

export interface SpriteClockOptions {
  durations: readonly number[];
  mode: SpriteMode;
  /** Frame shown in "still" mode. */
  stillFrame: number;
  /** Cap for "once" (a flourish never runs longer than this). */
  maxMs?: number;
  /** Called once when a "once" animation finishes or hits the cap. */
  onEnd?: () => void;
}

/** Returns the frame to show; restart it by remounting (change the React key). */
export function useSpriteClock({ durations, mode, stillFrame, maxMs = Infinity, onEnd }: SpriteClockOptions): number {
  const [frame, setFrame] = useState(0);
  const ended = useEffectEvent(() => onEnd?.());
  const signature = durations.join(",");

  useEffect(() => {
    if (mode === "still") return;
    const list = signature.split(",").map(Number);
    const loop = mode === "loop";
    let timer: ReturnType<typeof setTimeout> | undefined;
    // Elapsed time counts only while the page is visible.
    let elapsedBefore = 0;
    let startedAt = performance.now();
    let running = !document.hidden;
    let finished = false;

    const elapsedNow = () => elapsedBefore + (running ? performance.now() - startedAt : 0);
    const step = () => {
      const r = frameAt(list, elapsedNow(), loop, maxMs);
      setFrame(r.frame);
      if (r.done) {
        if (!finished) {
          finished = true;
          ended();
        }
        return;
      }
      if (running && Number.isFinite(r.nextIn)) timer = setTimeout(step, r.nextIn);
    };
    const onVisibility = () => {
      if (finished) return;
      if (document.hidden && running) {
        clearTimeout(timer);
        elapsedBefore += performance.now() - startedAt;
        running = false;
      } else if (!document.hidden && !running) {
        startedAt = performance.now();
        running = true;
        step();
      }
    };

    timer = setTimeout(step, 0);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [signature, mode, maxMs]);

  return mode === "still" ? stillFrame : frame;
}
