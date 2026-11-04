// Dev-preview packs. "fixtures" uses the test sprites in ./fixtures; "pip"
// loads the repo's bundled placeholder pack from the Vite dev server when it
// exists (packs/placeholder/pack.json); "missing-art" resolves nothing, so the
// built-in static companion shows. Dev only.

import type { PackInfo } from "../ipc/bindings/PackInfo";
import type { PaletteSpec } from "../ipc/bindings/PaletteSpec";
import type { Rendering } from "../ipc/bindings/Rendering";
import type { ResolvedAnimation } from "../ipc/bindings/ResolvedAnimation";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import atlasUrl from "./fixtures/atlas.svg?url";
import celebrateUrl from "./fixtures/celebrate.svg?url";
import { DEV_UNLOCKS } from "./devData";

export interface DevPack {
  info: PackInfo;
  rendering: Rendering;
  animations: Record<string, Omit<ResolvedAnimation, "key" | "sourceKey"> & { sourceKey?: string }>;
  palettes: Record<string, PaletteSpec>;
}

const BASE = ["idle", "focus", "paused", "completed", "break"];
const EXTRA = ["react", "blink"];

/** Every render key per SPEC §7 (base ∪ extra ∪ animation/idleVariant unlocks ∪ pack keys). */
export function renderKeys(pack: DevPack): string[] {
  const keys = new Set([...BASE, ...EXTRA]);
  for (const u of DEV_UNLOCKS) if (u.kind === "animation" || u.kind === "idleVariant") keys.add(u.id);
  for (const k of Object.keys(pack.animations)) keys.add(k);
  return [...keys];
}

/** Resolution: own animation → built-in chain (base states → idle; extras and unlocks → none). */
export function resolvePack(pack: DevPack, fallbackFrom: string | null = null): ResolvedPack {
  const animations: Record<string, ResolvedAnimation | null> = {};
  for (const key of renderKeys(pack)) {
    const own = pack.animations[key];
    const src = own ? key : BASE.includes(key) && key !== "idle" && pack.animations.idle ? "idle" : null;
    const a = src ? pack.animations[src] : undefined;
    animations[key] = a ? { ...a, key, sourceKey: a.sourceKey ?? src ?? key } : null;
  }
  return { info: pack.info, rendering: pack.rendering, animations, palettes: pack.palettes, fallbackFrom };
}

const info = (id: string, name: string, extra: Partial<PackInfo> = {}): PackInfo => ({
  id,
  name,
  author: "Dev fixtures",
  license: "CC0-1.0",
  description: null,
  source: "dev",
  formatKind: "native",
  valid: true,
  errors: [],
  warnings: [],
  ...extra,
});

const cell = (row: number, column: number, frames: number, extra: Partial<ResolvedAnimation> = {}) => ({
  url: atlasUrl,
  frameWidth: 64,
  frameHeight: 64,
  frames,
  fps: 3,
  loop: true,
  row,
  column,
  sheetColumns: 4,
  sheetRows: 4,
  stillFrame: 0,
  frameDurationsMs: null,
  ...extra,
});

const celebrate = {
  url: celebrateUrl,
  frameWidth: 96,
  frameHeight: 96,
  frames: 6,
  fps: 8,
  loop: false,
  row: 0,
  column: 0,
  sheetColumns: 6,
  sheetRows: 1,
  stillFrame: 5,
  frameDurationsMs: [120, 110, 160, 110, 120, 260],
};

export const FIXTURE_PACK: DevPack = {
  info: info("fixtures", "Moss pebble (dev fixtures)", {
    description: "Test sprites for the dev preview: a 4×4 atlas and a 6-frame strip.",
  }),
  rendering: "smooth",
  animations: {
    idle: cell(0, 0, 4),
    focus: cell(1, 0, 2, { frameDurationsMs: [1400, 1400] }),
    blink: cell(1, 2, 2, { loop: false, fps: 10 }),
    paused: cell(2, 0, 1),
    break: cell(2, 1, 3, { fps: 1.5 }),
    react: cell(3, 0, 4, { loop: false, fps: 9, stillFrame: 3 }),
    wave: cell(3, 0, 4, { loop: false, fps: 7, stillFrame: 3 }),
    sleepy: cell(1, 0, 2, { frameDurationsMs: [1800, 1800] }),
    completed: celebrate,
    celebrate: { ...celebrate },
  },
  palettes: {},
};

export const MISSING_ART_PACK: DevPack = {
  info: info("missing-art", "No art (static fallback)", {
    description: "Resolves nothing, so the built-in static companion renders.",
  }),
  rendering: "smooth",
  animations: {},
  palettes: {},
};

export const BROKEN_PACK_INFO: PackInfo = info("broken-pack", "Broken pack (example)", {
  author: null,
  license: null,
  source: "user",
  valid: false,
  errors: ["pack.json: animations.idle.frames must be at least 1"],
  warnings: ["no license declared"],
});

export const PETDEX_PACK_INFO: PackInfo = info("sprout-pet", "Sprout (Petdex example)", {
  author: null,
  license: null,
  source: "user",
  formatKind: "petdex",
  warnings: ["no license declared in pet.json"],
});

interface ManifestAnimation {
  sheet: string;
  frameWidth: number;
  frameHeight: number;
  frames: number;
  fps: number;
  loop?: boolean;
  row?: number;
  column?: number;
  sheetColumns?: number;
  sheetRows?: number;
  stillFrame?: number;
  frameDurationsMs?: number[] | null;
}

/** Loads packs/placeholder from the dev server, if it is there (it is after the art lands). */
export async function loadRepoPlaceholder(): Promise<DevPack | null> {
  try {
    const res = await fetch("/packs/placeholder/pack.json", { cache: "no-store" });
    if (!res.ok || !(res.headers.get("content-type") ?? "").includes("json")) return null;
    const m = (await res.json()) as {
      id: string;
      name: string;
      author?: string;
      license?: string;
      description?: string;
      rendering?: Rendering;
      animations: Record<string, ManifestAnimation>;
      palettes?: Record<string, PaletteSpec>;
    };
    const animations: DevPack["animations"] = {};
    for (const [k, a] of Object.entries(m.animations ?? {})) {
      const column = a.column ?? 0;
      const row = a.row ?? 0;
      animations[k] = {
        url: `/packs/placeholder/${encodeURIComponent(a.sheet)}`,
        frameWidth: a.frameWidth,
        frameHeight: a.frameHeight,
        frames: a.frames,
        fps: a.fps,
        loop: a.loop ?? true,
        row,
        column,
        sheetColumns: a.sheetColumns ?? column + a.frames,
        sheetRows: a.sheetRows ?? row + 1,
        stillFrame: a.stillFrame ?? 0,
        frameDurationsMs: a.frameDurationsMs ?? null,
      };
    }
    return {
      info: info("placeholder", m.name, {
        author: m.author ?? null,
        license: m.license ?? null,
        description: m.description ?? null,
        source: "bundled",
      }),
      rendering: m.rendering ?? "smooth",
      animations,
      palettes: m.palettes ?? {},
    };
  } catch {
    return null;
  }
}
