# Totoro Focus

An offline macOS focus timer with a quiet animated desktop companion: a small
character, a readable `mm:ss` countdown inside a shrinking ring, and a row of
today's completed sessions — one acorn per completed session, one leaf per
session ended early. The countdown also runs in the menu bar, and a global
hotkey starts, pauses and resumes.

**A focus timer that never punishes.** Every active minute earns a friendship
point, whether or not the session completes; completion adds a small bonus;
nothing you earn is ever taken away. There are no failure states and no
scolding.

> "Totoro Focus" is the personal, local display name. The name
> (`productName` in `src-tauri/tauri.conf.json`) and the character pack are
> configuration. The repository bundles only an original placeholder character,
> **Pip**. The public build will ship under another name with its own character.

- No network, no account, no AI. Everything stays in one local file.
- Tauri 2 (Rust) + React + TypeScript. The authoritative timer lives in
  `crates/focus-core`, a pure Rust crate with a fake-clock test suite.
- Status: milestones 0–4 are implemented. **Nothing has been verified on a Mac
  yet** — see [Verified vs pending](#verified-vs-pending).

## Setup

Prerequisites on macOS 11+:
- Xcode Command Line Tools (`xcode-select --install`)
- Rust stable ≥ 1.89 (`rustup`)
- Node.js 22+ and npm

```bash
npm install
```

## Development

```bash
npm run app:dev
```

`app:dev` runs Vite and the app with the dev identifier
`io.github.jc011118.totorofocus.dev`, so dev runs never touch your real data.

Other commands:

| Command | What it does |
|---|---|
| `npm run check` | typecheck, ESLint, Vitest, production build (fails if the dev mock leaks in), `cargo fmt --check`, `clippy -D warnings`, `cargo test` |
| `npm run check:macos` | `cargo check` + `clippy` for `aarch64-apple-darwin` (also works from Linux using a stub C compiler; proves the macOS code compiles, not that it behaves) |
| `cargo test -p focus-core` | timer, engine, rewards, history, persistence, packs and geometry tests (fake clock) |
| `npm run bindings` | regenerate `src/ipc/bindings/*.ts` from the Rust types (ts-rs) — never hand-edit those |
| `npm run dev` → <http://127.0.0.1:1420/dev.html> | renderer-only browser preview over an in-memory mock backend, with a toolbar to jump between states. Never part of the production build |
| `FOCUS_WINDOW_MODE=native npm run app:dev` | companion as a plain always-on-top window instead of the non-activating panel (milestone-0 spike comparison) |

How the code is organized, and the rules it follows, are in
[`CLAUDE.md`](CLAUDE.md) and [`docs/SPEC.md`](docs/SPEC.md):
- the backend owns all timing;
- the renderer only renders snapshots;
- rewards are idempotent per session;
- nothing is ever subtracted;
- there is no network.

## Packaging

```bash
npm run app:build
```

This produces an unsigned `.app` and `.dmg`, using `@tauri-apps/cli` 2.12.4,
`tauri` 2.12.5 and `tauri-nspanel` 2.1.1 (all versions pinned):

- `target/release/bundle/macos/Totoro Focus.app`
- `target/release/bundle/dmg/Totoro Focus_0.1.0_<arch>.dmg`

**Signing and notarization are pending.** They need your Apple Developer
account; no signed or notarized installer exists. To open the unsigned app,
right-click → Open, or run
`xattr -dr com.apple.quarantine "/Applications/Totoro Focus.app"`.

An optional GitHub Actions workflow (`.github/workflows/ci.yml`) runs only when
started by hand. It runs the Linux checks and builds an unsigned macOS `.app`
artifact.

## Controls

**Companion widget** (≈240×200, floats above other windows, on every Space):
- Idle:
  - **Start N** starts your last duration;
  - **15 / 25 / 45** are presets;
  - **±** opens a button-only stepper for 1–180 minutes (a click is ±1, hold or
    scroll to move faster);
  - **✎ task** opens Settings → Start to type a task.
- Running: **Pause**, **End**. Paused: **Resume**, **End**.
- After a session: **5-min break**, **Another N**, **Done**. Nothing starts by
  itself, and work and break sessions are never chained automatically.
- Click the character for a little reaction (it earns nothing).
- **⋯** opens the character menu to replay unlocked animations. **–** hides the
  widget; the timer keeps running.
- Drag the widget by its top strip.
- Its buttons never take keyboard focus from the app you are working in. That is
  the point of the non-activating panel; it is still pending verification on a
  Mac. The two links into Settings (task, companion settings) do activate the
  app, because typing needs focus.

**Menu bar:**
- The title shows `mm:ss` while a session exists (`⏸ mm:ss` when paused) and is
  icon-only when idle. You can turn the title off.
- Menu items: Start last duration, 15 / 25 / 45, Custom duration…,
  Pause/Resume, End session, Show/Hide companion, Today: N min focused,
  Settings…, Quit.

**Global hotkey:**
- Default **⌃⌥T** (Control+Option+T). Idle → start the last duration; running →
  pause; paused → resume.
- Rebind or disable it in Settings → General. A binding needs Control, Option
  or Command, except F-keys.

**Settings window:**
- **Start**: task label, typed minutes.
- **General**: sound, reduced motion, hide during focus, menu-bar title,
  hotkey.
- **History**: today's focus time, harvest, recent sessions.
- **Companion**: friendship points and the next unlock, the unlock gallery with
  replay, palette, character pack, reset position.

**Behavior notes:**
- Pausing for more than 120 minutes ends the session as "ended early", and the
  minutes are kept.
- Sleeping the Mac pauses the session; waking never earns time and never
  resumes by itself.
- Quitting pauses and saves. After a crash, the last checkpoint is restored
  paused, with at most **15 s plus one tick (≤ 16 s)** of active time lost.
- Time while the app is closed or the Mac is asleep never counts.
- Changing the wall clock never completes a session or adds time. Setting it
  forward by more than 60 s mid-session pauses the session so you can resume.
- Detection bounds:
  - If a sleep notification is missed, at most 120 s can be earned, and only
    on clocks that count sleep. macOS's clock does not.
  - A ticker stall longer than 120 s pauses the session and earns nothing for
    the stall.

## Friendship and unlocks

One point per whole active minute of a focus session, plus 5 on completion. A
completed 25-minute session earns 30, and the first unlock (a small wave)
arrives at 25. Breaks and character clicks earn nothing. Unlocks never
decrease.

The table is data, not code: [`data/unlocks.json`](data/unlocks.json). It holds
19 entries from 0 to 28,300 points, growing about 1.45× each step:
- animations at transitions (wave, stretch, celebrate, acorn gift, long-session
  reaction, hum);
- idle variants (rainy day, sleepy, fireflies, lantern, stargazing);
- palettes (evening, autumn, dawn, snow, moss, bloom, golden hour).

Unlocked animations play at transitions, never during active focus.

"Automatic" palettes and idle variants read only the local clock:
- sunset is an approximate per-month table;
- "rainy days" are a hash of the date, because there is no weather API.

## Character packs

A pack is a folder with `pack.json` plus sprite sheets (PNG, WebP, GIF, JPEG or
SVG). The manifest maps states and unlock ids to animations and declares
fallbacks. Missing art never breaks the timer: it falls back to the pack's idle
animation, then to a built-in static companion.

Full format: [`docs/PACK_FORMAT.md`](docs/PACK_FORMAT.md).

Where packs are loaded from:
- the bundled `packs/placeholder/` (Pip, original art, CC0, embedded in the
  binary);
- your user packs folder:
  `~/Library/Application Support/io.github.jc011118.totorofocus/packs/<folder>/`;
- in dev builds only, the gitignored `packs-local/<folder>/`.

Choose the active pack in Settings → Companion (use **Rescan** after adding
one). **Codex/Petdex pets** (a folder with `pet.json` and an 8×9 or 8×11 sprite
atlas) are converted on load and can be selected alongside native packs. The
loader only reads local folders; it never downloads anything.

**Your personal Totoro pack** goes in `packs-local/` or the user packs folder
and is never committed. Studio Ghibli's published permission covers selected
website stills only. It is not an animation license, and open-sourcing this code
grants no rights to any character art.

## Assets and attribution

- **Pip**, the placeholder character (`packs/placeholder/`): original work
  generated by `scripts/gen_placeholder_pack.py`, dedicated to the public domain
  (CC0-1.0; see `packs/placeholder/LICENSE.md`).
- The app icon and tray icon (`scripts/gen_icons.py`) and the completion chime
  (`scripts/gen_chime.py`, synthesized) are original and generated by the
  scripts in `scripts/`.
- No third-party character art, fonts or sounds are bundled. The UI uses the
  system font stack.
- The code license has not been chosen yet. Decide before any public release.

## Data and privacy

Everything lives in one schema-versioned JSON file, replaced atomically:

`~/Library/Application Support/io.github.jc011118.totorofocus/state.json`

It holds settings, the active-session checkpoint, session history and
friendship awards.
- `state.prev.json` keeps the previous copy.
- A corrupt file is moved aside, never deleted.
- A file written by a newer version puts the app into read-only mode, so newer
  totals are never overwritten.

Logs go to `~/Library/Logs/io.github.jc011118.totorofocus/`. Records are timer
records, not proof of attention. The app has no network code. Changing the
bundle `identifier` moves the data folder.

## Measured resource use

**Not yet measured on macOS.** This acceptance item is pending; follow
[`docs/MANUAL_CHECKS.md`](docs/MANUAL_CHECKS.md) §14 on a Mac and record the
results here.

| Scenario (macOS, release `.app`) | App CPU (avg) | WebKit CPU (avg) | Memory | Idle wake-ups |
|---|---|---|---|---|
| Idle, companion visible | pending | pending | pending | pending |
| Idle, companion hidden | pending | pending | pending | pending |
| Idle, reduced motion | pending | pending | pending | pending |
| Focus running, visible | pending | pending | pending | pending |
| Focus running, hidden | pending | pending | pending | pending |

Reference only — this is **not** the acceptance measurement:
- A Linux cloud container ran the release binary (7.2 MB) under Xvfb. WebKitGTK
  rendered in software, with no GPU, so the numbers overstate what a Mac will
  show.
- Results, totals for the app process plus its WebKit web and network
  processes, each averaged over 120 s after a 40 s settle:

  | Scenario (Linux/Xvfb reference) | CPU (avg, all processes) | Resident memory (all processes / app) |
  |---|---|---|
  | Idle, companion visible | 3.9 % | 494 MB / 160 MB |
  | Focus running, companion visible | 4.3 % | 501 MB / 161 MB |

- This run found and fixed a real cost. Before the fix, the progress ring
  re-animated every second, which kept WebKit repainting continuously: focus
  running measured **45.6 %** in the same setup, against 4.3 % after. The
  remaining idle cost is the 4 fps idle sprite being repainted without a GPU.
  Reduced motion stops it.

## Verified vs pending

**Automated checks passed** in a Linux container:
- 511 Rust tests, 144 Vitest tests;
- clippy `-D warnings` for Linux and for `aarch64-apple-darwin`;
- typecheck, lint, and a production build with no dev mock.

There is also a Linux smoke run of the real app, driven by clicks: start, pause
freeze, end → leaf, `kill -9` → restored paused, a 1-minute completion → +6
once, break, menu, settings. The per-requirement map is
[`docs/ACCEPTANCE.md`](docs/ACCEPTANCE.md).

**Needs manual macOS verification (all pending):**
- the non-activating panel keeping focus (milestone-0 spike);
- the menu-bar title;
- the global hotkey;
- sleep/wake;
- quit, crash and relaunch on macOS;
- clock changes;
- sound;
- display scaling and monitor disconnect;
- sprites in the bundled `.app`;
- offline operation;
- idle CPU and memory.

Concise checklist:

1. Type in TextEdit, click every companion button and keep typing: the text
   keeps landing in TextEdit. Repeat with `FOCUS_WINDOW_MODE=native`.
2. Menu-bar title ticks `mm:ss` once a second without shifting width, and shows
   `⏸` when paused; turning it off leaves icon only.
3. ⌃⌥T starts, pauses and resumes from another app; rebinding and disabling
   work and persist.
4. Sleep the Mac mid-session: it comes back paused with the same remaining
   time.
5. Quit, `kill -9` and relaunch: the session is restored paused, and closed time
   earns nothing.
6. Moving the clock ±2 h never completes a session or adds points.
7. A 1-minute session completes once: chime, acorn, +6 points.
8. Unplug an external display holding the companion: it moves back on screen.
   It looks crisp on both Retina and 1× displays.
9. The bundled `.app` animates Pip; a user pack in the packs folder can be
   selected without a restart.
10. With Wi-Fi off everything works; `lsof -i` shows no sockets.
11. Measure idle CPU and memory and fill in the table above.

The full step-by-step list is in
[`docs/MANUAL_CHECKS.md`](docs/MANUAL_CHECKS.md); results go in
[`NOTES.md`](NOTES.md).
