# Totoro Focus — behavior and architecture contract

Every module is implemented and reviewed against this document. It was revised
after an independent design review. When behavior changes, update this file in
the same change.

**Thesis: a focus timer that never punishes.** Every active minute counts,
completion is celebrated rather than demanded, and nothing earned is ever taken
away. There are no scolding or failure states.

The display name ("Totoro Focus") and the active character pack are
configuration (`productName` in `src-tauri/tauri.conf.json`, `settings.activePack`).
Product logic never names a character.

## 1. Architecture

```
crates/focus-core   pure Rust, no Tauri: clock, timer, engine, rewards, history,
                    settings, palette, store (persistence), packs (+petdex), geometry,
                    snapshot (view model), events (IPC names/payloads), testkit
src-tauri           app shell: companion panel + settings window, tray + menu-bar
                    title, global hotkey, ticker thread, sleep/wake/clock/screen
                    observers, pack:// protocol, sound, App Nap activity, commands.
                    Applies engine Effects. Owns all I/O.
src/                React + TypeScript renderer (companion.html, settings.html;
                    dev.html = browser preview with a mock backend, dev only).
                    Renders Snapshots and sends commands. Never owns timing.
packs/placeholder   original bundled character pack (embedded into the binary)
data/unlocks.json   friendship rules, unlock table, default unlock palettes (embedded)
```

Rules:
- The backend owns windows, tray, hotkey, session state, timer scheduling,
  suspend handling, checkpoints and persistence. The renderer subscribes to
  events first, then calls `get_snapshot`, and applies a Snapshot only when
  `seq > lastSeq`. A renderer reload or an extra window can never create a second
  timer. There is exactly one Engine, behind one Mutex, and
  `tauri-plugin-single-instance` prevents a second process.
- **Engine lock discipline.** Hold the Engine mutex only around engine calls
  (`command`, `system`, `snapshot`, `persisted_state`, `mark_persisted`) and the
  `seq` stamp. Read `Now` while holding the lock. Copy out
  `(effects, snapshot, persisted_state)`, drop the guard, then apply effects.
  Never call a Tauri, panel, tray, menu or global-shortcut API, `emit`, or
  anything that re-enters a handler while holding the guard. Many callers run on
  the main thread (sync commands, menu events, hotkey callbacks, NSWorkspace
  observers, RunEvent::Exit), and Tauri calls made off the main thread block on
  it, so holding the guard across them deadlocks. The pack registry sits behind
  its own RwLock and never takes the Engine lock.
- No network access anywhere. No AI, accounts or telemetry.
- Art lives in packs. Missing art always falls back (§7) and never breaks the timer.
- Rewards are idempotent per session ID. Points and unlocks never decrease.
- IPC types are generated from Rust with ts-rs into `src/ipc/bindings/`
  (`npm run bindings`). Never hand-edit them.

## 2. Clock model

- `Mono`: milliseconds on a monotonic clock with an arbitrary per-process origin.
  Used for every duration. Never persisted, never compared across processes.
- `Wall`: `DateTime<Utc>`. Used only to stamp records, find the local calendar
  day, and measure pause age (§4.6).
- Every engine call takes `Now { mono, wall }`. The `Clock` trait supplies it
  (`SystemClock` in the app, `FakeClock` in tests) and also supplies
  `local_offset(wall)` for local-day math.
- No clock's behavior across sleep is relied on. On macOS, `Instant` stops
  during sleep, but correctness never depends on that (§4.5).
- If `now.mono < baseline.mono` (a race between threads), the engine treats
  `dm = 0` and never moves the baseline backward.

## 3. Session model

`Phase`: `idle`, `focus` (running | paused), `break` (running | paused), and
`awaitingChoice`, which follows a completed focus or a finished break.

`ActiveSession` holds `id`, `kind` (focus|break), an optional `taskLabel`,
`plannedMs`, `startedAt` (wall), `accruedMs` (active time folded so far,
≤ planned) and a run state: `Running { since: Mono }` or
`Paused { pausedAt: Wall, reason, ageAnchor: Wall }`.

- `activeMs(at) = min(planned, accrued + (at − since))` while running.
  `remainingMs = planned − activeMs`.
- A one-second tick only refreshes the display. Remaining time comes from
  accumulated active time. It is never derived from counting ticks or from
  wall time.
- **Countdown format** (shared by tray and widget; Rust
  `snapshot::format_countdown`, delivered as `SessionView.remainingLabel`):
  `s = ceil(remainingMs / 1000)`, `format!("{:02}:{:02}", s / 60, s % 60)`, with
  no hour wrap: 1_500_000 → "25:00", 1_499_001 → "25:00", 1_499_000 → "24:59",
  999 → "00:01", 0 → "00:00", 300_000 → "05:00", 10_800_000 → "180:00".
- Limits: presets 15/25/45 min; custom 1–180 min; break 5 min (custom 1–180). The
  last focus duration is remembered (`settings.lastFocusMinutes`, default 25) and
  updated on every focus start. Breaks never change it.
- **Task label normalization** (never an error): replace every `char::is_control`
  with a space, collapse whitespace runs to one space, trim; empty → none;
  otherwise keep the first 80 Unicode scalar values, then trim again.

## 4. Engine rules (all unit-tested with a fake clock)

### 4.0 Call pipeline
`command(cmd, now)`:
1. Resolve and validate against the **pre-observation** state. On `Err`, return
   with no mutation, no observation and no revision bump (`Err` ⇒ engine
   unchanged). Toggle's intent is fixed here: idle/awaitingChoice → StartFocus
   (last duration, no label); running → Pause; paused → Resume.
2. `fx = observe(now)` (§4.5), the same routine Tick uses.
3. Apply the resolved intent to the post-observation state. If observation has
   already overtaken the intent:
   - Pause is a no-op when the session is no longer running.
   - Resume is a no-op when the session no longer exists (it expired).
   - End is a no-op when observation completed or expired the session.
   - Toggle never starts a session in the same call in which observation ended one.
4. `suspended = false`. Return `Ok(fx ++ command effects)`, ordered per §4.9.

**A focus session whose active time has reached planned is always completed
(§4.2), even when the triggering call is End, Pause, Toggle or Quit. It is never
recorded as interrupted.**

`system(ev, now)` runs the routine for `ev` (§4.5) and returns its effects.

### 4.1 Commands
| Command | Allowed from | Result |
|---|---|---|
| `startFocus{minutes?, taskLabel?}` | idle, awaitingChoice | new running focus session; minutes default = lastFocusMinutes, validated 1..=180 (else `InvalidMinutes`); label normalized (§3); lastFocusMinutes updated; `hidden_for_focus` set if `hideDuringFocus` |
| `startBreak{minutes?}` | idle, awaitingChoice | running break (default 5; 1..=180); no label |
| `pause` | session exists | running → fold, Paused{user}; already paused → `Ok([])`, no bump |
| `resume` | session exists | paused → Running{since: now.mono}; already running → `Ok([])` |
| `toggle` (hotkey) | any | see §4.0 step 1 |
| `end` | session exists | focus → completion if due, else interruption transaction (§4.3, `userEnded`); break → idle, `BreakEnded`, no record |
| `dismiss` | awaitingChoice | → idle |
| `updateSettings(patch)` | any | validate whole patch, apply atomically (§4.10) |
| `quit` | any | running session → fold + Paused{quit}; always Persist |

`startFocus`/`startBreak` while a session exists → `SessionActive`. `pause`,
`resume` and `end` with no session → `NoSession`. `dismiss` when not awaiting →
`NotAwaitingChoice`.

### 4.2 Completion transaction (focus)
Triggered when observation folds `accrued ≥ planned` for a focus session (or
via End/Quit/Toggle per §4.0):
1. Clamp accrued to planned. `endedAt = max(startedAt, now.wall − overshootMs)`,
   where overshoot is the folded time beyond planned.
2. Append a `SessionRecord{outcome: completed, endReason: completed,
   endedUtcOffsetMinutes}` unless that id is already in history.
3. Award once per id: `points = floor(activeMs / 60000) × pointsPerActiveMinute
   + completionBonus` (5). Unlocks are a union and are never removed.
4. Phase → `awaitingChoice{after: focus, record, award, longSession}`. The active
   session is cleared. `longSession` = planned minutes ≥ `longSessionMinutes` (45).
5. Effects: Persist, ShowCompanion (if hidden_for_focus; then clear it),
   ActivityChanged, `FocusCompleted`, `PlaySound(completion)` if `settings.sound`.

Steps 1–4 are one in-memory mutation, followed by one atomic save. Repeating the
transaction for the same id adds nothing. A completed 25-minute session awards
exactly 30 points.

### 4.3 Interruption transaction (focus ended early, or pause expiry)
This is the same as §4.2, with these differences: `outcome: interrupted`,
`endReason: userEnded | pauseExpired`, **no bonus**, and points equal to whole
active minutes (22 active minutes → 22 points). The phase goes to `idle`, and the
transition is `FocusInterrupted`. There is no sound and no celebration, and
never any scolding. ShowCompanion fires if `hidden_for_focus` is set. The
harvest row gains a leaf, derived from history. User end:
`endedAt = max(startedAt, now.wall)`. Expiry: `endedAt = max(startedAt, pausedAt)`.

### 4.4 Breaks
A break that reaches zero → `awaitingChoice{after: break}`, `BreakFinished`, and
`PlaySound(breakOver)` if sound is on. A break that is ended or expires → idle,
`BreakEnded`. Breaks never earn points and are not written to history. The next
focus session starts only on user action. Work and break sessions are never
chained automatically.

### 4.5 Observation, suspend, wake, clock changes
Engine fields that are not persisted: `baseline: Option<Now>`,
`suspended: bool`, `activity: ActivityLevel`, `hidden_for_focus: bool`.

`observe(now)`:
```
if !suspended and baseline = Some(b) and a session exists:
    dm   = now.mono.since(b.mono)                           (saturating)
    skew = (now.wall − b.wall).as_ms() − dm                 (signed)
    if running:
        if dm > maxObservationGapMs (120 s): pause_at(b.mono, b.wall, suspend)   # gap earns nothing
        else:
            fold(now.mono)
            if is_due → completion (§4.2) / break finished (§4.4)
            else if skew > clockJumpToleranceMs (60 s): pause_at(now.mono, now.wall, clockJump)
    else if paused and |skew| > clockJumpToleranceMs: shift_age_anchor(skew)
else if baseline is None and running and !suspended:
    fold(now.mono); completion check                         # defensive
expire_if_due(now)                                           # always, §4.6
if !suspended: baseline = Some(max_by_mono(baseline, now))
```
- `Tick`: `observe(now)`. Also emits Persist on the checkpoint cadence (§4.7).
- `Suspend`: `observe(now)`. If the session is running, `pause_at(now.mono,
  now.wall, suspend)` and Persist. Then `baseline = None` and `suspended = true`.
  While suspended, Tick and ClockChanged skip gap and skew detection and leave
  the baseline unset, but they still evaluate pause expiry.
- `Wake`: if a session is still running (Suspend was missed), `pause_at(b.mono,
  b.wall, suspend)` where `b = baseline.unwrap_or(now)`, and Persist. It never
  folds past the last observation. Then `suspended = false`,
  `expire_if_due(now)`, `baseline = Some(now)`. Nothing resumes automatically.
- `ClockChanged` (macOS `NSSystemClockDidChangeNotification`): `observe(now)`
  immediately, like Tick but without the checkpoint cadence. If the clock is set
  forward more than 60 s during a running session, the session pauses with
  clockJump. No time is gained or lost, and the user resumes.
- **Wall-clock changes never complete a session, never add active time and
  never award points.** **Input inactivity is never used to infer focus.**
- Known bounds, documented in the README: a missed suspend notification can earn
  at most `maxObservationGapMs`, and only on clocks that count sleep. A missed
  sleep shorter than `clockJumpToleranceMs` does not pause the session (on macOS
  it earns nothing anyway). A ticker stall longer than `maxObservationGapMs`
  pauses the session with reason suspend and does not earn the stalled interval;
  the shell logs it.

### 4.6 Pause expiry
`expire_if_due(now)`: a paused session whose `now.wall − ageAnchor` is strictly
greater than `pauseExpiryMs` (120 min) ends. A focus session goes through the
interruption transaction (`pauseExpired`, minutes kept). A break goes to idle
silently (`BreakEnded`). There is no scolding transition.

Pause age counts time that both clocks agree has passed, time that Suspend/Wake
vouch for (sleep), and time the app was closed (measured from `pausedAt`, which
is `checkpointAt` for running checkpoints). An unexplained wall-only jump while
the app runs is treated as a clock change (the anchor shifts) and never counts
as pause time. As a result, a session suspended or closed for more than 120
minutes is ended as interrupted at Wake or at launch instead of being shown
paused. Clock changes made while the app is closed cannot be detected. That is
the one documented case where a wall change can trigger expiry, and even then
it never adds active time or points.

### 4.7 Checkpoints, quit, crash, relaunch
- Every state change emits Persist. On a Tick while running, the engine also
  emits Persist iff `last_persist` is None or `now.mono − last_persist ≥
  checkpointIntervalMs` (15 s). The shell applies Persist with the **same `Now`**
  it passed to that engine call, and calls `mark_persisted(now)` only after a
  successful save. On failure it logs, and the next running tick retries.
  At most 15 s plus one tick of active time can be lost in a crash (README).
- `persisted_state(now)` and `snapshot(now)` never mutate. They fold a running
  session at `min(now.mono, baseline.mono)`, never past the last observation.
  The shell's `get_snapshot` handler runs `system(Tick, now)` first and applies
  its effects.
- `quit` pauses the session (reason quit) and always emits Persist.
- `Engine::new` steps:
  1. Sanitize settings.
  2. Reconcile friendship (§5).
  3. Dedupe history by id, keeping the first.
  4. Drop the checkpoint (and Persist) if its id is in history or awards, or if
     `plannedMs` is outside `[min, max] × 60000`. Clamp `accrued ≤ planned`, and
     clamp `pausedAt` and `ageAnchor` to ≤ now.wall.
  5. Restore the session **paused**: a running checkpoint becomes
     `Paused{restored, pausedAt = ageAnchor = checkpointAt}`; a paused one keeps
     its fields. A focus checkpoint with `accrued == planned` instead runs the
     completion transaction silently: `endedAt = checkpointAt`, award, no sound,
     phase idle, no transition.
  6. `expire_if_due(now)`.
  7. `baseline = Some(now)`, `suspended = false`, `activity = none`,
     `hidden_for_focus = false`.

  Effects: Persist if anything changed, and `Restored` only if a session
  remains. Never ShowCompanion or ActivityChanged (the shell shows the companion
  at launch).
- `awaitingChoice` is not persisted. A relaunch shows idle.

### 4.8 App Nap and idle sleep (`ActivityLevel`)
At the end of every call the engine computes the level (`focus` when a focus
session is running, `break` when a break is running, otherwise `none`). It emits
`ActivityChanged{level}` exactly when the level differs from the last one
emitted. The shell maps `focus` → `NSProcessInfo` activity `UserInitiated`
(prevents App Nap and *idle* system sleep, so reading quietly never pauses the
timer; display sleep and closing the lid still work). It maps `break` →
`UserInitiatedAllowingIdleSystemSleep`, and `none` → end the activity.

### 4.9 Effects, order and revision
Within one call, effects are ordered: `Persist`, `SettingsChanged`,
`Show/HideCompanion`, `ActivityChanged`, `Transition`, `PlaySound`. At most one
Persist per call. The shell saves once, applies the rest in order, then emits
exactly one `focus://snapshot`.
- Every entry into Paused, for any reason, emits `Transition::Paused{reason}`.
  Every resume emits `Resumed`.
- StartFocus: Persist, SettingsChanged (if lastFocusMinutes changed),
  HideCompanion (if hiding), ActivityChanged, FocusStarted.
- StartBreak: Persist, ActivityChanged, BreakStarted.
- Dismiss: Dismissed, with no Persist.
- UpdateSettings: Persist, SettingsChanged.
- `revision += 1` whenever a call changes observable state: phase, run status,
  session id, settings, history length, friendship totals, or awaiting. Folding
  active time and moving the baseline do not bump it. `snapshot()` is pure.
  The shell stamps every Snapshot it emits or returns with a strictly increasing
  `seq` under the engine lock.

### 4.10 Settings
`Settings` persists every field. Two patch types exist:
- `UserSettingsPatch` is the IPC `update_settings` argument. Its fields are
  `sound`, `reducedMotion`, `hideDuringFocus`, `menuBarTitle`, `palette` and
  `lastFocusMinutes`.
- `SettingsPatch` is internal to the shell. It adds `hotkey` (only via
  `set_hotkey`), `activePack` (only via `set_active_pack`) and `widgetPosition`
  (only from window-moved events).

Validation: minutes in range; pack id matches `^[a-z0-9][a-z0-9_-]{0,63}$`;
palette ∈ {"auto", "forest"} ∪ the unlocked palette-kind ids (checked by the
engine, else `InvalidSettings`); hotkey ≤ 64 chars when set; finite positions.
Turning `hideDuringFocus` off while `hidden_for_focus` is set emits
ShowCompanion and clears the flag. Turning it on mid-session takes effect at the
next start. Any user `show_companion`/`hide_companion` clears `hidden_for_focus`
(the shell calls `engine.clear_hidden_for_focus()`).

## 5. Friendship and unlocks

`data/unlocks.json` holds `rules {pointsPerActiveMinute, completionBonus,
longSessionMinutes}` and an ordered `unlocks` list. Each entry has `{id, points,
kind, trigger, label, description, palette?}`; palette-kind entries carry a
default `palette` (PaletteSpec), so unlocking a palette always changes colors.
Thresholds grow about 1.45×. Extend the list in the data file, not in code.

`Friendship { totalPoints, unlocked: set, awards: map<sessionId, AwardRecord> }`
- `award(id, activeMs, completed)` returns `None` if `id` was already awarded.
  Otherwise it records the award, adds points plus the bonus, and returns the
  newly unlocked ids.
- `reconcile` on load: `totalPoints = max(stored, sum(awards))`; `unlocked = stored
  ∪ {thresholds ≤ totalPoints}`. Nothing is ever removed.
- Breaks and character clicks earn nothing.

**Presentation triggers.** The renderer may read local time, for presentation
only (`src/theme/localTime.ts`):

| trigger | when |
|---|---|
| `lateNight` | 23:00–04:59 |
| `afterSunset` / `evening` | `SUNSET_HOUR[month]` to 22:59, `SUNSET_HOUR = [17,17.5,18,19.5,20,20.5,20.5,20,19,18,17,16.5]` (northern-hemisphere approximation) |
| `earlyMorning` | 05:00–07:59 |
| seasons | spring Mar–May, summer Jun–Aug, `autumnSeason` Sep–Nov, `winterSeason` Dec–Feb, `springSeason` |
| `summerEvening` | summer ∧ afterSunset |
| `rainyIdle` | `fnv1a32(today.date) % 4 == 0` (offline; no weather API) |
| `lateNightClear` | lateNight ∧ ¬rainyIdle |
| `focusStart`, `breakStart`, `focusComplete`, `harvest`, `longSessionComplete`, `breakEnd` | transitions (§7.1) |
| `manual` / unknown | never automatic (palettes: pick in menu; animations: replay only) |

Precedence:
- Palette `auto`: time of day (evening, dawn) > season (autumn, snow, bloom) >
  `forest`.
- Idle variant: sleepy > stargaze > lantern > firefly > umbrella > idle.

A candidate counts only if it is unlocked, and an idle variant only if its
animation resolves in the active pack. Effective palette = built-in `forest` ←
the unlock's default palette ← the pack's `palettes[id]`, overridden field by
field.

## 6. History and today's harvest

`SessionRecord { id, kind: focus, taskLabel?, startedAt, endedAt, plannedMs,
activeMs, outcome: completed|interrupted, endReason: completed|userEnded|pauseExpired,
endedUtcOffsetMinutes? }`. Only focus sessions are recorded. These are timer
records, not proof of attention.

`TodaySummary` is **derived** at snapshot time. "Today" is the local calendar day
of `endedAt`, using `endedUtcOffsetMinutes` when present and
`Clock::local_offset(endedAt)` otherwise. Today's own date uses
`local_offset(now)`. The harvest has one `acorn` per completed record and one
`leaf` per interrupted record, in history insertion order (never sorted by
timestamp). The UI shows the first 12, then a `+N` chip (`N = total − 12`).
`focusMs` is the sum of today's record active time. `activeFocusMs` is the
current focus session's active time. `focusLabel` is `floor((focusMs +
activeFocusMs) / 60000)` formatted as "1 h 05 min", "25 min" or "0 min", and both
the tray and the History tab use it.

## 7. Character packs

A pack is a folder containing `pack.json` and sprite sheets (PNG, WebP, GIF, JPEG
or SVG).

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
               "row": 0, "column": 0, "sheetColumns": 4, "sheetRows": 1,
               "stillFrame": 0, "frameDurationsMs": null }
    // states: idle, focus, paused, completed, break; extras: react (click), blink
    // unlock ids: wave, stretch, umbrella, sleepy, celebrate, acorn_gift, long_session, …
  },
  "fallbacks": { "paused": "idle" },   // optional overrides of the built-in chain
  "palettes": { "evening": { "background": "#…", "surface": "#…", "text": "#…",
                "muted": "#…", "accent": "#…", "ring": "#…", "ringTrack": "#…",
                "characterFilter": "brightness(0.9) saturate(0.9)" } }
}
```
Defaults: `row`, `column` and `stillFrame` default to 0; `sheetColumns` to
`column + frames`; `sheetRows` to `row + 1`; `loop` to true. `frameDurationsMs`
(length == frames) overrides fps when present. Frames run left to right within
one row.

**Resolution** (`resolve(key)`): the pack's own animation → the pack's
`fallbacks` chain → the built-in chain (`focus, paused, break, completed → idle`;
`react`, `blink` and every unlock id → none) → `None`. Cycles are cut. `None` for
a base state means the renderer draws its built-in static companion, which must
look intentional.

**Render keys** (`render_keys(table, manifest)`): BASE_STATES ∪ EXTRA_STATES ∪
every unlock id of kind animation or idleVariant ∪ every key in the pack's
`animations`. `get_active_pack` and `focus://pack-changed` resolve exactly these
keys. Every key is present in the map, with `null` when unresolved.

**Sources and precedence**:
- bundled `placeholder`: embedded in the binary, always available, id reserved;
- user packs: `<appData>/packs/<dir>/pack.json`;
- debug builds only: `packs-local/<dir>/pack.json` (gitignored), which overrides
  user packs with the same id.

A directory holding `pet.json` and no `pack.json` is loaded as a Petdex pack
(§7.2). Invalid packs are listed with their errors and cannot be activated
(`PackUnavailable`). If the active pack is missing at runtime, the placeholder
renders, `fallbackFrom` is set, and the setting is kept.

**Protocol**: files are served as `pack://localhost/<packId>/<path>` (on Windows,
`http://pack.localhost/…`). Path segments are percent-encoded in URLs and
decoded by the handler, which rejects absolute paths, `..`, backslashes, NUL,
hidden components, disallowed types, files over 16 MB, and anything that
resolves outside the pack directory (symlinks are canonicalized). Responses carry
`Content-Type`, `Cache-Control: no-cache` and `Access-Control-Allow-Origin: *`.
A refusal returns 404. Sprites render only as CSS backgrounds, never inline SVG.
CSS math uses percentages:
`background-size: sheetColumns×100% sheetRows×100%`,
`background-position: c/(sheetColumns−1)×100% row/(sheetRows−1)×100%` (0% when
the count is 1). Pixelated sampling is used only when
`rendering = pixelated` and the effective scale is ≥ 1.

### 7.1 Character state mapping (renderer, `src/character/mapping.ts`)
Base key:
- idle → the chosen idle variant (§5), otherwise `idle`
- focus running → `focus`; focus paused → `paused`
- break running → `break`; break paused → `paused`
- awaitingChoice{after: focus} → `completed`; awaitingChoice{after: break} → `idle`

A base animation with `loop: false` plays once and holds its last frame.

One-shot flourishes play only if the id is in `friendship.unlocked` **and** the
pack resolves it. They play their frames once regardless of `loop`, are capped
at 4 s, and never play under reduced motion:
- focusStarted → `wave`
- breakStarted → `stretch`
- focusCompleted → `long_session` if longSession, otherwise `celebrate`; then
  `acorn_gift`
- breakFinished → `hum`
- others → none

A character click → `react` in any phase. It makes no IPC call and earns
nothing. If `react` is null, a CSS-only 300 ms hop plays instead. `blink` is a
one-shot that replaces idle or focus every 5–9 s; it is skipped when null.
Flourishes are presentation timers only. They never gate, delay or issue
commands. Replays (`focus://play`) use the flourish path.

**Reduced motion** is `settings.reducedMotion || prefers-reduced-motion`. Under
it the companion shows the base key's `stillFrame`, with no flourishes, blink,
hop or ring transition. A click shows a static speech bubble for about 1.5 s.
The timer and controls are unchanged.

### 7.2 Petdex / Codex pet format (optional loader)
`pet.json` (`id`, `displayName`, `description`, `spriteVersionNumber` 1|2,
`spritesheetPath`) plus a spritesheet: 8 columns × 9 rows (v1) or 11 rows (v2),
192×208 px cells. The converter maps fixed rows onto our keys. It always sets
`sheetColumns = 8` and `sheetRows = 9|11` explicitly, `rendering = smooth`,
license and author null (with the warning "no license declared in pet.json").
It never fetches anything. The row table and research are in
`docs/PACK_FORMAT.md`.

## 8. Shell behavior (macOS)

### 8.0 Spike status
**Pending manual verification on a Mac**: type in TextEdit, click a companion
button, keep typing. Both window paths ship. The default is the panel;
`FOCUS_WINDOW_MODE=native` (read at runtime) builds a plain always-on-top window
(`.always_on_top(true).visible_on_all_workspaces(true).focusable(false)
.focused(false).accept_first_mouse(true).skip_taskbar(true)`) for comparison.
`get_app_info().windowMode` reports `panel`, `native` or `other`. All companion
window code lives in `src-tauri/src/companion.rs` behind `create/show/hide/
position` functions, so an Electron fallback stays contained.

### 8.1 Companion panel rules
1. `.plugin(tauri_nspanel::init())`. One `tauri_panel!` invocation, in its own
   module, with config `can_become_key_window: false`,
   `can_become_main_window: false`, `is_floating_panel: true`.
2. Build with `WebviewWindowBuilder::new(app, "companion",
   WebviewUrl::App("companion.html".into())).inner_size(240., 200.)
   .resizable(false).decorations(false).transparent(true).shadow(false)
   .skip_taskbar(true).visible(false).focused(false).accept_first_mouse(true)`.
3. Convert with `to_panel::<CompanionPanel>()` and configure through
   tauri-nspanel only:
   - `add_style_mask(NonactivatingPanel)`
   - level `NSFloatingWindowLevel`
   - `set_hides_on_deactivate(false)`, `set_works_when_modal(true)`
   - collection behavior `CanJoinAllSpaces | FullScreenAuxiliary | Stationary | IgnoresCycle`
   - `set_transparent(true)`, `set_has_shadow(false)`
4. Show = `panel.show()` (orderFrontRegardless). Hide = `panel.hide()` (orderOut).
   Both run on the main thread.
5. Never call `set_focus`, `show`, `set_decorations`, `set_resizable`,
   `set_focusable`, `to_window` or `set_event_handler` on the companion. Use
   `on_window_event` for Moved and CloseRequested (prevent close and hide).
6. First click is handled by `accept_first_mouse(true)` (wry's WKWebView
   override). Hover is not guaranteed in a non-key panel, so controls must read
   as controls without hover.
7. **The companion has no text, number or select inputs.** It uses only
   `<button>` elements with `tabIndex=-1`. That is how every widget control
   leaves keyboard focus in the user's app. The task label and typed custom
   minutes live in Settings → Start, a normal window where activating the app is
   expected.
8. Header layout: `[grip span with data-tauri-drag-region="deep", flex:1,
   ≥28 px tall, no interactive descendants][menu button][hide button]`.

### 8.2 Other shell rules
- **Activation**: `let mut app = builder.build(ctx)?;`
  `#[cfg(target_os="macos")] app.set_activation_policy(Accessory);` before
  `app.run`, plus `activate_ignoring_other_apps(false)` (macOS only, gated by
  cfg). `src-tauri/Info.plist` sets `LSUIElement` true. There is no Dock icon.
- **Settings window**: label `settings`, URL `settings.html#<tab>`
  (tab ∈ general|start|history|companion). It is created on demand, activated
  with `set_focus()` (activation is intended here), and **destroyed on close**.
  `focus://open-tab` is emitted only when the window already exists; the
  renderer reads the initial tab from `location.hash`.
- **Quit/shutdown**: one idempotent `shutdown(app)`, guarded by an AtomicBool,
  that runs `command(Quit)` and saves synchronously. It is called from
  `RunEvent::Exit`, which fires for Cmd+Q, logout and `app.exit`. It runs on the
  main thread, so it must not round-trip to the main thread. Tray Quit and
  `quit_app` call `app.exit(0)`. `RunEvent::ExitRequested{code: None}` →
  `prevent_exit()`. Keep Tauri's default macOS menu, which supplies Cmd+Q and
  Edit shortcuts for the settings inputs.
- **Single instance**: `tauri_plugin_single_instance` is the **first** plugin;
  its callback shows the companion on the main thread. Store and Engine are
  created only inside `.setup()`. `RunEvent::Reopen` shows the companion. Dev
  runs use `src-tauri/tauri.dev.conf.json` (identifier `….dev`, name "… Dev"), so
  dev never touches real state.
- **Position**: positions are logical AppKit points, with the origin at the
  top-left of the primary display. Work areas are
  `m.work_area().{position,size}.to_logical(m.scale_factor())` for each
  available monitor. The window position is
  `outer_position()?.to_logical(w.scale_factor()?)`, and moves use
  `set_position(LogicalPosition)`. `geometry::ensure_visible` runs at startup and
  on `NSApplicationDidChangeScreenParametersNotification`. A recovery move does
  not overwrite the saved preferred position (the next Moved event is ignored).
  User Moved events are debounced (~500 ms) into `widgetPosition`.
- **Tray**: the menu is built once with stable ids (`start-last`, `start-15`,
  `start-25`, `start-45`, `custom`, `pause-resume`, `end`, `show-hide`, `today`,
  `settings`, `quit`). After that, only `set_text` and `set_enabled` change it;
  `set_menu` is never called again. The title is `remainingLabel` while a session
  exists, prefixed `"\u{23F8}\u{FE0E} "` when paused. With no session, or with the
  title turned off, it is `set_title(None)`. The status button uses a
  monospaced-digit font. The icon is the `icons/tray-template.png` template
  image. "Custom duration…" shows the companion and emits `focus://open-picker`.
  "Today: {focusLabel} focused" is a disabled label item.
- **Hotkey**: the default is `Ctrl+Alt+T` (⌃⌥T). A binding is valid iff
  `Shortcut::from_str` succeeds and it has a Ctrl, Alt or Super modifier
  (F-keys exempt). The stored form is canonical `Ctrl+Alt+Shift+Super+<Key>`.
  Bindings are compared by parsed `Shortcut` equality. `set_hotkey(Some)`:
  validate, unregister the old binding, register the new one; on failure,
  re-register the old one and return `InvalidSettings`. `set_hotkey(None)`
  disables. The handler acts only on `Pressed`. While
  `set_hotkey_capture(true)` is active (the settings recorder), the binding is
  unregistered; it is restored on false, on settings close, or after 30 s.
  Startup registration failures keep the setting and are reported in
  `AppInfo.hotkeyError`.
- **Observers**: `NSWorkspaceWillSleepNotification` and `DidWake` are posted on
  `NSWorkspace.sharedWorkspace.notificationCenter`.
  `NSSystemClockDidChangeNotification` and
  `NSApplicationDidChangeScreenParametersNotification` are posted on
  `NSNotificationCenter.defaultCenter`. Observers use `queue: None` (so blocks run
  synchronously on the main thread), and their tokens are kept for the app's
  lifetime. WillSleep runs `system(Suspend)` and saves before it returns.
- **Sound**: a short original soft chime (an embedded WAV) played with `NSSound`
  on the main thread.
- **Ticker**: a dedicated thread sleeps `next_tick_delay_ms`:
  - running: until the next second boundary (50–1000 ms)
  - paused: 15 s
  - idle or awaiting: until the next local midnight, capped at 60 s

  A tick emits a snapshot only when `remainingLabel`, `phase`, `revision` or
  `today.date` changed.

## 9. Persistence

There is one file: `<appDataDir>/state.json`, schema-versioned
(`schemaVersion: 1`), and written atomically (write `state.json.tmp`, fsync,
rename, fsync the directory). It holds `settings`, `activeSession` (the
checkpoint), `history` and `friendship` together, so they stay consistent.
- On the first save of each process, the existing `state.json` is copied to
  `state.prev.json`.
- Load:
  - missing → defaults;
  - corrupt → moved to `state.corrupt-<UTC stamp>.json`, then `state.prev.json`
    is tried (`RecoveredFromPrevious`), then defaults;
  - newer schema → backed up, defaults used, **and saving disabled for this run**
    (read-only), so a newer file is never replaced with lower totals;
  - older schema → migrated.
- Never delete a user's state file.

## 10. IPC surface

Commands. Each window's capability allows only the commands that window uses.

| Command | Purpose | Companion | Settings |
|---|---|---|---|
| `get_snapshot` | | ✓ | ✓ |
| `start_focus{minutes?, taskLabel?}` | | ✓ | ✓ |
| `start_break{minutes?}` | | ✓ | ✓ |
| `pause`, `resume`, `end_session`, `dismiss` | | ✓ | ✓ |
| `update_settings{patch: UserSettingsPatch}` | | | ✓ |
| `set_hotkey{accelerator: string\|null}` | | | ✓ |
| `set_hotkey_capture{active}` | | | ✓ |
| `get_history{limit?}` | | | ✓ |
| `list_packs` | | | ✓ |
| `get_active_pack` | | ✓ | ✓ |
| `set_active_pack{id}` | | | ✓ |
| `replay_animation{key}` | refused with `FocusRunning` while a focus session is running; the key must be unlocked (else `NotUnlocked`), or be `react`; shows the companion if hidden | ✓ | ✓ |
| `show_companion` | | | ✓ |
| `hide_companion` | | ✓ | ✓ |
| `reset_companion_position` | | | ✓ |
| `open_settings{tab?: SettingsTab}` | | ✓ | |
| `get_app_info` | name, version, dirs, windowMode, debug, hotkeyError, unlock table, rules, limits — static data moved out of the per-tick snapshot | ✓ | ✓ |
| `quit_app` | | | ✓ |

Events (names in `focus_core::events`, mirrored in `src/ipc/events.ts`):
`focus://snapshot` (Snapshot), `focus://transition` (Transition),
`focus://pack-changed` (ResolvedPack), `focus://play` (PlayEvent{key}),
`focus://open-tab` (OpenTabEvent{tab}), `focus://open-picker` (no payload).
The History tab refetches on mount and on `focusCompleted`/`focusInterrupted`.

## 11. Testing and reporting

- `focus_core::testkit` (behind `cfg(test)` or the `testkit` feature) provides
  a harness:
  - `H { engine, clock, last_saved, fx }`, which applies Persist into
    `last_saved` and calls mark_persisted;
  - `cmd`, `ev`, `tick`, `run(ms)` (1 s steps with ticks), `run_step(ms, step)`,
    `snap`, and `relaunch(closed_ms, mono_counts_sleep)`;
  - a counter id source (`s1`, `s2`, …), `transitions(&fx)`, and
    `sleep_mono_stopped` / `sleep_mono_counting` helpers.

  **Engine tests must tick at least every 60 s of fake time while a session runs.**
- Fake-clock unit tests cover:
  - every timer transition and the call pipeline (overtaken intents);
  - suspend/wake (with and without notifications, mono stopped and counting),
    gap and clock-jump handling;
  - relaunch recovery, pause expiry, thresholds;
  - duplicate completion and reward handling;
  - persistence round trips, corruption, prev-file recovery, read-only mode;
  - pack parsing, resolution, render keys, path safety and Petdex conversion;
  - geometry recovery, countdown formatting, label normalization.
- Renderer tests (Vitest) cover formatting, harvest capping, palette and
  idle-variant choice, state and transition → animation mapping, sprite math, and
  hotkey recording → accelerator.
- Checks: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
  `npm run typecheck`, `npm run lint`, `npm test`, `npm run build` (which asserts
  no dev mock is in `dist/`), and `npm run check:macos`.
- Reports always separate "automated checks passed" from "needs manual macOS
  verification". Manual checks are never marked passed by an agent.
