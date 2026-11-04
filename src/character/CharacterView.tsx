// Reusable character renderer (companion widget now, illustrated scene later).
// Renders from plain props; owns only presentation timers (blink, hop). It
// never issues commands.

import { useEffect, useEffectEvent, useRef, useState, type MouseEvent } from "react";
import type { ResolvedAnimation } from "../ipc/bindings/ResolvedAnimation";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import { canBlinkOver, FLOURISH_MAX_MS } from "./mapping";
import { Sprite } from "./Sprite";
import { StaticCompanion } from "./StaticCompanion";

export interface OneShot {
  key: string;
  /** Distinguishes repeated plays of the same key. */
  nonce: number;
}

export interface CharacterViewProps {
  pack: ResolvedPack | null;
  /** From `baseKeyFor` (mapping.ts). */
  baseKey: string;
  /** A flourish, replay or click reaction to play once over the base. */
  oneShot?: OneShot | null;
  onOneShotEnd?: () => void;
  /** Changing this (to a non-zero value) plays the 300 ms CSS hop (no `react` art). */
  hopNonce?: number;
  /** Static speech bubble (reduced-motion click acknowledgement). */
  bubble?: string | null;
  reducedMotion: boolean;
  /** Allow the occasional blink over idle/focus. */
  blink?: boolean;
  width: number;
  height: number;
  /** CSS filter from the palette (`characterFilter`). */
  filter?: string;
  label: string;
  /** When set, the character is a button (tabIndex -1: never takes keyboard focus). */
  onClick?: () => void;
  onContextMenu?: (e: MouseEvent) => void;
}

const BLINK_MIN_MS = 5000;
const BLINK_SPREAD_MS = 4000;

function anim(pack: ResolvedPack | null, key: string): ResolvedAnimation | null {
  return pack?.animations[key] ?? null;
}

export function CharacterView(props: CharacterViewProps) {
  const { pack, baseKey, oneShot, reducedMotion, width, height, filter, label } = props;
  const rendering = pack?.rendering ?? "smooth";
  const hopRef = useRef<HTMLSpanElement>(null);
  const oneShotEnded = useEffectEvent(() => props.onOneShotEnd?.());

  const shotAnim = oneShot ? anim(pack, oneShot.key) : null;
  // Base: the requested key, defensively falling back to idle (the shell already resolves fallbacks).
  const baseAnim = anim(pack, baseKey) ?? anim(pack, "idle");
  const blinkAnim = anim(pack, "blink");

  // One-shots without art end immediately, and so does every one-shot under
  // reduced motion (SPEC §7.1: no flourishes; the base key's still frame stays).
  const shotKey = oneShot ? `${oneShot.key}:${oneShot.nonce}` : null;
  const shotSkipped = oneShot !== null && oneShot !== undefined && (shotAnim === null || reducedMotion);
  useEffect(() => {
    if (shotKey !== null && shotSkipped) oneShotEnded();
  }, [shotKey, shotSkipped]);

  // Blink: a one-shot that replaces idle/focus every 5–9 s, skipped when there is no blink art.
  const canBlink = Boolean(props.blink) && !reducedMotion && blinkAnim !== null && !oneShot && canBlinkOver(baseKey);
  const [blinkNonce, setBlinkNonce] = useState(0);
  const [blinking, setBlinking] = useState(false);
  useEffect(() => {
    if (!canBlink || blinking) return;
    const t = setTimeout(
      () => {
        setBlinkNonce((n) => n + 1);
        setBlinking(true);
      },
      BLINK_MIN_MS + Math.random() * BLINK_SPREAD_MS,
    );
    return () => clearTimeout(t);
  }, [canBlink, blinking]);

  // Hop: a CSS-only 300 ms lift when a click has no `react` art.
  const hopNonce = props.hopNonce ?? 0;
  useEffect(() => {
    if (hopNonce === 0 || reducedMotion) return;
    const el = hopRef.current;
    el?.animate?.(
      [
        { transform: "translateY(0)" },
        { transform: "translateY(-4px)", offset: 0.4 },
        { transform: "translateY(0)" },
      ],
      { duration: 300, easing: "ease-out" },
    );
  }, [hopNonce, reducedMotion]);

  const spriteBox = { width, height };
  let content;
  if (oneShot && shotAnim && !reducedMotion) {
    content = (
      <Sprite
        key={`shot:${oneShot.key}:${oneShot.nonce}`}
        anim={shotAnim}
        rendering={rendering}
        {...spriteBox}
        mode="once"
        maxMs={FLOURISH_MAX_MS}
        onEnd={props.onOneShotEnd}
      />
    );
  } else if (canBlink && blinking && blinkAnim) {
    content = (
      <Sprite
        key={`blink:${blinkNonce}`}
        anim={blinkAnim}
        rendering={rendering}
        {...spriteBox}
        mode="once"
        maxMs={FLOURISH_MAX_MS}
        onEnd={() => setBlinking(false)}
      />
    );
  } else if (baseAnim) {
    content = (
      <Sprite
        key={`base:${baseKey}:${baseAnim.url}:${baseAnim.row}:${baseAnim.column}`}
        anim={baseAnim}
        rendering={rendering}
        {...spriteBox}
        mode={reducedMotion ? "still" : baseAnim.loop ? "loop" : "once"}
      />
    );
  } else {
    content = <StaticCompanion mood={baseKey} size={Math.min(width, height)} />;
  }

  const inner = (
    <>
      <span ref={hopRef} className="character__figure" style={filter && filter !== "none" ? { filter } : undefined}>
        {content}
      </span>
      {props.bubble ? (
        <span className="character__bubble" role="status">
          {props.bubble}
        </span>
      ) : null}
    </>
  );

  const style = { width, height };
  if (props.onClick) {
    return (
      <button
        type="button"
        tabIndex={-1}
        className="character"
        style={style}
        aria-label={label}
        data-base={baseKey}
        onClick={props.onClick}
        onContextMenu={props.onContextMenu}
      >
        {inner}
      </button>
    );
  }
  return (
    <div className="character" style={style} role="img" aria-label={label} data-base={baseKey}>
      {inner}
    </div>
  );
}
