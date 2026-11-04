# Character pack format

A character pack is a folder of sprite sheets plus a `pack.json` manifest. Packs
are the only place character art lives: product logic never names a character,
and missing art always falls back to something that still works. A broken or
incomplete pack never breaks the timer.

This document is for pack authors. The behavior contract is `docs/SPEC.md` §7.
The implementation is `crates/focus-core/src/packs.rs` (and `packs/`), and the
palette allow-lists are in `crates/focus-core/src/palette.rs`.

> **Licensing in one paragraph.** The repository bundles exactly one pack: the
> original placeholder in `packs/placeholder/`, which the project may
> redistribute. Personal packs, including fan art such as a Totoro pack, stay on
> your machine, in the user packs folder or in the gitignored `packs-local/`.
> They are never committed, bundled or published. See [Licensing](#licensing).

## Where packs live

| Source | Location | Notes |
|---|---|---|
| Bundled | `packs/placeholder/` in the repo, embedded in the app binary | Always available. Its id `placeholder` is reserved. |
| User | `<appData>/packs/<folder>/` | On macOS: `~/Library/Application Support/io.github.jc011118.totorofocus/packs/<folder>/`. Dev runs (`npm run app:dev`) use their own data dir, `…/io.github.jc011118.totorofocus.dev/packs/<folder>/`. |
| Dev (debug builds only) | `packs-local/<folder>/` at the repo root | Gitignored. A dev pack **replaces** a user pack with the same id. |

To install a pack, copy its folder into the user packs folder and pick it in
Settings → Companion. The pack folders are rescanned whenever Settings asks for
the pack list (`list_packs`). Choosing a pack needs no code changes.

Discovery rules:

- Every direct subfolder is a pack candidate. Folders whose names start with `.`
  are skipped. Loose files are ignored.
- A folder with `pack.json` is a native pack. A folder with `pet.json` and no
  `pack.json` is converted as a [Petdex pet](#petdex--codex-pets). A visible
  folder with neither file is listed with the error "no pack.json (or Petdex
  pet.json) in this folder", so a nested unzip (`packs/my-pack/my-pack/pack.json`)
  is easy to spot.
- At most 256 folders are read per source directory.
- A symlinked pack folder is followed: the pack's root is the link's target.
  Inside a pack, every file must resolve to a location inside that root (see
  [File serving](#file-serving)).

Invalid packs are still listed, with their errors, but cannot be activated. If
the active pack disappears or becomes invalid, the placeholder renders, Settings
shows which pack it replaced (`fallbackFrom`), and your setting is kept. The pack
comes back as soon as it is fixed.

### Ids and collisions

A pack's id comes from `pack.json` (`id`). For a Petdex pet it is the folder
name. Ids match `^[a-z0-9][a-z0-9_-]{0,63}$`.

- A disk pack that claims the reserved id `placeholder` is rejected. Give your
  copy another id.
- Within one source, two packs with the same id: a parsed `pack.json` wins over a
  converted Petdex pet. A converted pet wins over a broken pack whose id could
  only be guessed from its folder name. Remaining ties go to the folder name that
  sorts first. The loser is listed with a "duplicate pack id" error.
- A dev pack (`packs-local/`) replaces the user pack with the same id, even when
  the dev pack is broken. This lets you see the errors of the copy you are
  editing. The user pack is then not listed, and the dev pack carries the warning
  "overrides the user pack with the same id".
- Packs that cannot own an id (reserved-id claims and collision losers) are
  listed as `user:<folder>` or `dev:<folder>`. That form can never be activated.
  Every listed id is therefore unique.

## Folder layout

```
my-pack/
  pack.json
  idle.svg            # sprite sheets: .png .webp .gif .jpg/.jpeg .svg
  sheets/focus.png    # subfolders are fine
  LICENSE.md          # recommended; not served to the renderer
```

## `pack.json`

```jsonc
{
  "format": 1,                     // required; only 1 exists
  "id": "my-pack",                 // required; ^[a-z0-9][a-z0-9_-]{0,63}$
  "name": "My Pack",               // required; shown in Settings (≤ 80 chars)
  "author": "You",                 // optional (≤ 120 chars)
  "license": "CC-BY-4.0",          // optional, but a warning is shown without it (≤ 64 chars)
  "description": "…",              // optional (≤ 500 chars)
  "rendering": "smooth",           // "smooth" (default) or "pixelated"
  "animations": {
    "idle": {
      "sheet": "idle.svg",         // required; path relative to the pack folder
      "frameWidth": 96,            // required; 1–4096 px
      "frameHeight": 96,           // required; 1–4096 px
      "frames": 4,                 // required; 1–256
      "fps": 4,                    // required; > 0 and ≤ 60
      "loop": true,                // default true
      "row": 0,                    // default 0
      "column": 0,                 // default 0: first frame's column
      "sheetColumns": 4,           // default column + frames
      "sheetRows": 1,              // default row + 1
      "stillFrame": 0,             // default 0; frame shown under reduced motion
      "frameDurationsMs": null     // optional; one entry per frame, overrides fps
    }
  },
  "fallbacks": { "paused": "idle" },  // optional; see Fallbacks
  "palettes": { "evening": { … } }    // optional; see Palettes
}
```

Unknown fields are ignored. Free-text fields have control characters removed
and whitespace collapsed, and are cut to the lengths shown, with a warning. A
blank `name` shows the id instead. The manifest may be at most 256 KiB. A UTF-8
byte-order mark is accepted.

### Sprite sheets

- Each animation is one row of one sheet. Frames run **left to right** from
  `column`, for `frames` cells, inside a grid of `sheetColumns × sheetRows`
  cells of `frameWidth × frameHeight`. Several animations can share a sheet
  through different `row` and `column` values.
- Set `sheetColumns` and `sheetRows` explicitly when a sheet holds more than one
  animation. The defaults assume the sheet ends at this animation's last frame
  and at this row.
- The renderer draws sprites only as CSS backgrounds (never inline SVG), using
  percentage math: `background-size: sheetColumns×100% sheetRows×100%`, and
  `background-position: column/(sheetColumns−1)×100% row/(sheetRows−1)×100%`
  (0% when the count is 1). Only the frame **aspect** matters, so a sheet
  exported at 2× works unchanged.
- `rendering: "pixelated"` uses nearest-neighbour sampling, and only when the
  sprite is drawn at a scale of 1 or more. Smaller sprites are always smoothed.
- `frameDurationsMs` gives per-frame timing (1–60000 ms each, exactly `frames`
  entries). When present it overrides `fps`, which must still be valid.
- A sheet may be at most 16 MB. Its implied size (`frameWidth × sheetColumns`,
  `frameHeight × sheetRows`) may be at most 16384 px per side.

### States and animation keys

| Key | When it shows |
|---|---|
| `idle` | No session, and after a break ends (awaiting the next choice). |
| `focus` | A focus session is running. Keep it calm: breathing, blinking. |
| `paused` | A focus or break session is paused. |
| `break` | A break is running. |
| `completed` | A focus session just completed (awaiting the next choice). |
| `react` | One-shot when the character is clicked. Without it, a 300 ms CSS hop plays. |
| `blink` | One-shot that replaces `idle` or `focus` every 5–9 s. Skipped when absent. |
| Unlock ids | `wave`, `stretch`, `celebrate`, `acorn_gift`, `long_session`, `hum` (one-shot flourishes) and `umbrella`, `sleepy`, `firefly`, `lantern`, `stargaze` (idle variants). The current list is the `animation`/`idleVariant` entries of `data/unlocks.json`. |

Base states (`idle`, `focus`, `paused`, `completed`, `break`) are the core
of a pack. A pack needs at least one of them; provide all five if you can.
Everything else is optional.

- A base animation with `loop: false` plays once and then holds its last frame.
  `completed` usually works best that way.
- Flourishes (`wave`, `celebrate`, `acorn_gift`, …) play only after the player
  has unlocked them, and only if your pack has them. They play their frames once
  whatever `loop` says, and are capped at 4 seconds.
- Under reduced motion the companion shows the base state's `stillFrame` and
  plays nothing else. For a `loop: false` animation, the last frame is usually
  the right still frame.
- There are no failure or scolding states, and there never will be. Please don't
  make art for one.
- Keys are free-form, so you can add your own (they must match the id pattern).
  They are resolved and sent to the renderer, and can be targets of `fallbacks`.

### Fallbacks

To find the animation for a key, the app tries in order:

1. the pack's own animation with that key;
2. the pack's `fallbacks` entry for that key, followed recursively;
3. the built-in chain: `focus`, `paused`, `break` and `completed` → `idle`.
   `react`, `blink`, every unlock id and custom keys have no built-in fallback;
4. nothing (`null`). For a base state, the renderer then draws its built-in
   static companion. A flourish is simply skipped.

The search backtracks. If a pack fallback leads nowhere (for example `"paused":
"nap"` when `nap` doesn't exist), the built-in chain is still tried, so `paused`
reaches `idle`. Each key is visited at most once, which cuts cycles: with
`{"paused": "focus", "focus": "paused"}` both still resolve to `idle`.

Examples:

```jsonc
"fallbacks": {
  "paused": "focus",          // a paused session keeps the focus loop
  "long_session": "celebrate",// reuse the celebration for long sessions
  "react": "wave"             // clicking plays the wave (if the pack has one)
}
```

### Render keys

The app resolves a fixed set of keys for the renderer: the base states, `react`,
`blink`, every unlock id of kind `animation` or `idleVariant`, and every key in
your `animations`. Each one appears in the result, resolved or `null`. Settings
uses this to tell "unlocked but not in this pack" apart from "not unlocked yet".

## Palettes

A pack may recolor the companion per palette id. The palette ids are the
palette unlocks in `data/unlocks.json` (`evening`, `autumn`, `dawn`, `snow`,
`moss`, `bloom`, `old_friends`) and the default `forest`. Every field is
optional. The effective palette is built field by field: the built-in `forest`,
then the unlock's default colors, then your pack's values.

```json
"palettes": {
  "evening": {
    "label": "Lantern evening",
    "background": "#2b2238", "surface": "rgba(60, 48, 80, 0.9)",
    "text": "#f3ead8", "muted": "hsl(260deg 15% 70%)",
    "accent": "#e9a86b", "ring": "#e9a86b", "ringTrack": "#ffffff22",
    "characterFilter": "brightness(0.9) saturate(0.9)"
  }
}
```

The values are checked against strict allow-lists. Anything else is dropped,
with a warning such as `palettes.evening.background: dropped unsupported color
"url(x)"`. A dropped value never makes the pack invalid.

- **Colors** (`background`, `surface`, `text`, `muted`, `accent`, `ring`,
  `ringTrack`): at most 64 characters, one of:
  - `#rgb`, `#rrggbb`, `#rrggbbaa`;
  - `rgb()`, `rgba()`, `hsl()`, `hsla()` with plain numbers, either
    comma-separated (`rgb(255, 128, 0)`, `rgba(0, 0, 0, 0.5)`) or
    space-separated with an optional `/ alpha` (`rgb(255 128 0 / 50%)`). A number
    may carry `%`; the hue of `hsl()`/`hsla()` may carry `deg`. Exponents,
    `calc()`, `var()`, `url()`, named colors and `transparent` are not accepted.
    Four-digit hex (`#rgba`) is not accepted either. For transparency, use
    eight-digit hex (`#00000000`) or `rgba(…, 0)`.
- **`characterFilter`**: at most 160 characters and 8 functions, separated by
  spaces, from `brightness()`, `contrast()`, `saturate()`, `sepia()` and
  `grayscale()` (a non-negative number, optionally `%`), and `hue-rotate()`
  (a number with `deg`, or plain `0`). Use `brightness(1)` for "no filter".
- **`label`**: display text, at most 48 characters.
- Palette ids must match the id pattern; other ids are ignored with a warning.

## Validation

Settings → Companion lists every pack with its errors and warnings.

**Errors** make a pack unusable:

- `format` is not 1, or the id does not match the pattern (reported by the
  parser for `pack.json`);
- invalid JSON, or a missing or mistyped required field;
- no base state resolves to any animation;
- an animation key that does not match the id pattern;
- an unsafe sheet path (absolute, `..`, `\`, `:`, control characters, empty or
  hidden `.`-prefixed components, longer than 512 bytes or deeper than 16
  folders);
- a sheet that is not `.png`, `.webp`, `.gif`, `.jpg`/`.jpeg` or `.svg`, that is
  missing, larger than 16 MB, or resolves outside the pack folder;
- `frameWidth` or `frameHeight` of 0 or above 4096, `frames` of 0 or above 256,
  `fps` not in (0, 60];
- frames running past the sheet (`column + frames > sheetColumns`), `row ≥
  sheetRows`, a grid above 256×256 cells or an implied sheet above 16384 px;
- `stillFrame ≥ frames`; `frameDurationsMs` with the wrong length or an entry
  outside 1–60000;
- a reserved or duplicate id (see [Ids and collisions](#ids-and-collisions)).

**Warnings** are informational:

- a base state that borrows another animation, for example `no "paused"
  animation; it uses "idle"`;
- fallbacks that are unused, point to unknown keys, use invalid keys, or form a
  cycle (which is cut);
- dropped palette values and invalid palette ids;
- no license declared, a blank name, or truncated text;
- for dev packs, overriding a user pack.

## File serving

The renderer loads sheets only through `pack://localhost/<packId>/<path>`
(`http://pack.localhost/…` on Windows). Each path segment is percent-encoded,
so `react sheet.svg` becomes `react%20sheet.svg`. The handler decodes it, then
refuses (with 404) anything that:

- is unsafe by the path rules above, or is a hidden file;
- has a type other than png, webp, gif, jpg, svg or json;
- is larger than 16 MB;
- resolves outside the pack folder. Symlinks are canonicalized first, so a link
  that points out of the pack is refused. A link to another file inside the pack
  works.

Bundled placeholder files are served from memory. Only valid packs are served.

## Petdex / Codex pets

The app can also load pets in the Petdex / Codex format
([crafter-station/petdex](https://github.com/crafter-station/petdex)). **It
never downloads anything.** You install pets yourself by copying the pet's
folder into the user packs folder or `packs-local/`. The app does not scan
`~/.petdex/pets` or `~/.codex/pets`.

A pet folder holds `pet.json` and one sprite atlas:

```json
{ "id": "boba", "displayName": "Boba", "description": "…",
  "spriteVersionNumber": 1, "spritesheetPath": "spritesheet.webp" }
```

The atlas has **8 columns × 9 rows** (v1, 1536×1872 px) or **8 columns × 11
rows** (v2, 1536×2288 px) of **192×208 px** cells. Frames run left to right
from column 0, and unused cells are transparent. A uniformly scaled atlas is
accepted (for example 768×936).

### Conversion

| Our key | Petdex row | Frames | Loop | Frame timing (ms) | Still frame |
|---|---|---|---|---|---|
| `idle` | 0 idle | 6 | yes | 280, 110, 110, 140, 140, 320 (1100 total) | 0 |
| `focus` | 0 idle | 6 | yes | as idle | 0 |
| `break` | 0 idle | 6 | yes | as idle | 0 |
| `paused` | 6 waiting | 6 | yes | 150 ×5, 260 (1010 total) | 0 |
| `completed` | 4 jumping | 5 | no | 140 ×4, 280 (840 total) | 4 (settle) |
| `react` | 3 waving | 4 | no | 140 ×3, 280 (700 total) | 3 |
| `wave` | 3 waving | 4 | no | as react | 3 |
| `celebrate` | 4 jumping | 5 | no | as completed | 4 |
| `long_session` | 4 jumping | 5 | no | as completed | 4 |

Every converted animation uses `frameWidth` 192, `frameHeight` 208, `column` 0,
explicit `sheetColumns: 8` and `sheetRows: 9` or `11`, `fps = frames × 1000 /
total`, and `frameDurationsMs` from the table. That table is the native Petdex
renderer's timing. The pack gets `rendering: "smooth"`, no fallbacks and no
palettes; palette unlocks still apply through their default colors.

Deliberately **not** mapped:

- row 5, `failed`: the app has no failure animations, ever;
- rows 1, 2 and 7 (running): locomotion looks wrong in a stationary widget;
- row 8, `review`: too busy for focus;
- v2 rows 9–10: static look-direction poses.

All other keys (`stretch`, `sleepy`, `umbrella`, `acorn_gift`, `hum`, `blink`, …)
are left unmapped and fall back as usual. `paused` and the other base states are
always present.

Conversion details:

- **Id**: the folder name if it matches the id pattern (Petdex slugs do).
  Otherwise the pet's `id` is lowercased and sanitized, then the folder name is,
  with a warning. If neither yields a valid id, the pet is invalid.
- **Name**: `displayName`, else the id. **Description**: `description`.
- **Sheet**: `spritesheetPath` if it is a plain file name
  (`[A-Za-z0-9._-]{1,63}`, a raster type, not hidden), then `spritesheet.webp`,
  then `spritesheet.png`. Each fallback adds a warning. If no sheet is found, or
  it is larger than 16 MB, the pet is invalid.
- **Rows**: read from the PNG, WebP or GIF header without decoding. 11 rows if
  `height × 1536 ≥ width × 2288`, else 9. The size must then form an exact
  8-column grid of 192:208 cells, or the pet is invalid. If the size disagrees
  with `spriteVersionNumber`, the size wins (with a warning). If the header
  cannot be read, the row count follows `spriteVersionNumber` (with a warning).
- **`spriteVersionNumber`**: 1 when absent; anything other than 1 or 2 is an
  error.
- Extra pet.json fields (`kind`, `tags`, `states`, …) are ignored, as Petdex
  itself does.

### Caveats

- **There is no license field in `pet.json`.** Petdex records licenses on its
  server, and many pets are user-submitted fan art owned by their submitters.
  Converted pets therefore have no author or license and always carry the
  warning "no license declared in pet.json". Using a pet is your responsibility;
  check its license on the site you got it from.
- **Never bundle or commit Petdex pets.** Keep them in your user packs folder
  or `packs-local/`.
- Petdex frames include transparent padding, so a pet looks smaller than a
  native pack drawn edge to edge in a 96 px frame.
- Rows hold fixed meanings, so unlock flourishes the atlas doesn't have
  (`stretch`, `acorn_gift`, …) don't play for pets.

## Licensing

- The repository bundles **only** the original placeholder pack
  (`packs/placeholder/`), created for this project and licensed as stated in
  its `pack.json` and `LICENSE.md`. It is the only art that is embedded in the
  app or redistributed.
- Personal packs, such as a Totoro pack or other fan art, are for your own
  machine. Put them in the user packs folder or in `packs-local/`, which is
  gitignored. Never commit them, attach them to issues, bundle them or publish
  them, and never fetch them from the network in app code.
- Studio Ghibli's published permission covers selected website stills only. It
  is not an animation license. Open-sourcing this code grants no rights to any
  character art.
- The public build ships under another name with an original character. It never
  references a personal pack.
- If you publish a pack of your own original art, include a `license` in
  `pack.json` and a `LICENSE.md` in the folder.

## Author checklist

- [ ] `format: 1`, a unique `id` (not `placeholder`), a `name` and a `license`.
- [ ] At least `idle`; ideally all of `idle`, `focus`, `paused`, `completed` and
      `break`.
- [ ] Frames run left to right in one row; `sheetColumns` and `sheetRows` are set
      for shared sheets.
- [ ] `stillFrame` is a calm pose for reduced motion.
- [ ] Nothing in the pack scolds or shows failure.
- [ ] Settings → Companion shows the pack as valid with no unexpected warnings.
