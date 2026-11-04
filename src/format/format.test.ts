import { describe, expect, it } from "vitest";
import type { HarvestItem } from "../ipc/bindings/HarvestItem";
import type { SessionRecord } from "../ipc/bindings/SessionRecord";
import { capHarvest, countHarvest, harvestLabel } from "./harvest";
import { activeMinutesLabel, clockTime, dayLabel, groupByDay, previousDateKey, recordLocal } from "./history";
import { clampMinutes, DEFAULT_LIMITS, parseMinutes, stepMinutes } from "./minutes";

const items = (s: string): HarvestItem[] => [...s].map((c) => (c === "a" ? "acorn" : "leaf"));

describe("capHarvest (SPEC §6)", () => {
  it("shows everything up to 12 with no chip", () => {
    expect(capHarvest([])).toEqual({ shown: [], overflow: 0 });
    expect(capHarvest(items("aal"))).toEqual({ shown: items("aal"), overflow: 0 });
    const twelve = items("aaaalaaaalaa");
    expect(capHarvest(twelve)).toEqual({ shown: twelve, overflow: 0 });
  });

  it("shows the first 12 in history order, then +N = total − 12", () => {
    const seventeen = items("laaaaaaaaaaalllla");
    const { shown, overflow } = capHarvest(seventeen);
    expect(shown).toEqual(seventeen.slice(0, 12));
    expect(shown[0]).toBe("leaf"); // never sorted: a leaf first stays first
    expect(overflow).toBe(5);
  });

  it("honours a custom max and clamps nonsense", () => {
    expect(capHarvest(items("aaaa"), 2)).toEqual({ shown: items("aa"), overflow: 2 });
    expect(capHarvest(items("aa"), -3)).toEqual({ shown: [], overflow: 2 });
  });

  it("counts and labels neutrally", () => {
    expect(countHarvest(items("aala"))).toEqual({ completed: 3, interrupted: 1 });
    expect(harvestLabel([])).toBe("Today's harvest: nothing yet");
    expect(harvestLabel(items("aal"))).toBe("Today's harvest: 2 acorns (completed), 1 leaf (ended early)");
    expect(harvestLabel(items("a"))).toBe("Today's harvest: 1 acorn (completed)");
  });
});

describe("minutes stepper", () => {
  const L = DEFAULT_LIMITS;
  it("steps ±1 below 5 and ±5 from 5", () => {
    expect(stepMinutes(1, 1, L)).toBe(2);
    expect(stepMinutes(4, 1, L)).toBe(5);
    expect(stepMinutes(5, 1, L)).toBe(10);
    expect(stepMinutes(25, 1, L)).toBe(30);
    expect(stepMinutes(25, -1, L)).toBe(20);
    expect(stepMinutes(10, -1, L)).toBe(5);
    expect(stepMinutes(5, -1, L)).toBe(4);
    expect(stepMinutes(2, -1, L)).toBe(1);
  });
  it("snaps typed odd values to the 5-minute grid", () => {
    expect(stepMinutes(7, 1, L)).toBe(10);
    expect(stepMinutes(7, -1, L)).toBe(5);
    expect(stepMinutes(178, 1, L)).toBe(180);
  });
  it("clamps to the limits", () => {
    expect(stepMinutes(1, -1, L)).toBe(1);
    expect(stepMinutes(180, 1, L)).toBe(180);
    expect(clampMinutes(0, L)).toBe(1);
    expect(clampMinutes(999, L)).toBe(180);
    expect(clampMinutes(Number.NaN, L)).toBe(1);
  });
  it("parses typed minutes strictly", () => {
    expect(parseMinutes(" 45 ", L)).toBe(45);
    expect(parseMinutes("180", L)).toBe(180);
    expect(parseMinutes("0", L)).toBeNull();
    expect(parseMinutes("181", L)).toBeNull();
    expect(parseMinutes("2.5", L)).toBeNull();
    expect(parseMinutes("", L)).toBeNull();
    expect(parseMinutes("abc", L)).toBeNull();
  });
});

describe("history formatting", () => {
  const rec = (endedAt: string, offset?: number): SessionRecord => ({
    id: endedAt,
    kind: "focus",
    startedAt: endedAt,
    endedAt,
    plannedMs: 1_500_000,
    activeMs: 1_500_000,
    outcome: "completed",
    endReason: "completed",
    ...(offset === undefined ? {} : { endedUtcOffsetMinutes: offset }),
  });

  it("uses the record's own UTC offset for its local day and time", () => {
    // 23:30 UTC on Nov 3 is 08:30 Nov 4 in UTC+9 and 15:30 Nov 3 in UTC−8.
    expect(recordLocal("2026-11-03T23:30:00Z", 540)).toMatchObject({ dateKey: "2026-11-04", hours: 8, minutes: 30 });
    expect(recordLocal("2026-11-03T23:30:00Z", -480)).toMatchObject({ dateKey: "2026-11-03", hours: 15, minutes: 30 });
    expect(clockTime("2026-11-03T23:30:00Z", 540, "en-GB")).toBe("8:30");
    expect(clockTime("2026-11-03T23:30:00Z", -480, "en-US")).toBe("3:30 PM");
    expect(recordLocal("not a date")).toBeNull();
    expect(clockTime("not a date")).toBe("");
  });

  it("labels whole active minutes like the reward rule", () => {
    expect(activeMinutesLabel(22 * 60_000 + 59_999)).toBe("22 min");
    expect(activeMinutesLabel(59_999)).toBe("0 min");
    expect(activeMinutesLabel(-5)).toBe("0 min");
  });

  it("does calendar math without the clock", () => {
    expect(previousDateKey("2026-03-01")).toBe("2026-02-28");
    expect(previousDateKey("2026-01-01")).toBe("2025-12-31");
    expect(dayLabel("2026-11-04", "2026-11-04")).toBe("Today");
    expect(dayLabel("2026-11-03", "2026-11-04")).toBe("Yesterday");
    expect(dayLabel("2026-11-02", "2026-11-04", "en-US")).toBe("Mon, Nov 2");
  });

  it("groups newest-first records by local end day", () => {
    const groups = groupByDay(
      [rec("2026-11-04T10:00:00Z", 0), rec("2026-11-04T08:00:00Z", 0), rec("2026-11-03T22:00:00Z", 0), rec("2026-11-03T22:00:00Z", 540)],
      "2026-11-04",
    );
    expect(groups.map((g) => [g.label, g.records.length])).toEqual([
      ["Today", 2],
      ["Yesterday", 1],
      ["Today", 1],
    ]);
  });
});
