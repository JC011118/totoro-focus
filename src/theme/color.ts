// Minimal color math for derived palette shades. Done in JS rather than CSS
// color-mix(), which WKWebView only supports from Safari 16.2 (macOS 11 ships 14).

export interface RGBA {
  r: number;
  g: number;
  b: number;
  a: number;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function hex(s: string): RGBA | null {
  const m = /^#([0-9a-f]{3,8})$/i.exec(s);
  if (!m) return null;
  let h = m[1] as string;
  if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
  if (h.length !== 6 && h.length !== 8) return null;
  const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
  return { r: n(0), g: n(2), b: n(4), a: h.length === 8 ? n(6) / 255 : 1 };
}

function num(token: string, scale: number): number | null {
  const t = token.trim();
  if (!/^[+-]?(\d+\.?\d*|\.\d+)(%|deg)?$/.test(t)) return null;
  const v = parseFloat(t);
  return t.endsWith("%") ? (v / 100) * scale : v;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const hh = (((h % 360) + 360) % 360) / 360;
  if (s === 0) return [l * 255, l * 255, l * 255];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  const ch = (t: number) => {
    let x = t;
    if (x < 0) x += 1;
    if (x > 1) x -= 1;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  return [ch(hh + 1 / 3) * 255, ch(hh) * 255, ch(hh - 1 / 3) * 255];
}

/** Parses `#rgb[a]`, `#rrggbb[aa]`, `rgb[a](…)`, `hsl[a](…)` (comma or space syntax). */
export function parseColor(input: string): RGBA | null {
  const s = input.trim().toLowerCase();
  if (s.startsWith("#")) return hex(s);
  const m = /^(rgba?|hsla?)\(([^)]*)\)$/.exec(s);
  if (!m) return null;
  const fn = m[1] as string;
  const body = m[2] as string;
  const [main, alphaPart] = body.includes("/") ? body.split("/") : [body, undefined];
  const parts = (main ?? "").split(/[\s,]+/).filter(Boolean);
  if (alphaPart === undefined && parts.length === 4) {
    // legacy rgba(r, g, b, a)
  } else if (parts.length !== 3) return null;
  const alphaToken = alphaPart ?? parts[3];
  const a = alphaToken === undefined ? 1 : num(alphaToken, 1);
  if (a === null) return null;
  if (fn.startsWith("rgb")) {
    const [r, g, b] = parts.slice(0, 3).map((p) => num(p, 255));
    if (r == null || g == null || b == null) return null;
    return { r: clamp(r, 0, 255), g: clamp(g, 0, 255), b: clamp(b, 0, 255), a: clamp(a, 0, 1) };
  }
  const h = num((parts[0] ?? "").replace(/deg$/, ""), 360);
  const sat = num(parts[1] ?? "", 1);
  const light = num(parts[2] ?? "", 1);
  if (h == null || sat == null || light == null) return null;
  const [r, g, b] = hslToRgb(h, clamp(sat, 0, 1), clamp(light, 0, 1));
  return { r, g, b, a: clamp(a, 0, 1) };
}

/** Linear mix in sRGB: t = 0 → a, t = 1 → b. */
export function mix(a: RGBA, b: RGBA, t: number): RGBA {
  const k = clamp(t, 0, 1);
  const lerp = (x: number, y: number) => x + (y - x) * k;
  return { r: lerp(a.r, b.r), g: lerp(a.g, b.g), b: lerp(a.b, b.b), a: lerp(a.a, b.a) };
}

export function withAlpha(c: RGBA, a: number): RGBA {
  return { ...c, a: clamp(a, 0, 1) };
}

export function toCss(c: RGBA): string {
  const r = Math.round(c.r);
  const g = Math.round(c.g);
  const b = Math.round(c.b);
  const a = Math.round(c.a * 1000) / 1000;
  return a >= 1 ? `rgb(${r}, ${g}, ${b})` : `rgba(${r}, ${g}, ${b}, ${a})`;
}

/** WCAG relative luminance (alpha ignored). */
export function luminance(c: RGBA): number {
  const ch = (v: number) => {
    const x = v / 255;
    return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b);
}

export function contrast(a: RGBA, b: RGBA): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}
