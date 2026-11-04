import type { CSSProperties } from "react";
import type { AppInfo } from "../ipc/bindings/AppInfo";
import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { Snapshot } from "../ipc/bindings/Snapshot";
import { chooseIdleVariant, effectivePalette, selectedPaletteId } from "./choose";
import { localDateKey, presentationTriggers, useLocalNow } from "./localTime";
import { paletteVars, type ResolvedPalette } from "./palettes";

export interface Theme {
  paletteId: string;
  palette: ResolvedPalette;
  /** CSS custom properties to put on the app root. */
  style: CSSProperties;
  idleVariant: string | null;
}

/** Palette and idle variant for right now (presentation only, SPEC §5). */
export function useTheme(snapshot: Snapshot | null, appInfo: AppInfo | null, pack: ResolvedPack | null): Theme {
  const now = useLocalNow();
  const triggers = presentationTriggers(now, snapshot?.today.date ?? localDateKey(now));
  const unlocks = appInfo?.unlocks ?? [];
  const unlocked = snapshot?.friendship.unlocked ?? [];
  const paletteId = selectedPaletteId(snapshot?.settings.palette, unlocks, unlocked, triggers);
  const palette = effectivePalette(paletteId, unlocks, pack);
  return {
    paletteId,
    palette,
    style: paletteVars(palette) as CSSProperties,
    idleVariant: chooseIdleVariant(unlocks, unlocked, pack, triggers),
  };
}
