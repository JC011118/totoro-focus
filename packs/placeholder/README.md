# Pip, the placeholder companion

The bundled pack (`id: placeholder`, name "Pip (placeholder)"). It is embedded
in the app binary and is always available, so the app never depends on
third-party character art. License: CC0-1.0 (see `LICENSE.md`).

**Pip** is an original character: a small, round, moss-green seedling with a
two-leaf sprout, dot eyes, a small blush, stubby arms and two little feet. It
is calm and quiet by design: it breathes, blinks, closes its eyes while you
focus, and celebrates gently. There are no sad, scolding or failure poses.

Design guard rails (keep these if you edit the art): Pip is green with a
sprout and has no ears, no belly patch, no whiskers and no tail. It must not
resemble any existing character (Studio Ghibli, Pokémon, Kirby, Moomins and so
on).

## Files

- `*.svg`: one sprite sheet per animation. Each is a single horizontal strip of
  96x96 frames, left to right (SPEC §7). Sheet width = frames × 96, height 96.
  Every frame is clipped to its own cell. The sheets use plain shapes and
  in-file gradients only: no scripts, no `<image>`, no external references.
- `pack.json`: the manifest. **It is authoritative**; the table below is a
  summary.

Everything here is generated. Do not edit the SVGs by hand. Change
`scripts/gen_placeholder_pack.py` instead and run:

```bash
python3 scripts/gen_placeholder_pack.py          # regenerate + validate
python3 scripts/gen_placeholder_pack.py --check  # validate only
```

The generator is deterministic (standard library only, no network): running it
twice produces byte-identical files. Its validator checks the manifest against
SPEC §7 and `data/unlocks.json`, so every animation and idle-variant unlock
has art. It also checks that every sheet exists, is well-formed and safe, and
has width = frames × frameWidth, and it runs the palette contrast checks below.

## Animations

| key | frames | plays | stillFrame | cycle | what it shows |
|---|---:|---|---:|---:|---|
| `idle` | 6 | loop | 0 | 2.40 s | gentle breathing, the sprout sways |
| `focus` | 4 | loop | 0 | 3.20 s | eyes closed, very slow calm breathing |
| `paused` | 4 | loop | 0 | 2.00 s | sitting, eyes open, a small pause bubble |
| `completed` | 6 | once | 5 | 0.79 s | a happy hop, then holds a happy pose |
| `break` | 6 | loop | 0 | 4.30 s | resting with a cup of tea, takes a sip |
| `react` | 5 | once | 4 | 0.43 s | a short friendly wiggle (character click) |
| `blink` | 3 | once | 0 | 0.22 s | one blink (half, shut, half) |
| `wave` | 7 | once | 3 | 0.88 s | a small wave |
| `stretch` | 7 | once | 2 | 1.56 s | a big stretch with a yawn |
| `umbrella` | 6 | loop | 0 | 1.50 s | idle under a leaf umbrella in the rain |
| `sleepy` | 4 | loop | 0 | 4.00 s | dozing, head tilted, tiny z's |
| `celebrate` | 8 | once | 3 | 0.80 s | a jump with sparkles |
| `acorn_gift` | 6 | once | 4 | 1.72 s | brings out an acorn and holds it out |
| `long_session` | 7 | once | 5 | 2.05 s | proud; a small flower opens on the sprout |
| `hum` | 8 | once | 0 | 1.60 s | sways and hums, music notes drift up |
| `firefly` | 6 | loop | 0 | 2.00 s | idle while three fireflies drift around |
| `lantern` | 4 | loop | 0 | 1.33 s | holds a small paper lantern that flickers |
| `stargaze` | 6 | loop | 0 | 3.00 s | looks up at twinkling stars |

Notes for the renderer:

- Every key that SPEC §7 resolves is provided directly, so `fallbacks` is empty.
- `completed` has `loop: false` and holds its last frame (a happy pose), which
  is also its `stillFrame` for reduced motion.
- `stillFrame` is a calm, complete pose for every key (frame 0 of the idle-like
  loops; the peak or held pose of one-shots), so a reduced-motion companion
  still looks intentional.
- `blink` uses the idle body pose, so it can replace `idle` or `focus` for one
  cycle without a jump.
- Timing comes from `frameDurationsMs` where it matters (focus, completed,
  break, react, blink, stretch, sleepy, acorn_gift, long_session), otherwise
  from `fps`. `fps` is always present and matches the total duration.
- The character stands on a ground line 84 px from the top of the frame (feet
  reach about 87 px, with a soft shadow below). It is horizontally centered at
  x = 48, except in `lantern`, where it shifts 7 px left to make room for the
  lantern. Every frame keeps at least 2 px of empty margin inside its cell.

## Character colors

For anything outside the pack that wants to match Pip (for example a static
fallback drawn by the renderer):

| part | color |
|---|---|
| body | `#8dbb66` |
| body highlight | `#b4d78f` |
| feet, speckles | `#6f9d4f` |
| leaves | `#a9d46b` (vein `#6c9c43`) |
| stem | `#5f8d3f` |
| outline | `#3b5b32` (2.2 px at 96 px) |
| eyes, mouth | `#2b2c26` |
| blush | `#f59c9c` |

## Palettes

The pack defines no `palettes` of its own. Unlock palettes get their default
colors from the `palette` objects on the palette-kind entries of
`data/unlocks.json` (evening, autumn, dawn, snow, moss, bloom, old_friends).
A pack can still override any of them field by field (SPEC §5).

### Base palette: `forest`

The renderer defines `forest` as a built-in constant (it is the base every
unlock palette is layered on). These are the values the art was designed and
checked against:

| field | value | role |
|---|---|---|
| `label` | Forest | |
| `background` | `#f3f0e4` | warm paper (widget card) |
| `surface` | `#e3e9d6` | soft sage (buttons, chips) |
| `text` | `#26331f` | deep moss ink (countdown, labels) |
| `muted` | `#55634a` | secondary text |
| `accent` | `#4a7535` | leaf green (primary button, highlights) |
| `ring` | `#5b8c43` | progress ring |
| `ringTrack` | `#d2dac3` | ring track |
| `characterFilter` | (none) | Pip is drawn unfiltered |

```ts
// src/theme/palettes.ts (suggested)
export const FOREST = {
  label: "Forest",
  background: "#f3f0e4",
  surface: "#e3e9d6",
  text: "#26331f",
  muted: "#55634a",
  accent: "#4a7535",
  ring: "#5b8c43",
  ringTrack: "#d2dac3",
} as const;
```

### Contrast rules

The generator enforces these WCAG 2.x ratios on `forest` and on every unlock
palette:

- `text` and `muted` against both `surface` and `background`: ≥ 4.5:1
- `accent` against `background`: ≥ 4.5:1, so it works as text color and as a
  button fill with `background`-colored text
- `ring` against `background`: ≥ 3:1, and against `ringTrack`: ≥ 2:1

Colors are lowercase `#rrggbb` only. `characterFilter` uses only
`brightness`, `contrast`, `saturate`, `sepia`, `hue-rotate` and `grayscale`
with numeric arguments.

## Visual QA

The sheets were rendered frame by frame with the renderer's CSS math
(`background-size: N×100% 100%`, `background-position: c/(N−1)×100% 0`) in
headless Chromium at device scale 1. They were checked at 96 px and 64 px on
light (`#f3f0e4`) and dark (`#2d2a32`, `#1f2b22`) backgrounds, with each
palette's `characterFilter` applied in a mock 240x200 widget. Nothing is
clipped, the character is centered, and the face reads at 64 px.

Not yet checked: rendering in WKWebView on macOS (the app's real renderer),
pending manual verification.
