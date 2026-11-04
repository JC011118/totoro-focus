// Small stroke glyphs (currentColor) for buttons. Decorative: buttons carry
// their own text or aria-label.

import type { ReactElement } from "react";

type GlyphName =
  | "play"
  | "pause"
  | "stop"
  | "plus"
  | "minus"
  | "dots"
  | "hide"
  | "pencil"
  | "cup"
  | "close"
  | "check"
  | "sparkle"
  | "grip"
  | "repeat"
  | "sliders"
  | "clock"
  | "leaf";

const PATHS: Record<GlyphName, ReactElement> = {
  play: <path d="M5 3.6v8.8a.6.6 0 0 0 .9.5l7-4.4a.6.6 0 0 0 0-1l-7-4.4a.6.6 0 0 0-.9.5Z" fill="currentColor" stroke="none" />,
  pause: (
    <g fill="currentColor" stroke="none">
      <rect x="4.2" y="3.4" width="2.6" height="9.2" rx=".9" />
      <rect x="9.2" y="3.4" width="2.6" height="9.2" rx=".9" />
    </g>
  ),
  stop: <rect x="4" y="4" width="8" height="8" rx="1.8" fill="currentColor" stroke="none" />,
  plus: <path d="M8 3.5v9M3.5 8h9" />,
  minus: <path d="M3.5 8h9" />,
  dots: (
    <g fill="currentColor" stroke="none">
      <circle cx="3.6" cy="8" r="1.3" />
      <circle cx="8" cy="8" r="1.3" />
      <circle cx="12.4" cy="8" r="1.3" />
    </g>
  ),
  hide: <path d="M4 8.5h8" />,
  pencil: <path d="M10.6 3.2l2.2 2.2-6.9 6.9-2.9.7.7-2.9Z M9.4 4.4l2.2 2.2" />,
  cup: (
    <g>
      <path d="M3.2 6.4h7.6v2.8a3.8 3.8 0 0 1-3.8 3.8 3.8 3.8 0 0 1-3.8-3.8Z" />
      <path d="M10.8 7.4h.9a1.5 1.5 0 0 1 0 3h-1.2" />
      <path d="M5.6 2.8c-.5.7.5 1.2 0 2M8.2 2.8c-.5.7.5 1.2 0 2" />
    </g>
  ),
  close: <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />,
  check: <path d="M3.6 8.4l2.8 2.8 6-6.2" />,
  sparkle: <path d="M8 2.5l1.3 3.2 3.2 1.3-3.2 1.3L8 11.5 6.7 8.3 3.5 7l3.2-1.3Z" fill="currentColor" stroke="none" />,
  grip: (
    <g fill="currentColor" stroke="none">
      <circle cx="5.5" cy="5" r="1" />
      <circle cx="10.5" cy="5" r="1" />
      <circle cx="5.5" cy="8" r="1" />
      <circle cx="10.5" cy="8" r="1" />
      <circle cx="5.5" cy="11" r="1" />
      <circle cx="10.5" cy="11" r="1" />
    </g>
  ),
  repeat: <path d="M4 7.2a4 4 0 0 1 6.9-2.6L12 5.8M12 3.4v2.4H9.6M12 8.8a4 4 0 0 1-6.9 2.6L4 10.2M4 12.6v-2.4h2.4" />,
  sliders: (
    <g>
      <path d="M3 5h10M3 11h10" />
      <circle cx="6" cy="5" r="1.7" fill="currentColor" stroke="none" />
      <circle cx="10.5" cy="11" r="1.7" fill="currentColor" stroke="none" />
    </g>
  ),
  clock: (
    <g>
      <circle cx="8" cy="8" r="5.6" />
      <path d="M8 4.9V8l2.2 1.4" />
    </g>
  ),
  leaf: (
    <g>
      <path d="M3.2 12.8C2.9 7.6 6.4 3.4 12.9 3.1 13.2 9.3 9.4 13 3.2 12.8Z" />
      <path d="M3.8 12.2 10.6 5.4" />
    </g>
  ),
};

export function Glyph({ name, size = 12 }: { name: GlyphName; size?: number }) {
  return (
    <svg
      className={`glyph glyph--${name}`}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}

export type { GlyphName };
