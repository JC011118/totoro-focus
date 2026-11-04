import type { HarvestItem } from "../ipc/bindings/HarvestItem";

export const HARVEST_MAX = 12;

/**
 * SPEC §6: show the first `max` items in history (end) order, then a `+N`
 * chip with N = total − max. Never sorted, never reordered.
 */
export function capHarvest(
  items: readonly HarvestItem[],
  max: number = HARVEST_MAX,
): { shown: HarvestItem[]; overflow: number } {
  const limit = Math.max(0, Math.floor(max));
  return { shown: items.slice(0, limit), overflow: Math.max(0, items.length - limit) };
}

export function countHarvest(items: readonly HarvestItem[]): { completed: number; interrupted: number } {
  let completed = 0;
  let interrupted = 0;
  for (const i of items) {
    if (i === "acorn") completed += 1;
    else interrupted += 1;
  }
  return { completed, interrupted };
}

/** Neutral wording: timer records, not judgments. */
export function harvestLabel(items: readonly HarvestItem[]): string {
  if (items.length === 0) return "Today's harvest: nothing yet";
  const { completed, interrupted } = countHarvest(items);
  const parts = [`${completed} ${completed === 1 ? "acorn" : "acorns"} (completed)`];
  if (interrupted > 0) parts.push(`${interrupted} ${interrupted === 1 ? "leaf" : "leaves"} (ended early)`);
  return `Today's harvest: ${parts.join(", ")}`;
}
