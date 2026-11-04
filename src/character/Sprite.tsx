import type { Rendering } from "../ipc/bindings/Rendering";
import type { ResolvedAnimation } from "../ipc/bindings/ResolvedAnimation";
import {
  backgroundPosition,
  backgroundSize,
  cssUrl,
  fitFrame,
  frameDurations,
  imageRendering,
  stillFrameOf,
} from "./spriteMath";
import { useSpriteClock, type SpriteMode } from "./useSpriteClock";

export interface SpriteProps {
  anim: ResolvedAnimation;
  rendering: Rendering;
  /** Box the frame is fitted into (aspect preserved). */
  width: number;
  height: number;
  mode: SpriteMode;
  maxMs?: number;
  onEnd?: () => void;
}

/** A sprite-sheet frame drawn as a CSS background (never inline SVG, SPEC §7). */
export function Sprite({ anim, rendering, width, height, mode, maxMs, onEnd }: SpriteProps) {
  const frame = useSpriteClock({
    durations: frameDurations(anim),
    mode,
    stillFrame: stillFrameOf(anim),
    maxMs,
    onEnd,
  });
  const box = fitFrame(anim.frameWidth, anim.frameHeight, width, height);
  return (
    <div
      className="sprite"
      aria-hidden="true"
      data-anim={anim.key}
      data-frame={frame}
      style={{
        width: box.width,
        height: box.height,
        backgroundImage: cssUrl(anim.url),
        backgroundRepeat: "no-repeat",
        backgroundSize: backgroundSize(anim),
        backgroundPosition: backgroundPosition(anim, frame),
        imageRendering: imageRendering(rendering, box.width, anim.frameWidth),
      }}
    />
  );
}
