import type { Limits } from "../ipc/bindings/Limits";

/** Used until `get_app_info` answers; the backend validates every start anyway. */
export const DEFAULT_LIMITS: Limits = {
  presets: [15, 25, 45],
  minMinutes: 1,
  maxMinutes: 180,
  breakMinutes: 5,
  maxTaskLabelChars: 80,
};

type Range = Pick<Limits, "minMinutes" | "maxMinutes">;

export function clampMinutes(v: number, limits: Range): number {
  if (!Number.isFinite(v)) return limits.minMinutes;
  return Math.min(limits.maxMinutes, Math.max(limits.minMinutes, Math.round(v)));
}

/**
 * Stepper rule (frontend-ipc-petdex-issues #8): ±1 below 5 minutes, ±5 from 5
 * up, snapping to multiples of 5 above 5 (7 → 10 up, 7 → 5 down).
 */
export function stepMinutes(v: number, dir: 1 | -1, limits: Range): number {
  const cur = clampMinutes(v, limits);
  let next: number;
  if (dir > 0) next = cur < 5 ? cur + 1 : (Math.floor(cur / 5) + 1) * 5;
  else next = cur <= 5 ? cur - 1 : Math.ceil(cur / 5) * 5 - 5;
  return clampMinutes(next, limits);
}

/** Parses typed minutes ("25", " 45 "). Returns null unless it is a whole number in range. */
export function parseMinutes(text: string, limits: Range): number | null {
  const t = text.trim();
  if (!/^\d{1,4}$/.test(t)) return null;
  const n = Number(t);
  return n >= limits.minMinutes && n <= limits.maxMinutes ? n : null;
}
