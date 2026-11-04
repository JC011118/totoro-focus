// Companion controls are plain <button>s with tabIndex=-1 (SPEC §8.1 rule 7):
// clicking one never moves keyboard focus out of the user's app. Every
// variant has a visible fill or outline, so it reads as a control without hover.

import { useEffect, useRef, type ReactNode } from "react";
import { Glyph, type GlyphName } from "../ui/glyphs";

export type WidgetButtonVariant = "primary" | "soft" | "outline" | "ghost" | "chip" | "icon";

export interface WidgetButtonProps {
  variant?: WidgetButtonVariant;
  icon?: GlyphName;
  children?: ReactNode;
  label?: string;
  onPress: () => void;
  disabled?: boolean;
  active?: boolean;
  className?: string;
  wide?: boolean;
}

export function WidgetButton({
  variant = "soft",
  icon,
  children,
  label,
  onPress,
  disabled,
  active,
  className,
  wide,
}: WidgetButtonProps) {
  return (
    <button
      type="button"
      tabIndex={-1}
      className={[
        "wbtn",
        `wbtn--${variant}`,
        active ? "wbtn--active" : "",
        wide ? "wbtn--wide" : "",
        className ?? "",
      ]
        .filter(Boolean)
        .join(" ")}
      aria-label={label}
      aria-pressed={active === undefined ? undefined : active}
      disabled={disabled}
      onClick={onPress}
    >
      {icon ? <Glyph name={icon} size={variant === "icon" ? 12 : 11} /> : null}
      {children !== undefined ? <span className="wbtn__text">{children}</span> : null}
    </button>
  );
}

const HOLD_DELAY_MS = 420;
const HOLD_REPEAT_MS = 110;

/**
 * A button that steps once on press and repeats while held (stepper − / +).
 * Pointer-driven, so it works in a non-key panel; click still works for
 * assistive technology.
 */
export function HoldButton({
  icon,
  label,
  onStep,
  disabled,
}: {
  icon: GlyphName;
  label: string;
  onStep: () => void;
  disabled?: boolean;
}) {
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pressed = useRef(false);

  const stop = () => {
    clearTimeout(timer.current);
    timer.current = undefined;
  };
  useEffect(() => stop, []);

  const repeat = () => {
    onStep();
    timer.current = setTimeout(repeat, HOLD_REPEAT_MS);
  };

  return (
    <button
      type="button"
      tabIndex={-1}
      className="wbtn wbtn--step"
      aria-label={label}
      disabled={disabled}
      onPointerDown={(e) => {
        if (e.button !== 0 || disabled) return;
        pressed.current = true;
        onStep();
        stop();
        timer.current = setTimeout(repeat, HOLD_DELAY_MS);
      }}
      onPointerUp={stop}
      onPointerLeave={stop}
      onPointerCancel={stop}
      onClick={() => {
        // Pointer presses already stepped; keyboard / AT activation steps here.
        if (pressed.current) pressed.current = false;
        else onStep();
      }}
    >
      <Glyph name={icon} size={12} />
    </button>
  );
}
