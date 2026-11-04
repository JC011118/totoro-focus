import type { ReactNode } from "react";

export type RingTone = "focus" | "break" | "paused" | "done" | "idle";

export interface ProgressRingProps {
  /** 1 = full ring, 0 = empty. The caller passes remainingMs / plannedMs. */
  fraction: number;
  size: number;
  stroke: number;
  tone: RingTone;
  /** A 1 s linear transition between snapshots; off under reduced motion. */
  animate: boolean;
  children?: ReactNode;
}

/** SVG ring that shrinks clockwise from 12 o'clock as time is used. */
export function ProgressRing({ fraction, size, stroke, tone, animate, children }: ProgressRingProps) {
  const f = Number.isFinite(fraction) ? Math.min(1, Math.max(0, fraction)) : 0;
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - f);
  return (
    <div className={`ring ring--${tone}`} style={{ width: size, height: size }}>
      <svg className="ring__svg" width={size} height={size} viewBox={`0 0 ${size} ${size}`} aria-hidden="true">
        <circle className="ring__track" cx={size / 2} cy={size / 2} r={r} strokeWidth={stroke} fill="none" />
        {f > 0 && (
          <circle
            className={`ring__arc${animate ? " ring__arc--animate" : ""}`}
            cx={size / 2}
            cy={size / 2}
            r={r}
            strokeWidth={stroke}
            fill="none"
            strokeLinecap="round"
            strokeDasharray={`${c} ${c}`}
            strokeDashoffset={offset}
            transform={`rotate(-90 ${size / 2} ${size / 2})`}
            data-fraction={Math.round(f * 1000) / 1000}
          />
        )}
      </svg>
      <div className="ring__content">{children}</div>
    </div>
  );
}
