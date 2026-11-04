# Manual macOS verification checklist (all items pending)

Run this on a real Mac. Record PASS or FAIL with the date, macOS version, Mac model and build in `NOTES.md`. Never mark a step passed unless it was run on a Mac. The `[A#]`/`[T#]` tags are the matrix IDs each step closes.

## 0. Setup

1. Build and check the app:
   - `npm ci && npm run check && npm run check:macos`. Record the results; the `cargo test` run on macOS repeats the automated matrix.
   - `npm run app:build` builds the unsigned `target/release/bundle/macos/Totoro Focus.app` and the `.dmg`.
   - Copy the app to `/Applications`. If it came from CI, run `xattr -dr com.apple.quarantine "/Applications/Totoro Focus.app"`.
2. Set up a clean data folder:
   - `DATA="$HOME/Library/Application Support/io.github.jc011118.totorofocus"; LOGS="$HOME/Library/Logs/io.github.jc011118.totorofocus"`.
   - For a clean run, move `$DATA` aside with `mv "$DATA" "$DATA.bak-$(date +%s)"`. Never delete it. Dev runs use the `….totorofocus.dev` folder instead.
3. Launch the app and get its process ID:
   - `APP=$(pgrep -f "Totoro Focus.app/Contents/MacOS/")`.
   - Follow the log with `tail -f "$LOGS"/*.log`.

## 1. Milestone-0 spike: non-activating panel [A2]

4. Check the launch state:
   - No Dock icon appears, and Cmd+Tab does not list the app.
   - The companion appears near the bottom-right of the primary display.
   - The log says `companion created (panel mode)`.
5. Open TextEdit and a new document, and type continuously.
6. Without stopping typing, click each companion control in turn:
   - Start (last), a preset, Custom length → More/Fewer → Back, Pause, Resume, End.
   - The character.
   - ⋯ menu → Close menu.
   - Hide; then restore it from the tray.

   After each click, characters must keep landing in TextEdit, TextEdit's title bar must stay active, and the menu-bar app name must stay "TextEdit". Each control must work on the first click.
7. Expected exceptions: the pencil button ("Add a task in Settings") and ⋯ → companion settings open and activate the Settings window. Record that this happens.
8. Drag by the grip strip (the header area left of ⋯): the window moves and TextEdit keeps focus. Pressing on a button does not drag.
9. Check floating and Spaces:
   - Maximize TextEdit over the companion: the companion stays on top.
   - Switch Spaces: it shows on every Space.
   - Enter a full-screen app: it shows over it.
10. Click a fully transparent corner inside the 240×200 window and record whether the click reaches the app beneath.
11. Repeat steps 5–8 with `FOCUS_WINDOW_MODE=native npm run app:dev` and record which mode keeps focus.
    - If panel mode fails after a reasonable attempt, apply the Electron fallback rule and record it in CLAUDE.md.

## 2. Menu bar [A1, A12, A3]

12. Idle: the status item shows the template icon only, and it reads correctly in both light and dark menu bars.
13. Check the tray menu:
    - It lists Start last duration (25 min), 15, 25, 45, Custom duration…, Pause, End, Show/Hide companion, Today: … focused (disabled), Settings…, Quit.
    - "Start last duration" starts with no task prompt.
    - The title shows `25:00`, then `24:59`, and so on. It ticks once a second and the digits do not shift in width.
14. Pause from the tray: the title becomes `⏸ mm:ss` (a text glyph, not an emoji) and stays frozen for at least 60 s. Resume continues from the same value. While a session exists, the Start items are disabled.
15. In Settings → General, turn "Countdown in the menu bar" off: the title disappears at once and only the icon stays. Turn it back on and the title returns.

## 3. Hotkey [A1]

16. With TextEdit focused:
    - ⌃⌥T when idle starts the last duration, and focus stays in TextEdit.
    - ⌃⌥T while running pauses; pressing it again resumes.
    - On the completion screen, ⌃⌥T starts a new session.
17. Rebinding:
    - In Settings → General → Change…, press ⌃⌥K. ⌃⌥K now works and ⌃⌥T does nothing.
    - "Disable shortcut" turns the hotkey off.
    - Binding a shortcut another app already owns shows a calm error.
    - After a relaunch, the binding is kept.

## 4. Widget, hide, reload, single instance [A1, A4, T1]

18. From idle, the widget's Start button starts the last duration without a task.
19. Hide and restore during a session:
    - Start a session, Hide the widget, wait 2 min, then tray → Show companion.
    - The countdown equals the tray title, and the session is the same one: no restart, no jump.
20. Hide during focus:
    - Turn on "Hide companion during focus" and start a 1-min session.
    - The widget hides, and the tray title keeps counting.
    - At completion the widget returns and celebrates.
21. Reload while running:
    - Before launching, record the WebKit processes with `pgrep -fl com.apple.WebKit.WebContent`. After launch, the new PID is the companion's.
    - With a session running, `kill -9 <that pid>`.
    - The companion reloads and shows the same remaining time. The tray never jumps, and there is no second countdown.
22. Single instance: launch the .app again from Finder. `pgrep -f "Totoro Focus.app/Contents/MacOS/"` still shows one PID, the companion is shown, and the session is unchanged.
23. Covered and idle: leave the companion hidden or covered for more than 10 min during a session.
    - The tray still updates every second.
    - The session is not paused.
    - Activity Monitor → Energy shows App Nap = No while focus runs.

## 5. Sleep and wake [A5, T2, T5]

24. Sleep during a session:
    - Start 25 min. After about 2 min, note the remaining time, then use Apple menu → Sleep (or `pmset sleepnow`). Wait at least 5 min, then wake.
    - The session is paused with a sleep reason and the same remaining time. Nothing resumes automatically.
    - The log shows `system will sleep` and `system did wake`. Resume continues from the noted value.
25. Repeat with a lid close (laptop). Then repeat with an already paused session: it stays paused.
26. Hands-off run:
    - Set the display to turn off after 1–2 min, then start 25 min and leave the Mac untouched.
    - The display sleeps, but the Mac does not idle-sleep, and the session completes.
    - `pmset -g assertions` lists "Focus session running" during focus and not when paused or idle.

## 6. Quit, crash and relaunch [A6, T3, A13]

27. Quit and relaunch:
    - Run 3 min, then tray → Quit. Wait 5 min and relaunch.
    - The session is paused at the same remaining time (quit reason), with no minutes gained.
    - Repeat with Cmd+Q while Settings is frontmost.
28. Crash and relaunch:
    - With a session running, `kill -9 $APP` about 20 s after a quarter-minute mark. Wait 5 min and relaunch.
    - The session is paused, and remaining time is at most about 16 s more than at the kill. No points were earned for the closed time.
29. Log out and back in with a session running: it is restored paused.

## 7. Clock changes [T4]

30. Change the clock during a session:
    - In System Settings → General → Date & Time, turn off automatic time and set the clock forward 2 h.
    - The running session pauses (clock jump): no completion and no points. Resume continues from the same value.
    - Set the clock back 2 h: there is no completion and no gain.
    - Turn automatic time back on.

## 8. Completion, interruption, break and sound [A7, A8, A10]

31. Completion:
    - A custom 1-min session completes once, with the celebration, the soft chime (sound on) and one acorn.
    - The choices offered are a 5-min break, another session, or Done. Nothing starts by itself.
    - Settings → Companion shows +6 points. A relaunch does not change that total.
    - With sound off, there is no chime.
32. Early end: End a session after at least 1 min. A leaf appears with "Saved N min · +N". There is no sound, no celebration and no scolding.
33. Break:
    - The 5-min break shows a separate countdown in the resting look and has only End break.
    - When it ends, "Break's over" offers the presets again. Points are unchanged.
34. Character click: the character reacts (or shows a bubble under Reduced motion) and earns nothing.

## 9. Display scaling and disconnect

35. Scaling:
    - On the built-in Retina display (2×) and on an external 1× display (or a scaled "More Space" mode), the companion is about 240×200 points.
    - Text and ring are crisp, and the sprites are not blurry.
36. Saved position on an external display:
    - Drag the companion to the external display, quit and relaunch: it comes back there.
    - Unplug the display while the companion is on it: within about 1 s it moves fully onto the built-in display, and the log says `display change: moving the companion`.
    - Replug: it returns to the saved spot.
37. Usable area:
    - Change the display arrangement, Dock size and position, and menu-bar auto-hide: the companion stays fully visible.
    - The Settings reset-position control puts it bottom-right on the primary display.

## 10. Sprites in the bundled .app and user packs [A11]

38. In the release .app (not dev), the placeholder character animates in idle, focus, paused, break and completed. It is not the static fallback. Settings → Companion lists Placeholder first.
39. User pack:
    - `cp -R crates/focus-core/tests/fixtures/packs/fixture-sprout "$DATA/packs/"`, then Settings → Companion → Rescan → select it.
    - The companion switches without a restart.
    - States without art fall back: for example, paused uses idle art.
    - Remove the folder and relaunch: the placeholder renders and the setting is kept. Repeat with `fixtures/petdex/fixture-pet`.
40. Reduced motion on: the character holds a still pose, clicks show a bubble, and the timer is unaffected.

## 11. Persistence [A13]

41. Persist and relaunch:
    - Change sound, reduced motion, the menu-bar title, the hotkey, the pack, the palette and the widget position, and complete one session. Quit and relaunch.
    - Everything is kept, and Settings → History shows the session and today's time.
    - `$DATA/state.json` has `"schemaVersion": 1`. After the second launch's first save, `$DATA/state.prev.json` exists.

## 12. No network [A15]

42. Offline: turn Wi-Fi off and unplug Ethernet, then launch and run a 1-min session. Everything works.
43. Online: `lsof -nP -i -a -p $APP` prints nothing. Repeat for each WebKit PID that belongs to the app.

## 13. Settings window never stops the timer

44. During a session, open and close Settings several times. The session keeps running, and the app does not quit when Settings closes.

## 14. Idle CPU and memory [A14]

45. Use the release .app, launched at least 2 min earlier, idle with no session, companion visible, Reduced motion off, nothing touched.
    - The companion's WebKit PIDs are the `pgrep -fl com.apple.WebKit` entries that appeared after launch: WebContent, plus Networking and GPU if present.
46. Measure CPU and memory:
    - **Average CPU over 10 min:** `ps -o cputime= -p $APP` at t0 and at t0+600 s gives Δseconds ÷ 600 × 100 %. Do the same for each WebKit PID.
    - **Snapshot:** `ps -o pid,%cpu,rss,command -p $APP,<webkit pids>`.
    - **top:** `top -l 2 -s 60 -pid $APP -stats pid,command,cpu,mem,idlew,power`, using the second sample. `mem` is the physical footprint.
    - **Activity Monitor:** read % CPU and Idle Wake Ups (CPU tab), Memory (Memory tab), and Energy Impact and App Nap (Energy tab), for the app and its Web Content process.
47. Repeat step 46 for:
    - companion hidden;
    - Reduced motion on;
    - focus running with the companion visible;
    - focus running with the companion hidden;
    - paused.
48. Record the date, macOS version, Mac model and app version, plus a table of the numbers for each scenario, in README "Measured resource use". Copy the results of this checklist into NOTES.md.
