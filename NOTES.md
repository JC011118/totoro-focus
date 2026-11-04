# NOTES — open items and pending manual checks

Keep this file current. Anything not run on a real Mac is **pending**, never
passed. Record manual results here as `PASS/FAIL — date, macOS version, Mac
model, commit`.

## Where things stand

- Milestones 0–4 are implemented. All automated checks pass in a Linux cloud
  container:
  - 511 Rust tests;
  - 143 Vitest tests;
  - clippy `-D warnings` on Linux and on `aarch64-apple-darwin`;
  - typecheck, lint, and a production build that contains no dev mock.

  See `docs/ACCEPTANCE.md`.
- **Nothing has been run on macOS yet.** The macOS-only code (NSPanel, sleep and
  wake observers, App Nap activity, NSSound, tray font) is type-checked and
  linted for the macOS target. Its behavior is unverified.
- A Linux smoke run of the real app under Xvfb passed. It drove the window with
  clicks and checked `state.json` after each step:
  - start, countdown, and pause holding the remaining time;
  - end → leaf;
  - `kill -9` → restored paused at the last 15 s checkpoint;
  - a 1-min session completing → +6 points once, an acorn, and break /
    another / done;
  - break, the menu popover, and the settings window.

  This is evidence for the shared logic and the IPC plumbing, not for macOS
  behavior.

## Open items

1. **Milestone-0 spike verdict (blocking the stack decision).** Run
   `docs/MANUAL_CHECKS.md` §1 (type in TextEdit, click companion controls, keep
   typing) in panel mode and in `FOCUS_WINDOW_MODE=native` mode. If panel mode
   cannot keep focus after a reasonable attempt, apply the Electron fallback
   rule in CLAUDE.md and record why there.
2. **Idle CPU and memory on a Mac** (`docs/MANUAL_CHECKS.md` §14). Fill in the
   README table. The README currently has only a Linux/Xvfb reference run.
3. **Signing and notarization**: not done. They need your Apple Developer
   account. The local `.app` and `.dmg` are unsigned (macOS will show a
   Gatekeeper warning; right-click → Open, or `xattr -dr com.apple.quarantine`).
4. **The Totoro pack is yours to supply.** Put it in `packs-local/<dir>/` (dev)
   or `~/Library/Application Support/io.github.jc011118.totorofocus/packs/<dir>/`,
   then Settings → Companion → Rescan. It must never be committed. Use
   `docs/PACK_FORMAT.md` for the format.
5. **Use it for a week** (milestone 5) and keep a friction log below. After that:
   testers, an original character, a public name and identifier (note that
   changing `identifier` moves the data directory), signing, and a release.
6. **Optional CI**: `.github/workflows/ci.yml` runs only when you start it
   manually (Actions → CI → Run workflow). It has a Linux check job and a macOS
   job that runs tests and builds an unsigned `.app` artifact. It does not run
   on push, so it costs no Actions minutes unless you start it.

## Candidate tweaks for the friction log

- Ending a session in its first seconds still records a leaf (literal reading of
  the brief). Consider a short "cancel" grace period if accidental starts
  happen.
- "Add a task" and the character menu's "Companion settings…" open the Settings
  window. That activates the app by design (typing a task needs keyboard focus).
  Every other companion control is non-activating.
- Sunset times are a fixed northern-hemisphere table, and "rainy days" are a
  date hash, because there is no network. Adjust if they feel wrong where you
  live.

## Pending manual macOS checks

The full numbered steps are in **`docs/MANUAL_CHECKS.md`**. Every item below is
**pending**.

| § | Area | Status |
|---|---|---|
| 1 | Spike: non-activating panel keeps keyboard focus in TextEdit; drag; floating over full-screen apps and all Spaces; transparent-corner clicks; native-mode comparison | pending |
| 2 | Menu bar: template icon, all menu items, `mm:ss` title with monospaced digits, `⏸` text glyph when paused, title off/on | pending |
| 3 | Global hotkey ⌃⌥T: idle start, pause, resume; rebinding; disabling; conflict error; kept after relaunch | pending |
| 4 | Widget start; hide and restore; hide during focus; WebContent reload keeps the same timer; single instance; App Nap stays off during focus | pending |
| 5 | Sleep and wake (Apple menu, lid): session pauses, earns nothing, does not resume by itself; a hands-off 25-min run completes with the display asleep | pending |
| 6 | Quit (tray, Cmd+Q, logout) → restored paused; `kill -9` → loses at most about 16 s | pending |
| 7 | Clock forward or back during a session never completes it or adds points | pending |
| 8 | Completion (celebration, chime, acorn, +6/+30 once), early end (leaf, no sound), break, character click | pending |
| 9 | Retina and 1× displays; saved position on an external display; unplugging moves the companion back on screen; Dock and menu-bar changes | pending |
| 10 | Sprites animate in the bundled `.app`; user pack and Petdex fixture pack load, fall back and switch live; reduced motion | pending |
| 11 | Every setting, friendship total and history entry persists across relaunch; `state.prev.json` is written | pending |
| 12 | Works fully offline; `lsof -i` shows no sockets | pending |
| 13 | Opening and closing Settings never stops the timer or quits the app | pending |
| 14 | Idle CPU and memory measured in five scenarios and recorded in the README | pending |
| — | Cmd+H with Settings focused leaves the companion visible and the tray label correct (`canHide = NO`) | pending |
| — | `npm run app:dev` with HMR works on the Mac (dev CSP) | pending |

## Manual results log

_(empty — add results here)_

## Friction log (milestone 5)

_(empty)_
