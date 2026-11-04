// Small building blocks for the settings window (a normal, focusable window:
// regular buttons and inputs are fine here, unlike the companion).

import { useId, type ReactNode } from "react";
import { Glyph, type GlyphName } from "../ui/glyphs";

export function Section({ title, aside, children }: { title: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="st-section">
      <div className="st-section__head">
        <h2 className="st-section__title">{title}</h2>
        {aside}
      </div>
      <div className="st-card">{children}</div>
    </section>
  );
}

export function Row({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children?: ReactNode }) {
  return (
    <div className="st-row">
      <div className="st-row__text">
        <div className="st-row__label">{label}</div>
        {hint ? <div className="st-row__hint">{hint}</div> : null}
      </div>
      {children ? <div className="st-row__control">{children}</div> : null}
    </div>
  );
}

export function Toggle({
  label,
  hint,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  hint?: ReactNode;
  checked: boolean;
  disabled?: boolean;
  onChange: (v: boolean) => void;
}) {
  const id = useId();
  return (
    <div className="st-row">
      <div className="st-row__text">
        <label className="st-row__label" htmlFor={id}>
          {label}
        </label>
        {hint ? <div className="st-row__hint">{hint}</div> : null}
      </div>
      <div className="st-row__control">
        <input
          id={id}
          type="checkbox"
          role="switch"
          className="st-switch"
          checked={checked}
          disabled={disabled}
          onChange={(e) => onChange(e.target.checked)}
        />
      </div>
    </div>
  );
}

export function Button({
  children,
  icon,
  variant = "secondary",
  disabled,
  onClick,
  type = "button",
  label,
}: {
  children?: ReactNode;
  icon?: GlyphName;
  variant?: "primary" | "secondary" | "quiet" | "danger";
  disabled?: boolean;
  onClick?: () => void;
  type?: "button" | "submit";
  label?: string;
}) {
  return (
    <button type={type} className={`st-btn st-btn--${variant}`} disabled={disabled} onClick={onClick} aria-label={label}>
      {icon ? <Glyph name={icon} size={12} /> : null}
      {children}
    </button>
  );
}

export function Message({ tone = "info", children }: { tone?: "info" | "error" | "ok"; children: ReactNode }) {
  return (
    <p className={`st-msg st-msg--${tone}`} role={tone === "error" ? "alert" : "status"}>
      {children}
    </p>
  );
}
