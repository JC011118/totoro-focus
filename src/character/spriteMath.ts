// SPEC §7 sprite math. Percentages only, so a scaled atlas renders the same
// cells as the original: only the frame aspect ratio matters.

import type { Rendering } from "../ipc/bindings/Rendering";
import type { ResolvedAnimation } from "../ipc/bindings/ResolvedAnimation";

export type SpriteGeometry = Pick<
  ResolvedAnimation,
  "frames" | "row" | "column" | "sheetColumns" | "sheetRows"
>;

const pct = (n: number) => `${Math.round(n * 10000) / 10000}%`;

/** `background-size: sheetColumns×100% sheetRows×100%`. */
export function backgroundSize(a: Pick<SpriteGeometry, "sheetColumns" | "sheetRows">): string {
  return `${pct(Math.max(1, a.sheetColumns) * 100)} ${pct(Math.max(1, a.sheetRows) * 100)}`;
}

/** `background-position: c/(sheetColumns−1)×100% row/(sheetRows−1)×100%` (0% when the count is 1). */
export function backgroundPosition(a: SpriteGeometry, frame: number): string {
  const f = Math.min(Math.max(0, Math.floor(frame)), Math.max(0, a.frames - 1));
  const c = a.column + f;
  const x = a.sheetColumns > 1 ? (c / (a.sheetColumns - 1)) * 100 : 0;
  const y = a.sheetRows > 1 ? (a.row / (a.sheetRows - 1)) * 100 : 0;
  return `${pct(x)} ${pct(y)}`;
}

/** Quoted CSS url() that tolerates quotes and backslashes in the URL. */
export function cssUrl(url: string): string {
  return `url("${url.replace(/["\\\n\r]/g, (ch) => `\\${ch.charCodeAt(0).toString(16)} `)}")`;
}

/** Pixelated sampling only when the pack asks for it and the sprite is not shrunk. */
export function imageRendering(rendering: Rendering, displayWidth: number, frameWidth: number): "pixelated" | "auto" {
  return rendering === "pixelated" && frameWidth > 0 && displayWidth / frameWidth >= 1 ? "pixelated" : "auto";
}

/** Largest box with the frame's aspect ratio that fits in maxW × maxH. */
export function fitFrame(frameWidth: number, frameHeight: number, maxW: number, maxH: number): { width: number; height: number } {
  if (frameWidth <= 0 || frameHeight <= 0) return { width: maxW, height: maxH };
  const scale = Math.min(maxW / frameWidth, maxH / frameHeight);
  return { width: Math.round(frameWidth * scale), height: Math.round(frameHeight * scale) };
}

/** Per-frame durations: frameDurationsMs when it has one entry per frame, else 1000/fps. */
export function frameDurations(a: Pick<ResolvedAnimation, "frames" | "fps" | "frameDurationsMs">): number[] {
  const n = Math.max(1, Math.floor(a.frames));
  const list = a.frameDurationsMs;
  if (list && list.length === n && list.every((d) => Number.isFinite(d) && d > 0)) return [...list];
  const each = a.fps > 0 && Number.isFinite(a.fps) ? 1000 / a.fps : 250;
  return Array.from({ length: n }, () => each);
}

export function stillFrameOf(a: Pick<ResolvedAnimation, "frames" | "stillFrame">): number {
  return Math.min(Math.max(0, Math.floor(a.stillFrame)), Math.max(0, a.frames - 1));
}

export interface FrameState {
  frame: number;
  /** A once-through animation reached its end (or the cap). */
  done: boolean;
  /** ms until the frame changes (Infinity when nothing will change). */
  nextIn: number;
}

/**
 * Which frame shows `elapsed` ms after start. Looping animations wrap; others
 * hold the last frame and report done. `maxMs` caps a one-shot (SPEC: 4 s).
 */
export function frameAt(durations: readonly number[], elapsed: number, loop: boolean, maxMs = Infinity): FrameState {
  const n = durations.length;
  if (n === 0) return { frame: 0, done: !loop, nextIn: Infinity };
  const total = durations.reduce((s, d) => s + d, 0);
  const last = n - 1;
  if (!loop && elapsed >= Math.min(total, maxMs)) {
    return { frame: elapsed >= total ? last : locate(durations, elapsed).frame, done: true, nextIn: Infinity };
  }
  if (loop && n === 1) return { frame: 0, done: false, nextIn: Infinity };
  const t = loop ? ((elapsed % total) + total) % total : Math.max(0, elapsed);
  const { frame, nextIn } = locate(durations, t);
  const capLeft = maxMs - elapsed;
  return { frame, done: false, nextIn: Math.max(1, Math.min(nextIn, capLeft)) };
}

function locate(durations: readonly number[], t: number): { frame: number; nextIn: number } {
  let acc = 0;
  for (let i = 0; i < durations.length; i++) {
    acc += durations[i] as number;
    if (t < acc) return { frame: i, nextIn: acc - t };
  }
  return { frame: durations.length - 1, nextIn: 1 };
}
