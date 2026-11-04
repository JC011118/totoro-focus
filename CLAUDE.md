# CLAUDE.md — Totoro Focus

## What this is

An offline macOS focus timer with a quiet animated desktop companion: a small
character, a readable `mm:ss` countdown with a progress ring, and a row of
today's completed sessions ("harvest"), plus a menu-bar countdown and a global
hotkey. Completed minutes earn friendship points that unlock animations and
palettes.

**Thesis: a focus timer that never punishes.** Every active minute counts,
completion is celebrated rather than demanded, and nothing earned is ever taken
away. No scolding or failure animations, ever.

"Totoro Focus" is the personal, local display name. It is configuration
(`productName` in `src-tauri/tauri.conf.json`), as is the active character pack.
The public build will ship under another name with an original character.

The full behavior contract is **`docs/SPEC.md`**. Read it before changing
behavior; update it in the same change when behavior changes.

## Stack decision and spike status

- Tauri 2 (`tauri` 2.12.5, `@tauri-apps/cli` 2.12.4) + React 19 + TypeScript,
  authoritative timer in Rust (`crates/focus-core`).
- Companion window = non-activating `NSPanel` via `tauri-nspanel` 2.1.1
  (`canBecomeKeyWindow`/`canBecomeMainWindow` always false, `canHide` false,
  `acceptFirstMouse` on). The companion has no text, number or select inputs —
  only `tabIndex=-1` buttons (SPEC §8.1 rule 7); the task label and typed
  minutes live in Settings → Start. `FOCUS_WINDOW_MODE=native` runs a plain
  always-on-top Tauri window instead, for spike comparison.
- **Spike status: built, NOT yet verified on a Mac.** Development so far has
  happened in a Linux cloud container, where macOS code is only type-checked
  (`npm run check:macos`). The focus test (type in an editor, click a companion
  button, keep typing) is pending on real hardware. Fallback rule if it fails
  after a reasonable attempt: switch to Electron with its macOS `panel` window
  type, keep the React frontend, port `focus-core` to a TypeScript module in the
  main process with the same clock-injection design, and record the reason here.

## Commands

```bash
npm install                 # once
npm run app:dev             # run the app (Vite + Tauri dev)
npm run app:build           # local .app/.dmg (unsigned)
npm run check               # typecheck + lint + vitest + vite build + fmt/clippy/cargo test
npm run check:macos         # cargo check/clippy for aarch64-apple-darwin (works from Linux)
npm run bindings            # regenerate src/ipc/bindings/*.ts from Rust (ts-rs)
cargo test -p focus-core    # core unit tests (fake clock)
npm run dev                 # renderer only, in a browser, with the dev mock backend
```

## Architecture rules (do not break)

1. **Backend owns time.** `focus-core::Engine` (one instance, behind one mutex in
   the shell) is the only timer. The renderer never computes remaining time,
   never schedules completions, never persists. It renders `Snapshot`s and sends
   commands. A renderer reload or a second window must not create a second timer.
2. **Clock injection.** Durations use the monotonic clock only; wall time only
   stamps records, finds the local day, and ages pauses. Everything time-related
   in `focus-core` takes `Now`/`Clock` so it is testable with `FakeClock`.
3. **No network.** No HTTP clients, analytics, update checks, AI, or accounts.
4. **Art lives in packs.** Product logic never names a character. Missing art
   falls back (pack fallbacks → idle → built-in static companion) and never
   breaks the timer. Never fetch or commit Studio Ghibli art; personal packs go
   in the gitignored `packs-local/` or the user packs directory.
5. **Rewards are idempotent per session id; nothing is ever subtracted.** Points
   and unlocks only grow. Breaks and character clicks earn nothing.
6. **Persistence is one atomically replaced, schema-versioned JSON file**
   (`state.json` in the app data dir). Never delete a user's state file; corrupt
   files are moved aside.
7. **Minimal IPC surface.** Per-window capabilities allow only the commands that
   window uses; the renderer has no filesystem/Node/shell access. Pack files are
   served only through the validating `pack://` protocol.
8. **IPC types are generated** from Rust via ts-rs into `src/ipc/bindings/`;
   never hand-edit them — change the Rust type and run `npm run bindings`.
9. No plugin framework, no extra services. Keep `CharacterView`, `TimerView`
   and `HarvestRow` reusable for the later illustrated scene.

## Testing conventions

- Timer, engine, rewards, history, persistence, packs and geometry are tested in
  `crates/focus-core` with `FakeClock` — every transition, suspend/wake, clock
  jumps, relaunch recovery, pause expiry, thresholds, duplicate completion/reward
  handling. Tests are deterministic (injected id source, fixed clock).
- Renderer logic (formatting, harvest capping, palette/idle-variant choice,
  transition → animation mapping) is tested with Vitest. Components render from
  plain props so they can be tested and reused.
- Window, focus, pointer, animation, sound, sleep/wake and menu-bar behavior can
  only be verified manually on a Mac.

## Reporting rule: verified vs pending

Every report separates **"automated checks passed"** (commands actually run,
with results) from **"needs manual macOS verification"**. An agent never marks a
manual check as passed. Anything not run on macOS is reported as pending, not
passed. Keep `NOTES.md` current with open items and pending manual checks.

## Publishing rule

Do not create a remote repository, publish, push to a new remote, cut a
release, sign, or notarize without an explicit request from the user. Never
claim a signed or notarized installer exists before it does (signing requires
the user's Apple Developer account).
