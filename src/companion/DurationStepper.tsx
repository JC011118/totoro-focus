// Custom duration without typing (the companion has no inputs): − / +
// buttons with press-and-hold repeat, and the scroll wheel. A single click
// steps exactly 1 minute, so every value 1–180 is reachable; holding and the
// wheel use the coarse grid (±1 below 5 min, ±5 from 5 up, src/format/minutes.ts)
// so long distances stay quick. Typed minutes live in Settings → Start.

import { useRef } from "react";
import type { Limits } from "../ipc/bindings/Limits";
import { clampMinutes, stepMinutes } from "../format/minutes";
import { HoldButton, WidgetButton } from "./WidgetButton";

const WHEEL_STEP = 40;

export function DurationStepper({
  value,
  limits,
  onChange,
  onStart,
  onBack,
  disabled,
}: {
  value: number;
  limits: Limits;
  /** Receives an updater so press-and-hold repeats always step from the latest value. */
  onChange: (update: (v: number) => number) => void;
  onStart: () => void;
  onBack: () => void;
  disabled?: boolean;
}) {
  const wheel = useRef(0);
  const step = (dir: 1 | -1, coarse: boolean) =>
    onChange((v) => (coarse ? stepMinutes(v, dir, limits) : clampMinutes(v + dir, limits)));
  return (
    <div className="cw-stepper">
      <div
        className="cw-stepper__row"
        onWheel={(e) => {
          wheel.current += e.deltaY;
          if (Math.abs(wheel.current) < WHEEL_STEP) return;
          step(wheel.current < 0 ? 1 : -1, true);
          wheel.current = 0;
        }}
      >
        <HoldButton icon="minus" label="Fewer minutes" onStep={(repeat) => step(-1, repeat)} disabled={disabled || value <= limits.minMinutes} />
        <span className="cw-stepper__value" aria-live="polite">
          <span className="cw-stepper__num">{value}</span>
          <span className="cw-stepper__unit">min</span>
        </span>
        <HoldButton icon="plus" label="More minutes" onStep={(repeat) => step(1, repeat)} disabled={disabled || value >= limits.maxMinutes} />
      </div>
      <WidgetButton variant="primary" icon="play" wide onPress={onStart} disabled={disabled}>
        Start {value}
      </WidgetButton>
      <WidgetButton variant="ghost" onPress={onBack} disabled={disabled} className="cw-stepper__back">
        Back
      </WidgetButton>
    </div>
  );
}
