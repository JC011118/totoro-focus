import { describe, expect, it } from "vitest";
import { ALL_IDS, makePack, UNLOCKS } from "../test/fixtures";
import { autoPaletteId, chooseIdleVariant, effectivePalette, pickablePalettes, selectedPaletteId } from "./choose";
import { contrast, mix, parseColor, toCss } from "./color";
import { fnv1a32, isRainyDay, localDateKey, presentationTriggers, SUNSET_HOUR } from "./localTime";
import { ensureContrast, FOREST, mergePalette, paletteVars } from "./palettes";

// Local-time constructor: getHours()/getMonth() return these values in any TZ.
const at = (month: number, h: number, m = 0) => new Date(2026, month - 1, 15, h, m);
const NOT_RAINY = "2026-11-04";
const RAINY = (() => {
  for (let d = 1; d <= 28; d++) {
    const key = `2026-11-${String(d).padStart(2, "0")}`;
    if (isRainyDay(key)) return key;
  }
  throw new Error("no rainy day in November?");
})();

describe("fnv1a32", () => {
  it("matches the reference vectors", () => {
    expect(fnv1a32("")).toBe(0x811c9dc5);
    expect(fnv1a32("a")).toBe(0xe40c292c);
    expect(fnv1a32("foobar")).toBe(0xbf9cf968);
  });
  it("decides rainy days as hash % 4 == 0", () => {
    expect(isRainyDay(NOT_RAINY)).toBe(fnv1a32(NOT_RAINY) % 4 === 0);
    expect(fnv1a32(RAINY) % 4).toBe(0);
  });
});

describe("presentationTriggers (SPEC §5)", () => {
  const t = (d: Date, day = NOT_RAINY) => presentationTriggers(d, day);

  it("lateNight is 23:00–04:59", () => {
    expect(t(at(6, 22, 59)).has("lateNight")).toBe(false);
    expect(t(at(6, 23, 0)).has("lateNight")).toBe(true);
    expect(t(at(6, 2, 0)).has("lateNight")).toBe(true);
    expect(t(at(6, 4, 59)).has("lateNight")).toBe(true);
    expect(t(at(6, 5, 0)).has("lateNight")).toBe(false);
  });

  it("earlyMorning is 05:00–07:59", () => {
    expect(t(at(6, 4, 59)).has("earlyMorning")).toBe(false);
    expect(t(at(6, 5, 0)).has("earlyMorning")).toBe(true);
    expect(t(at(6, 7, 59)).has("earlyMorning")).toBe(true);
    expect(t(at(6, 8, 0)).has("earlyMorning")).toBe(false);
  });

  it("afterSunset/evening runs from SUNSET_HOUR[month] to 22:59, with half hours", () => {
    expect(SUNSET_HOUR).toHaveLength(12);
    // January: 17:00.
    expect(t(at(1, 16, 59)).has("afterSunset")).toBe(false);
    expect(t(at(1, 17, 0)).has("afterSunset")).toBe(true);
    expect(t(at(1, 17, 0)).has("evening")).toBe(true);
    // February: 17:30.
    expect(t(at(2, 17, 29)).has("afterSunset")).toBe(false);
    expect(t(at(2, 17, 30)).has("afterSunset")).toBe(true);
    // December: 16:30. July: 20:30.
    expect(t(at(12, 16, 30)).has("afterSunset")).toBe(true);
    expect(t(at(7, 20, 29)).has("afterSunset")).toBe(false);
    expect(t(at(7, 20, 30)).has("afterSunset")).toBe(true);
    // Ends at 22:59; 23:00 is late night, not evening.
    expect(t(at(7, 22, 59)).has("evening")).toBe(true);
    expect(t(at(7, 23, 0)).has("evening")).toBe(false);
  });

  it("seasons: spring Mar–May, summer Jun–Aug, autumn Sep–Nov, winter Dec–Feb", () => {
    const season = (m: number) => [...t(at(m, 12))].filter((x) => x.endsWith("Season"));
    expect(season(3)).toEqual(["springSeason"]);
    expect(season(5)).toEqual(["springSeason"]);
    expect(season(6)).toEqual(["summerSeason"]);
    expect(season(8)).toEqual(["summerSeason"]);
    expect(season(9)).toEqual(["autumnSeason"]);
    expect(season(11)).toEqual(["autumnSeason"]);
    expect(season(12)).toEqual(["winterSeason"]);
    expect(season(1)).toEqual(["winterSeason"]);
    expect(season(2)).toEqual(["winterSeason"]);
  });

  it("summerEvening = summer ∧ afterSunset", () => {
    expect(t(at(7, 21)).has("summerEvening")).toBe(true);
    expect(t(at(7, 12)).has("summerEvening")).toBe(false);
    expect(t(at(10, 21)).has("summerEvening")).toBe(false);
  });

  it("rainyIdle comes from the date hash; lateNightClear = lateNight ∧ ¬rainy", () => {
    expect(t(at(11, 12), RAINY).has("rainyIdle")).toBe(true);
    expect(t(at(11, 12), NOT_RAINY).has("rainyIdle")).toBe(false);
    expect(t(at(11, 23, 30), NOT_RAINY).has("lateNightClear")).toBe(true);
    expect(t(at(11, 23, 30), RAINY).has("lateNightClear")).toBe(false);
  });

  it("never includes manual or transition triggers", () => {
    const all = t(at(7, 21));
    for (const x of ["manual", "focusStart", "breakStart", "focusComplete", "harvest", "breakEnd"]) expect(all.has(x)).toBe(false);
    expect(all.has("always")).toBe(true);
  });

  it("formats a local date key", () => {
    expect(localDateKey(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
  });
});

describe("palette choice (SPEC §5 precedence)", () => {
  const trig = (d: Date, day = NOT_RAINY) => presentationTriggers(d, day);

  it("auto: time of day beats season beats forest", () => {
    const autumnEvening = trig(at(10, 20));
    expect(autoPaletteId(UNLOCKS, ALL_IDS, autumnEvening)).toBe("evening");
    expect(autoPaletteId(UNLOCKS, ALL_IDS, trig(at(10, 12)))).toBe("autumn");
    expect(autoPaletteId(UNLOCKS, ALL_IDS, trig(at(10, 6)))).toBe("dawn");
    expect(autoPaletteId(UNLOCKS, ALL_IDS, trig(at(1, 12)))).toBe("snow");
    expect(autoPaletteId(UNLOCKS, ALL_IDS, trig(at(4, 12)))).toBe("bloom");
    // Summer noon: no summer palette in the table → forest.
    expect(autoPaletteId(UNLOCKS, ALL_IDS, trig(at(7, 12)))).toBe("forest");
  });

  it("auto only counts unlocked palettes and never manual ones", () => {
    const autumnEvening = trig(at(10, 20));
    expect(autoPaletteId(UNLOCKS, ["base", "autumn"], autumnEvening)).toBe("autumn");
    expect(autoPaletteId(UNLOCKS, ["base"], autumnEvening)).toBe("forest");
    // moss is manual: unlocked, but never chosen automatically.
    expect(autoPaletteId(UNLOCKS, ["base", "moss"], trig(at(7, 12)))).toBe("forest");
  });

  it("honours an explicit unlocked palette and guards bad settings", () => {
    const noon = trig(at(7, 12));
    expect(selectedPaletteId("moss", UNLOCKS, ["base", "moss"], noon)).toBe("moss");
    expect(selectedPaletteId("forest", UNLOCKS, ALL_IDS, trig(at(10, 20)))).toBe("forest");
    expect(selectedPaletteId("moss", UNLOCKS, ["base"], noon)).toBe("forest");
    expect(selectedPaletteId("wave", UNLOCKS, ALL_IDS, noon)).toBe("forest");
    expect(selectedPaletteId(undefined, UNLOCKS, ALL_IDS, trig(at(10, 20)))).toBe("evening");
  });

  it("lists pickable palettes: auto, forest, then unlocked palettes in table order", () => {
    expect(pickablePalettes(UNLOCKS, ["base", "snow", "evening", "wave"])).toEqual(["auto", "forest", "evening", "snow"]);
  });

  it("merges forest ← unlock default ← pack palette, field by field", () => {
    const pack = makePack(["idle"], { palettes: { evening: { accent: "#ff0000", ring: "#00ff00" } } });
    const p = effectivePalette("evening", UNLOCKS, pack);
    expect(p.background).toBe("#2c2a31"); // unlock default
    expect(p.text).toBe("#f3e9da"); // unlock default
    expect(p.accent).toBe("#ff0000"); // pack wins over the unlock default
    expect(p.ring).toBe("#00ff00"); // pack
    expect(p.surface).toBe(FOREST.surface); // falls through to forest
    expect(p.characterFilter).toBe("brightness(0.9)");
    expect(p.label).toBe("Evening light");
    const forest = effectivePalette("forest", UNLOCKS, makePack(["idle"], { palettes: { forest: { accent: "#123456" } } }));
    expect(forest).toMatchObject({ ...FOREST, accent: "#123456" });
  });

  it("ignores empty override values", () => {
    expect(mergePalette(FOREST, { accent: "  ", text: "" }, undefined, null)).toEqual(FOREST);
  });
});

describe("idle variant choice (SPEC §5 precedence)", () => {
  const variants = ["sleepy", "stargaze", "lantern", "firefly", "umbrella"];
  const pack = makePack(["idle", ...variants]);

  it("sleepy > stargaze > lantern > firefly > umbrella", () => {
    const lateClear = presentationTriggers(at(7, 23, 30), NOT_RAINY);
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, pack, lateClear)).toBe("sleepy");
    const noSleepy = ALL_IDS.filter((i) => i !== "sleepy");
    expect(chooseIdleVariant(UNLOCKS, noSleepy, pack, lateClear)).toBe("stargaze");
    const summerEvening = presentationTriggers(at(7, 21), NOT_RAINY);
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, pack, summerEvening)).toBe("lantern");
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS.filter((i) => i !== "lantern"), pack, summerEvening)).toBe("firefly");
    const rainyNoon = presentationTriggers(at(11, 12), RAINY);
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, pack, rainyNoon)).toBe("umbrella");
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, pack, presentationTriggers(at(11, 12), NOT_RAINY))).toBeNull();
  });

  it("requires the variant to resolve in the active pack", () => {
    const lateClear = presentationTriggers(at(7, 23, 30), NOT_RAINY);
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, makePack(["idle", "stargaze"]), lateClear)).toBe("stargaze");
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, makePack(["idle"]), lateClear)).toBeNull();
    expect(chooseIdleVariant(UNLOCKS, ALL_IDS, null, lateClear)).toBeNull();
  });

  it("requires it to be unlocked", () => {
    const lateClear = presentationTriggers(at(7, 23, 30), NOT_RAINY);
    expect(chooseIdleVariant(UNLOCKS, ["base"], pack, lateClear)).toBeNull();
  });
});

describe("colors and derived CSS variables", () => {
  it("parses the allowed color forms", () => {
    expect(parseColor("#abc")).toEqual({ r: 170, g: 187, b: 204, a: 1 });
    expect(parseColor("#4a7535")).toEqual({ r: 74, g: 117, b: 53, a: 1 });
    expect(parseColor("#00000080")?.a).toBeCloseTo(0.502, 2);
    expect(parseColor("rgb(10, 20, 30)")).toEqual({ r: 10, g: 20, b: 30, a: 1 });
    expect(parseColor("rgba(10,20,30,0.5)")).toEqual({ r: 10, g: 20, b: 30, a: 0.5 });
    expect(parseColor("rgb(10 20 30 / 50%)")).toEqual({ r: 10, g: 20, b: 30, a: 0.5 });
    const red = parseColor("hsl(0, 100%, 50%)");
    expect(red && [Math.round(red.r), Math.round(red.g), Math.round(red.b)]).toEqual([255, 0, 0]);
    expect(parseColor("url(x)")).toBeNull();
    expect(parseColor("red")).toBeNull();
    expect(parseColor("#12")).toBeNull();
  });

  it("mixes and serializes", () => {
    const black = { r: 0, g: 0, b: 0, a: 1 };
    const white = { r: 255, g: 255, b: 255, a: 1 };
    expect(toCss(mix(black, white, 0.5))).toBe("rgb(128, 128, 128)");
    expect(toCss({ r: 1, g: 2, b: 3, a: 0.25 })).toBe("rgba(1, 2, 3, 0.25)");
    expect(contrast(black, white)).toBeCloseTo(21, 0);
  });

  it("darkens a low-contrast color until it reaches the ratio", () => {
    const bg = parseColor("#f3f0e4")!;
    const pale = parseColor("#e8e0c0")!;
    const ink = parseColor("#26331f")!;
    expect(contrast(ensureContrast(pale, bg, ink, 3), bg)).toBeGreaterThanOrEqual(3);
  });

  it("exposes every palette field plus derived shades", () => {
    const v = paletteVars(FOREST);
    for (const k of ["--c-bg", "--c-surface", "--c-text", "--c-muted", "--c-accent", "--c-ring", "--c-ring-track"]) {
      expect(v[k]).toMatch(/^rgba?\(/);
    }
    expect(v["--c-character-filter"]).toBe("none");
    expect(v.colorScheme).toBe("light");
    expect(v["--c-on-accent"]).toBe("rgb(243, 240, 228)"); // forest background on leaf green
    const evening = paletteVars(effectivePalette("evening", UNLOCKS, null));
    expect(evening.colorScheme).toBe("dark");
    const bg = parseColor(evening["--c-bg"] as string)!;
    expect(contrast(parseColor(evening["--c-break"] as string)!, bg)).toBeGreaterThanOrEqual(3);
  });

  it("falls back to forest for unparseable colors", () => {
    const v = paletteVars({ ...FOREST, accent: "not-a-color" });
    expect(v["--c-accent"]).toBe(toCss(parseColor(FOREST.accent)!));
  });
});
