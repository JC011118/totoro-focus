import type { PaletteSpec } from "../ipc/bindings/PaletteSpec";
import { contrast, mix, parseColor, toCss, withAlpha, type RGBA } from "./color";

export type ColorField = "background" | "surface" | "text" | "muted" | "accent" | "ring" | "ringTrack";

export const COLOR_FIELDS: readonly ColorField[] = [
  "background",
  "surface",
  "text",
  "muted",
  "accent",
  "ring",
  "ringTrack",
];

export interface ResolvedPalette extends Record<ColorField, string> {
  id: string;
  label: string;
  /** CSS filter for the character, or "none". */
  characterFilter: string;
}

/**
 * Built-in base palette (SPEC §5): every unlock palette and pack palette is
 * layered on top of it. Values match packs/placeholder/README.md, against which
 * the placeholder art and the contrast rules were checked.
 */
export const FOREST: ResolvedPalette = {
  id: "forest",
  label: "Forest",
  background: "#f3f0e4",
  surface: "#e3e9d6",
  text: "#26331f",
  muted: "#55634a",
  accent: "#4a7535",
  ring: "#5b8c43",
  ringTrack: "#d2dac3",
  characterFilter: "none",
};

/** Layers palettes field by field; later layers win, missing/empty fields fall through. */
export function mergePalette(base: ResolvedPalette, ...layers: Array<PaletteSpec | null | undefined>): ResolvedPalette {
  const out: ResolvedPalette = { ...base };
  for (const layer of layers) {
    if (!layer) continue;
    for (const f of COLOR_FIELDS) {
      const v = layer[f];
      if (typeof v === "string" && v.trim() !== "") out[f] = v.trim();
    }
    if (typeof layer.characterFilter === "string" && layer.characterFilter.trim() !== "") {
      out.characterFilter = layer.characterFilter.trim();
    }
    if (typeof layer.label === "string" && layer.label.trim() !== "") out.label = layer.label.trim();
  }
  return out;
}

const WHITE: RGBA = { r: 255, g: 255, b: 255, a: 1 };
const INK: RGBA = { r: 20, g: 24, b: 18, a: 1 };
/** Warm "tea" tone that breaks lean toward, so a break never looks like focus. */
const HONEY: RGBA = { r: 214, g: 160, b: 86, a: 1 };

/** Mixes `c` toward `toward` until it reaches `ratio` contrast against `bg` (or gives up). */
export function ensureContrast(c: RGBA, bg: RGBA, toward: RGBA, ratio: number): RGBA {
  let out = c;
  for (let i = 1; i <= 10 && contrast(out, bg) < ratio; i++) out = mix(c, toward, i / 10);
  return out;
}

/**
 * CSS custom properties for a palette, including a few derived shades used
 * for borders, soft fills and text on the accent color.
 */
export function paletteVars(p: ResolvedPalette): Record<string, string> {
  const get = (f: ColorField): RGBA => parseColor(p[f]) ?? (parseColor(FOREST[f]) as RGBA);
  const bg = get("background");
  const surface = get("surface");
  const text = get("text");
  const muted = get("muted");
  const accent = get("accent");
  const ring = get("ring");
  const track = get("ringTrack");
  const dark = contrast(text, WHITE) < contrast(text, INK);

  const best = (...options: RGBA[]) =>
    options.reduce((a, b) => (contrast(b, accent) > contrast(a, accent) ? b : a));
  const onAccent = contrast(bg, accent) >= 4.5 ? bg : best(bg, WHITE, INK);

  return {
    "--c-bg": toCss(bg),
    "--c-surface": toCss(surface),
    "--c-text": toCss(text),
    "--c-muted": toCss(muted),
    "--c-accent": toCss(accent),
    "--c-ring": toCss(ring),
    "--c-ring-track": toCss(track),
    "--c-character-filter": p.characterFilter || "none",
    "--c-on-accent": toCss(onAccent),
    "--c-accent-soft": toCss(mix(surface, accent, 0.16)),
    "--c-surface-raised": toCss(mix(surface, bg, 0.45)),
    "--c-surface-press": toCss(mix(surface, text, 0.1)),
    "--c-line": toCss(withAlpha(text, dark ? 0.16 : 0.12)),
    "--c-line-strong": toCss(withAlpha(text, dark ? 0.26 : 0.2)),
    "--c-break": toCss(ensureContrast(mix(ring, HONEY, 0.55), bg, text, 3)),
    "--c-break-track": toCss(mix(track, HONEY, 0.18)),
    "--c-card-edge": toCss(withAlpha(text, dark ? 0.22 : 0.1)),
    "--c-shadow": toCss(withAlpha(INK, dark ? 0.4 : 0.14)),
    colorScheme: dark ? "dark" : "light",
  };
}
