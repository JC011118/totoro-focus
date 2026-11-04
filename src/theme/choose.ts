// Palette and idle-variant choice (SPEC §5 precedence). Pure functions over
// the unlock table, the unlocked set, the active pack and the active triggers.

import type { ResolvedPack } from "../ipc/bindings/ResolvedPack";
import type { UnlockDef } from "../ipc/bindings/UnlockDef";
import { SEASON_TRIGGERS, TIME_OF_DAY_TRIGGERS } from "./localTime";
import { FOREST, mergePalette, type ResolvedPalette } from "./palettes";

/** Idle variants, highest precedence first. Unlisted variants follow in table order. */
export const IDLE_VARIANT_PRECEDENCE: readonly string[] = ["sleepy", "stargaze", "lantern", "firefly", "umbrella"];

export function resolvesIn(pack: ResolvedPack | null | undefined, key: string): boolean {
  return pack?.animations[key] != null;
}

/** Palette `auto`: time of day > season > forest; only unlocked palettes whose trigger is active. */
export function autoPaletteId(
  unlocks: readonly UnlockDef[],
  unlocked: readonly string[],
  triggers: ReadonlySet<string>,
): string {
  const have = new Set(unlocked);
  const candidates = unlocks.filter((u) => u.kind === "palette" && have.has(u.id) && triggers.has(u.trigger));
  return (
    candidates.find((u) => TIME_OF_DAY_TRIGGERS.has(u.trigger))?.id ??
    candidates.find((u) => SEASON_TRIGGERS.has(u.trigger))?.id ??
    FOREST.id
  );
}

/** The palette id to show for `settings.palette` ("auto", "forest" or an unlocked palette id). */
export function selectedPaletteId(
  setting: string | undefined,
  unlocks: readonly UnlockDef[],
  unlocked: readonly string[],
  triggers: ReadonlySet<string>,
): string {
  if (!setting || setting === "auto") return autoPaletteId(unlocks, unlocked, triggers);
  if (setting === FOREST.id) return FOREST.id;
  const def = unlocks.find((u) => u.id === setting);
  if (def && def.kind !== "palette") return FOREST.id;
  return unlocked.includes(setting) ? setting : FOREST.id;
}

/** Effective palette = built-in forest ← the unlock's default palette ← the pack's palettes[id]. */
export function effectivePalette(
  id: string,
  unlocks: readonly UnlockDef[],
  pack: ResolvedPack | null | undefined,
): ResolvedPalette {
  const def = unlocks.find((u) => u.id === id && u.kind === "palette");
  const base: ResolvedPalette = { ...FOREST, id, label: def?.label ?? (id === FOREST.id ? FOREST.label : id) };
  return mergePalette(base, def?.palette, pack?.palettes[id]);
}

/**
 * Idle variant: sleepy > stargaze > lantern > firefly > umbrella > none.
 * A candidate must be unlocked, have an active trigger, and resolve in the pack.
 */
export function chooseIdleVariant(
  unlocks: readonly UnlockDef[],
  unlocked: readonly string[],
  pack: ResolvedPack | null | undefined,
  triggers: ReadonlySet<string>,
): string | null {
  const have = new Set(unlocked);
  const rank = (id: string) => {
    const i = IDLE_VARIANT_PRECEDENCE.indexOf(id);
    return i === -1 ? IDLE_VARIANT_PRECEDENCE.length : i;
  };
  const candidates = unlocks
    .map((u, order) => ({ u, order }))
    .filter(({ u }) => u.kind === "idleVariant" && have.has(u.id) && triggers.has(u.trigger) && resolvesIn(pack, u.id))
    .sort((a, b) => rank(a.u.id) - rank(b.u.id) || a.order - b.order);
  return candidates[0]?.u.id ?? null;
}

/** Palettes the user can pick: auto, forest, then unlocked palette unlocks in table order. */
export function pickablePalettes(unlocks: readonly UnlockDef[], unlocked: readonly string[]): string[] {
  const have = new Set(unlocked);
  return ["auto", FOREST.id, ...unlocks.filter((u) => u.kind === "palette" && have.has(u.id)).map((u) => u.id)];
}
