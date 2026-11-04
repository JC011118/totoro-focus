# Totoro Focus — behavior and architecture contract

This document is the contract every module is implemented and reviewed against.
Product brief: a calm, offline macOS focus timer with a desktop companion.
**Thesis: a focus timer that never punishes.** Every active minute counts,
completion is celebrated rather than demanded, nothing earned is ever taken away.

Display name ("Totoro Focus") and the active character pack are configuration
(`productName` in `src-tauri/tauri.conf.json`, `settings.activePack`). Product
logic never names a character.

## 1. Architecture

```
crates/focus-core   pure Rust, no Tauri: clock, timer, engine, rewards, history,
                    settings, store (persistence), packs, geometry, snapshot (view model)
src-tauri           app shell: windows/panel, tray + menu-bar title, global hotkey,
                    ticker thread, sleep/wake + clock-change observers, pack:// protocol,
                    sound, commands. Applies engine Effects. Owns all I/O.
src/                React + TypeScript renderer. Renders Snapshot, sends commands.
                    Never owns timing. No Node, no filesystem, no network.
packs/placeholder   original bundled character pack (embedded into the binary)
data/unlocks.json   friendship rules + unlock table (embedded; edit data, not code)
```

Rules:
- The backend owns windows, tray, hotkey, session state, timer scheduling,
  suspend handling, checkpoints, persistence. The frontend is a pure view:
  `get_snapshot` on mount + `focus://snapshot` events. A renderer reload or a
  second window can never create a second timer (there is exactly one Engine,
  behind one Mutex, in the backend; `tauri-plugin-single-instance` prevents a
  second process).
- No network access anywhere. No AI, accounts or telemetry.
- Art lives in packs. Missing art always falls back (see §7); it never breaks the timer.
- Rewards are idempotent per session ID. Points and unlocks never decrease.
- IPC types are generated from Rust with ts-rs into `src/ipc/bindings/`
  (`npm run bindings`). Do not hand-edit generated files.

## 2. Clock model

- `Mono`: milliseconds on a monotonic clock with an arbitrary per-process origin.
  Used for ALL durations. Never persisted, never compared across processes.
- `Wall`: `DateTime<Utc>`. Used only to stamp records, compute the local
  calendar day, and measure pause age for expiry (§4.6).
- `Now { mono, wall }` is passed into every engine call. `Clock` trait supplies
  it (`SystemClock` in the app, `FakeClock` in tests) plus `local_offset(wall)`
  for local-day math.
- Do not rely on any clock's behavior across sleep (macOS `Instant` stops during
  sleep; Linux `CLOCK_MONOTONIC` too; but this is not relied upon — see §4.5).

## 3. Session model

States (`Phase`): `idle`, `focus` (running | paused), `break` (running | paused),
`awaitingChoice` (after a focus completes, or after a break finishes).

`ActiveSession` holds: `id`, `kind` (focus|break), optional `taskLabel`,
`plannedMs`, `startedAt` (wall), `accruedMs` (active time folded so far), and a
run state: `Running { since: Mono }` or `Paused { pausedAt: Wall, reason,
ageAnchor: Wall }`.

- `activeMs(now) = min(planned, accrued + (now.mono - since))` while running.
- `remainingMs = planned - activeMs`.
- A one-second tick is only a refresh; remaining time derives from accumulated
  active time, never from counting ticks and never from wall time.

Limits: presets 15/25/45 min; custom 1–180 min; break 5 min; the last focus
duration is remembered (`settings.lastFocusMinutes`, default 25) and updated on
every focus start.

## 4. Engine rules (all unit-tested with a fake clock)

### 4.1 Commands
| Command | Allowed from | Result |
|---|---|---|
| `startFocus{minutes?, taskLabel?}` | idle, awaitingChoice | new focus session running; `minutes` default = lastFocusMinutes; validates 1..=180; trims label (max 80 chars, empty → none); updates lastFocusMinutes; Persist; transition `focusStarted`; if `hideDuringFocus` → `HideCompanion` |
| `startBreak{minutes?}` | idle, awaitingChoice | break session running (default 5 min); transition `breakStarted` |
| `pause` | running | fold, Paused{reason:user}; Persist |
| `resume` | paused | Running{since: now.mono}; Persist |
| `toggle` (hotkey) | any | idle/awaitingChoice → startFocus(last duration, no label); running → pause; paused → resume |
| `end` | focus or break session | focus → interruption transaction (§4.3); break → idle, no record |
| `dismiss` | awaitingChoice | → idle |
| `updateSettings(patch)` | any | validates, applies, Persist, `SettingsChanged` effect |
| `quit` | any | running session → fold + Paused{reason: quit}; Persist (the shell exits after applying) |

Invalid commands return `CommandError` and change nothing (e.g. `startFocus`
while a session is active → `SessionActive`).

### 4.2 Completion transaction (focus)
When a tick folds `accrued >= planned` for a running focus session:
1. clamp accrued to planned;
2. append a `SessionRecord{outcome: completed}` to history **unless a record with
   this id already exists**;
3. award once per session id: `points = floor(activeMs / 60000) * pointsPerActiveMinute`
   + `completionBonus` (5); recompute unlocks (union; never removed);
4. phase → `awaitingChoice{after: focus, last}`; active session cleared;
5. effects: `Persist`, transition `focusCompleted{record, award, longSession}`,
   `PlaySound(completion)` if `settings.sound`, `ShowCompanion` if the companion
   was hidden by `hideDuringFocus`.
Steps 2–4 are one in-memory mutation followed by one atomic save; repeating the
transaction for the same id adds nothing.

A completed 25-minute session awards exactly 30 points. `longSession` = planned
minutes ≥ `longSessionMinutes` (45).

### 4.3 Interruption transaction (focus ended early, or pause expiry)
Same as 4.2 but `outcome: interrupted`, `endReason: userEnded | pauseExpired`,
**no bonus**, points = whole active minutes (22 active minutes → 22 points),
phase → `idle`, transition `focusInterrupted`, no sound, no celebration, never
any scolding. The harvest row gains a leaf (derived from history).

### 4.4 Breaks
Break completion → `awaitingChoice{after: break}`, transition `breakFinished`,
`PlaySound(breakOver)` if sound is on. Breaks never earn points and are not
written to history. The next focus starts only on user action. Never auto-chain.

### 4.5 Suspend, wake, observation gaps, clock jumps
The engine keeps an observation `baseline: Option<Now>` (last tick it saw).
- `SystemEvent::Suspend`: if running → fold at now, Paused{reason: suspend};
  `Persist`. Baseline cleared.
- `SystemEvent::Wake`: nothing resumes. Session stays paused (user resumes).
  Baseline cleared.
- `SystemEvent::Tick` (≈1 Hz; also used after commands): let
  `dm = now.mono - baseline.mono`, `skew = (now.wall - baseline.wall) - dm`.
  - if running and `dm > maxObservationGapMs` (120 s): the gap earns nothing:
    fold at `baseline.mono`, pause{reason: suspend} with `pausedAt = baseline.wall`.
    (Covers a missed sleep notification or a frozen process.)
  - else if `skew > clockJumpToleranceMs` (60 s) and running: possible missed
    sleep → fold at now.mono (mono time only), pause{reason: clockJump}.
  - if paused and `|skew| > clockJumpToleranceMs`: shift `ageAnchor` by `skew`
    so an unexplained wall jump never counts toward pause expiry.
  - otherwise fold normally, then check completion and pause expiry.
- `SystemEvent::ClockChanged` (macOS `NSSystemClockDidChangeNotification`):
  compute skew as above; if paused shift `ageAnchor` by skew; never pause,
  never complete, never award. Baseline reset to now.
- **Wall-clock changes never complete a session, never add active time and
  never award points.**
- **Input inactivity is never used to infer focus.**

### 4.6 Pause expiry
A focus or break session paused for more than `pauseExpiryMs` (120 min) is ended
automatically. Pause age = `now.wall - ageAnchor` (ageAnchor starts at pausedAt
and is shifted by detected clock jumps; sleep time while paused does count —
sleep is reported via Suspend/Wake, not as a jump). Focus → interruption
transaction with `endReason: pauseExpired`, `endedAt = pausedAt` (the moment
activity stopped), earned minutes kept. Break → idle silently. No scolding
transition: the frontend treats it like any interruption.
Expiry is checked on every tick and once at startup restore.

### 4.7 Checkpoints, quit, crash, relaunch
- While running, the engine emits `Persist` at most every `checkpointIntervalMs`
  (15 s) on ticks, plus on every state change. Saving always folds first so the
  file holds current `accruedMs` and `checkpointAt`.
- Quit: `quit` command pauses (reason quit) and persists.
- Crash: at most 15 s of active time since the last checkpoint is lost (documented).
- Relaunch: a persisted session is always restored **paused** (`reason: restored`
  if it was running at the checkpoint, otherwise its saved reason), with
  `pausedAt = checkpointAt` for running checkpoints. Closed-app time earns
  nothing. Pause expiry is evaluated immediately (§4.6).
- `awaitingChoice` is not persisted; relaunch shows idle.

### 4.8 App Nap / activity
While a session is running the shell holds an `NSProcessInfo` activity
(`UserInitiatedAllowingIdleSystemSleep`) so App Nap cannot throttle the ticker.
Engine emits `ActivityChanged{running}` when this should change.

## 5. Friendship and unlocks

`data/unlocks.json` holds `rules {pointsPerActiveMinute, completionBonus,
longSessionMinutes}` and an ordered `unlocks` list `{id, points, kind, trigger,
label, description}`. Thresholds grow ~1.45×; extend the list there, not in code.

`Friendship { totalPoints, unlocked: set, awards: map<sessionId, Award> }`
- `award(sessionId, activeMs, completed)` returns `None` if the id was already
  awarded (idempotent). Otherwise adds points + bonus, records the award, and
  returns newly unlocked ids.
- On load: `totalPoints = max(stored, sum(awards))`; `unlocked = stored ∪
  {thresholds ≤ totalPoints}`. Nothing is ever removed (even if a threshold is
  later raised).
- Breaks and character clicks earn nothing.

Unlock triggers (frontend presentation; never during active focus):
`focusStart` (wave, played at the start transition), `breakStart` (stretch),
`focusComplete` (celebrate replaces base completed), `harvest` (acorn_gift after
completion), `longSessionComplete` (planned ≥ 45 min), idle variants
(`rainyIdle` umbrella on ~1 in 4 local days chosen deterministically from the date —
offline, no weather API; `lateNight` sleepy 23:00–05:00; `summerEvening`, etc.),
palettes (`afterSunset` evening uses an approximate per-month sunset table;
`autumnSeason` Sep–Nov; others selectable). Unlocked animations can be replayed
from the character menu (Settings → Companion), only while not in active focus.

## 6. History and today's harvest

`SessionRecord { id, kind: focus, taskLabel?, startedAt, endedAt, plannedMs,
activeMs, outcome: completed|interrupted, endReason: completed|userEnded|pauseExpired }`.
Only focus sessions are recorded. Records are timer records, not proof of attention.

`TodaySummary` is **derived** from history at snapshot time: "today" is the
local calendar day (via `Clock::local_offset`) of `endedAt`. Harvest = one
`acorn` per completed and one `leaf` per interrupted record today, in end order.
The UI shows at most 12 icons, then `+N`. `focusMs` = sum of today's record
active time; `activeFocusMs` = the running/paused focus session's active time.

## 7. Character packs

A pack is a folder with `pack.json` plus sprite sheets (PNG, WebP or SVG).

```jsonc
{
  "format": 1,
  "id": "placeholder",            // ^[a-z0-9][a-z0-9_-]{0,63}$
  "name": "Pip (placeholder)",
  "author": "…", "license": "CC0-1.0", "description": "…",
  "rendering": "smooth",          // or "pixelated"
  "animations": {
    "idle":  { "sheet": "idle.svg", "frameWidth": 96, "frameHeight": 96,
               "frames": 4, "fps": 4, "loop": true,
               "row": 0, "column": 0, "sheetColumns": 4, "sheetRows": 1 }
    // states: idle, focus, paused, completed, break, react (click), blink (optional)
    // unlock ids: wave, stretch, umbrella, sleepy, celebrate, acorn_gift, long_session, …
  },
  "fallbacks": { "paused": "idle" },   // optional overrides of the built-in chain
  "palettes": { "evening": { "background": "#…", "surface": "#…", "text": "#…",
                "muted": "#…", "accent": "#…", "ring": "#…", "ringTrack": "#…",
                "characterFilter": "brightness(0.9) saturate(0.9)" } }
}
```
`row`, `column` default 0; `sheetColumns` defaults to `column + frames`;
`sheetRows` to `row + 1`; `loop` defaults to true. Frames run left to right in one row.

Resolution (`resolve(key)`): the pack's own animation → the pack's `fallbacks`
chain → the built-in chain (`focus, paused, break, completed, react, blink → idle`;
unlock animations → none, i.e. skip the flourish) → `None`. Cycles are cut.
`None` for a base state means the frontend draws its built-in static companion,
which must look intentional. Missing palettes fall back to app defaults.

Sources and precedence: bundled `placeholder` (embedded in the binary, always
available, id reserved), user packs in `<appData>/packs/<dir>/pack.json`, and in
debug builds `packs-local/<dir>/pack.json` (gitignored; overrides user packs with
the same id). Invalid packs are listed with errors and cannot be activated. If the
active pack is missing at runtime the placeholder renders and the setting is kept.

Files are served to the webview by the `pack://localhost/<packId>/<relativePath>`
protocol, which rejects absolute paths, `..`, backslashes, and anything outside the
pack directory. Sprites render as CSS backgrounds only (never inline SVG).

Optional (milestone 4): a loader for the Codex/Petdex pet format
(`pet.json` + 8×9 atlas) that converts it into a `PackManifest`.

## 8. Shell behavior (macOS)

- Companion: ~240×200 logical px, transparent, undecorated, non-resizable,
  floating above normal windows on all Spaces, **non-activating NSPanel** via
  `tauri-nspanel` (style mask `nonactivatingPanel`, `canBecomeKeyWindow` false
  except while the task-label field is being edited). `acceptFirstMouse` on.
  Drag via a header strip (`data-tauri-drag-region`) that does not overlap
  buttons. `FOCUS_WINDOW_MODE=native` env var builds a plain always-on-top window
  instead, for spike comparison.
- Activation policy `Accessory` (no Dock icon). Settings window is a normal window.
- Position saved (debounced) on move; on startup and on screen-parameter changes
  it is recovered to a visible spot inside some monitor's work area (pure
  `geometry::ensure_visible`).
- Tray: icon (template image); title `mm:ss` while a session exists (prefixed
  `⏸ ` when paused; breaks use the same format), icon only when idle or
  awaiting a choice; the title can be turned off in settings. Menu: Start last
  duration (N min), Start 15/25/45, Custom duration…, Pause/Resume, End
  session, Show/Hide companion, Today: H h M min focused (disabled label),
  Settings…, Quit.
- Global hotkey default `Ctrl+Alt+T` (⌃⌥T), rebindable, can be disabled; maps to
  `toggle`. Invalid accelerators are rejected and the previous binding kept.
- Sleep/wake via `NSWorkspaceWillSleepNotification` /
  `NSWorkspaceDidWakeNotification`; clock changes via
  `NSSystemClockDidChangeNotification`; display changes via
  `NSApplicationDidChangeScreenParametersNotification`.
- Sound: a short original soft chime (embedded WAV) played with `NSSound`.
- Quit (tray, Cmd+Q, logout): `quit` command → persist → exit. Closing windows
  hides them; the app keeps running in the menu bar.

## 9. Persistence

One file: `<appDataDir>/state.json`, schema-versioned (`schemaVersion: 1`),
written atomically (write `state.json.tmp`, fsync, rename, fsync dir). Holds
`settings`, `activeSession` (checkpoint), `history`, `friendship` together so they
stay consistent. Load: missing → defaults; unreadable/corrupt → the file is moved
to `state.corrupt-<timestamp>.json` and defaults are used (never silently
deleted); newer schema → same backup-and-defaults path with a logged warning;
older schema → migrated.

## 10. IPC surface

Commands (Rust `#[tauri::command]`, allowed per window in capabilities):
`get_snapshot`, `start_focus{minutes?, taskLabel?}`, `start_break{minutes?}`,
`pause`, `resume`, `end_session`, `dismiss`, `update_settings{patch}`,
`set_hotkey{accelerator: string|null}`, `get_history{limit?}`, `list_packs`,
`get_active_pack`, `set_active_pack{id}`, `replay_animation{key}`,
`show_companion`, `hide_companion`, `open_settings{tab?}`,
`set_text_input_active{active}`, `get_app_info`, `quit_app`.

Events (backend → frontend): `focus://snapshot` (Snapshot, every tick and after
every change), `focus://transition` (Transition), `focus://pack-changed`
(ResolvedPack), `focus://play` ({key}) for replays, `focus://open-tab` ({tab}).

## 11. Testing and reporting

- Fake-clock unit tests for every timer transition, suspend/wake, gap and clock
  jump handling, relaunch recovery, pause expiry, thresholds, duplicate
  completion/reward handling, persistence round-trips and corruption handling,
  pack parsing/resolution/path safety, geometry recovery.
- Frontend: Vitest for formatting, harvest capping, palette/idle-variant choice,
  transition → animation mapping.
- Checks: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
  `npm run typecheck`, `npm run lint`, `npm test`, `npm run build`,
  `scripts/check-macos.sh` (cargo check for aarch64-apple-darwin from Linux).
- Reports always separate "automated checks passed" from "needs manual macOS
  verification". Manual checks are never marked passed by an agent.
