// Static data for the dev mock. The unlock table is the real one
// (data/unlocks.json). Palette defaults are filled in here only when the data
// file lacks them, so the preview always shows distinct palettes.

import table from "../../data/unlocks.json";
import type { Limits } from "../ipc/bindings/Limits";
import type { PaletteSpec } from "../ipc/bindings/PaletteSpec";
import type { Rules } from "../ipc/bindings/Rules";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";

const DEV_PALETTE_FALLBACKS: Record<string, PaletteSpec> = {
  evening: { background: "#2c2a31", surface: "#3a3740", text: "#f3e9da", muted: "#c4b8aa", accent: "#e8a964", ring: "#e8a964", ringTrack: "#4b4753", characterFilter: "brightness(0.88) saturate(0.85) sepia(0.15)" },
  autumn: { background: "#f6ede1", surface: "#ecdcc7", text: "#3a2619", muted: "#6b4f3c", accent: "#a8502b", ring: "#b55a31", ringTrack: "#e0c9ae", characterFilter: "sepia(0.25) saturate(1.1) hue-rotate(350deg)" },
  dawn: { background: "#f8f1ee", surface: "#f0e3e0", text: "#3a3340", muted: "#675d6b", accent: "#a65844", ring: "#c06d58", ringTrack: "#e6d2cd" },
  snow: { background: "#f1f4f6", surface: "#e1e8ed", text: "#222e38", muted: "#52606c", accent: "#4b6f8b", ring: "#567c99", ringTrack: "#cdd8e0" },
  moss: { background: "#1e2a21", surface: "#29392c", text: "#e5eed8", muted: "#a9b99b", accent: "#9cc56f", ring: "#9cc56f", ringTrack: "#3a4d3d" },
  bloom: { background: "#fbf3f4", surface: "#f3e2e6", text: "#3c2b34", muted: "#6e5662", accent: "#a64c6f", ring: "#bf6386", ringTrack: "#ebd1d9" },
  old_friends: { background: "#f8eed8", surface: "#eedfbc", text: "#3a2b12", muted: "#66522c", accent: "#8c6014", ring: "#b47e22", ringTrack: "#e4d0a2" },
};

export const DEV_UNLOCKS: UnlockDef[] = (table.unlocks as UnlockDef[]).map((u) =>
  u.kind === "palette" && !u.palette && DEV_PALETTE_FALLBACKS[u.id] ? { ...u, palette: DEV_PALETTE_FALLBACKS[u.id] } : u,
);

export const DEV_RULES: Rules = table.rules;

export const DEV_LIMITS: Limits = {
  presets: [15, 25, 45],
  minMinutes: 1,
  maxMinutes: 180,
  breakMinutes: 5,
  maxTaskLabelChars: 80,
};
