// Reusable timer readout: progress ring (with any content inside, e.g. the
// character) and the mm:ss countdown. It renders the backend's label verbatim
// and never computes or interpolates time.

import type { ReactNode } from "react";
import type { Phase } from "../ipc/bindings/Phase";
import type { SessionView } from "../ipc/bindings/SessionView";
import { ProgressRing, type RingTone } from "./ProgressRing";

/** Remaining share of the session (1 → full ring, 0 → empty), from backend fields only. */
export function ringFraction(session: SessionView | null): number {
  if (!session || session.plannedMs <= 0) return 0;
  return Math.min(1, Math.max(0, session.remainingMs / session.plannedMs));
}

/**
 * Whether the ring should ease between one-second snapshots. Easing a ring that
 * changes every second keeps WebKit repainting for the whole session, which an
 * all-day companion must not do; it is only worth it when a one-second step is
 * visible (at least 1 px of arc, i.e. very short sessions). Breaks and focus
 * sessions of normal length step without a transition, invisibly.
 */
export function ringNeedsEasing(session: SessionView | null, size: number, stroke: number): boolean {
  if (!session || session.plannedMs <= 0) return false;
  const circumference = Math.PI * Math.max(0, size - stroke);
  const stepPx = circumference / (session.plannedMs / 1000);
  return stepPx >= 1;
}

export function ringTone(phase: Phase, session: SessionView | null, awaitingAfterFocus: boolean): RingTone {
  if (session?.status === "paused") return "paused";
  if (phase === "break") return "break";
  if (phase === "focus") return "focus";
  if (awaitingAfterFocus) return "done";
  return "idle";
}

export function Countdown({ label, tone }: { label: string; tone: RingTone }) {
  // Wide labels (100:00 and up) get a slightly smaller size so they always fit.
  const wide = label.length > 5;
  return (
    <span className={`countdown countdown--${tone}${wide ? " countdown--wide" : ""}`} role="timer" aria-live="off">
      {label}
    </span>
  );
}

export interface TimerViewProps {
  phase: Phase;
  session: SessionView | null;
  /** awaitingChoice after a completed focus: the ring shows closed, in the "done" tone. */
  awaitingAfterFocus?: boolean;
  size: number;
  stroke?: number;
  reducedMotion: boolean;
  /** Drawn inside the ring (the character). */
  children?: ReactNode;
  /** Shown beside the ring, under the countdown (status line, controls). */
  aside?: ReactNode;
  /** Shown instead of the countdown when there is no session (e.g. a greeting). */
  heading?: ReactNode;
}

export function TimerView({
  phase,
  session,
  awaitingAfterFocus = false,
  size,
  stroke = 6,
  reducedMotion,
  children,
  aside,
  heading,
}: TimerViewProps) {
  const tone = ringTone(phase, session, awaitingAfterFocus);
  const fraction = session ? ringFraction(session) : awaitingAfterFocus ? 1 : 0;
  return (
    <div className={`timer-view timer-view--${tone}`}>
      <ProgressRing fraction={fraction} size={size} stroke={stroke} tone={tone} animate={!reducedMotion && ringNeedsEasing(session, size, stroke)}>
        {children}
      </ProgressRing>
      <div className="timer-view__aside">
        {session ? <Countdown label={session.remainingLabel} tone={tone} /> : heading}
        {aside}
      </div>
    </div>
  );
}
