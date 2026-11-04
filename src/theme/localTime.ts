// Presentation-only local time (SPEC §5 "Presentation triggers"). This is one
// of the two renderer files allowed to read the clock. Nothing here affects
// the timer: it only picks palettes and idle variants.

import { useEffect, useState } from "react";

/**
 * Local sunset hour by month (Jan..Dec), a northern-hemisphere approximation.
 * Fractions are hours: 17.5 = 17:30.
 */
export const SUNSET_HOUR: readonly number[] = [17, 17.5, 18, 19.5, 20, 20.5, 20.5, 20, 19, 18, 17, 16.5];

/** Triggers that depend on the time of day; they outrank seasons for palettes. */
export const TIME_OF_DAY_TRIGGERS: ReadonlySet<string> = new Set([
  "lateNight",
  "afterSunset",
  "evening",
  "earlyMorning",
  "summerEvening",
  "lateNightClear",
]);

export const SEASON_TRIGGERS: ReadonlySet<string> = new Set([
  "springSeason",
  "summerSeason",
  "autumnSeason",
  "winterSeason",
]);

/** 32-bit FNV-1a over the UTF-8 bytes of `s`. */
export function fnv1a32(s: string): number {
  let h = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(s)) {
    h ^= byte;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h >>> 0;
}

/** "Rainy" days are deterministic and offline: no weather API. */
export function isRainyDay(dateKey: string): boolean {
  return fnv1a32(dateKey) % 4 === 0;
}

/**
 * The automatic triggers active at local time `local` on day `todayDate`
 * (`Snapshot.today.date`, YYYY-MM-DD). `manual` and unknown triggers are never
 * included, so they never apply automatically. Transition triggers
 * (focusStart, …) are handled by the flourish mapping, not here.
 */
export function presentationTriggers(local: Date, todayDate: string): Set<string> {
  const month = local.getMonth();
  const hour = local.getHours() + local.getMinutes() / 60;
  const t = new Set<string>(["always"]);

  const lateNight = hour >= 23 || hour < 5;
  const sunset = SUNSET_HOUR[month] ?? 19;
  const afterSunset = hour >= sunset && hour < 23;
  const earlyMorning = hour >= 5 && hour < 8;
  const spring = month >= 2 && month <= 4;
  const summer = month >= 5 && month <= 7;
  const autumn = month >= 8 && month <= 10;
  const winter = month === 11 || month <= 1;
  const rainy = isRainyDay(todayDate);

  if (lateNight) t.add("lateNight");
  if (afterSunset) {
    t.add("afterSunset");
    t.add("evening");
  }
  if (earlyMorning) t.add("earlyMorning");
  if (spring) t.add("springSeason");
  if (summer) t.add("summerSeason");
  if (autumn) t.add("autumnSeason");
  if (winter) t.add("winterSeason");
  if (summer && afterSunset) t.add("summerEvening");
  if (rainy) t.add("rainyIdle");
  if (lateNight && !rainy) t.add("lateNightClear");
  return t;
}

/** Local YYYY-MM-DD of `d` (used only until the first Snapshot arrives). */
export function localDateKey(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

const MINUTE = 60_000;

/** The current local time, refreshed at each minute boundary (and when the page becomes visible). */
export function useLocalNow(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const schedule = () => {
      clearTimeout(timer);
      timer = setTimeout(
        () => {
          setNow(new Date());
          schedule();
        },
        MINUTE - (Date.now() % MINUTE) + 25,
      );
    };
    const onVisible = () => {
      if (document.visibilityState === "visible") {
        setNow(new Date());
        schedule();
      }
    };
    schedule();
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, []);
  return now;
}
