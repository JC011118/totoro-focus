// Formatting for History records. Records carry their own timestamps; nothing
// here reads the current time ("today" comes from Snapshot.today.date).

import type { SessionRecord } from "../ipc/bindings/SessionRecord";

const pad = (n: number) => String(n).padStart(2, "0");

/**
 * The wall-clock fields of `iso` in the record's own UTC offset when known
 * (SPEC §6), else in the renderer's local zone.
 */
export function recordLocal(
  iso: string,
  offsetMinutes?: number,
): { dateKey: string; hours: number; minutes: number; shifted: Date; useUtc: boolean } | null {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return null;
  if (offsetMinutes !== undefined && Number.isFinite(offsetMinutes)) {
    const d = new Date(t + offsetMinutes * 60_000);
    return {
      dateKey: `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`,
      hours: d.getUTCHours(),
      minutes: d.getUTCMinutes(),
      shifted: d,
      useUtc: true,
    };
  }
  const d = new Date(t);
  return {
    dateKey: `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`,
    hours: d.getHours(),
    minutes: d.getMinutes(),
    shifted: d,
    useUtc: false,
  };
}

/** "14:05" (or the locale's form, e.g. "2:05 PM"). */
export function clockTime(iso: string, offsetMinutes?: number, locale?: string): string {
  const p = recordLocal(iso, offsetMinutes);
  if (!p) return "";
  try {
    return p.shifted.toLocaleTimeString(locale, {
      hour: "numeric",
      minute: "2-digit",
      ...(p.useUtc ? { timeZone: "UTC" } : {}),
    });
  } catch {
    return `${pad(p.hours)}:${pad(p.minutes)}`;
  }
}

/** Whole active minutes, like the reward rule: 22.9 min → "22 min". */
export function activeMinutesLabel(activeMs: number): string {
  return `${Math.floor(Math.max(0, activeMs) / 60_000)} min`;
}

/** "2026-11-04" → "2026-11-03". Pure calendar math. */
export function previousDateKey(dateKey: string): string {
  const [y, m, d] = dateKey.split("-").map(Number);
  if (!y || !m || !d) return "";
  const t = new Date(Date.UTC(y, m - 1, d - 1));
  return `${t.getUTCFullYear()}-${pad(t.getUTCMonth() + 1)}-${pad(t.getUTCDate())}`;
}

export function dayLabel(dateKey: string, todayKey: string, locale?: string): string {
  if (dateKey === todayKey) return "Today";
  if (dateKey === previousDateKey(todayKey)) return "Yesterday";
  const [y, m, d] = dateKey.split("-").map(Number);
  if (!y || !m || !d) return dateKey;
  try {
    return new Date(Date.UTC(y, m - 1, d)).toLocaleDateString(locale, {
      weekday: "short",
      month: "short",
      day: "numeric",
      timeZone: "UTC",
    });
  } catch {
    return dateKey;
  }
}

export interface DayGroup {
  dateKey: string;
  label: string;
  records: SessionRecord[];
}

/** Groups records (already newest first) by the local day they ended. */
export function groupByDay(records: readonly SessionRecord[], todayKey: string, locale?: string): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const r of records) {
    const key = recordLocal(r.endedAt, r.endedUtcOffsetMinutes)?.dateKey ?? "";
    const last = groups[groups.length - 1];
    if (last && last.dateKey === key) last.records.push(r);
    else groups.push({ dateKey: key, label: key ? dayLabel(key, todayKey, locale) : "Unknown day", records: [r] });
  }
  return groups;
}
