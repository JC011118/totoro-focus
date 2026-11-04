//! Menu-bar item (SPEC §8.2 Tray).
//!
//! The menu is built once with stable ids; afterwards only `set_text`,
//! `set_enabled` and `set_title` change it (never `set_menu`). The title is the
//! snapshot's `remainingLabel` while a session exists, prefixed
//! `"\u{23F8}\u{FE0E} "` when paused, and `None` with no session or when the
//! title is turned off. Updates are computed from snapshots off the main thread
//! and applied on the main thread from a latest-wins slot, so a slow tick can
//! never overwrite a newer label.

use std::sync::{Mutex, PoisonError};

use focus_core::engine::Command;
use focus_core::events::SettingsTab;
use focus_core::snapshot::{Phase, Snapshot};
use focus_core::timer::{RunStatus, SessionKind};
use tauri::image::Image;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, Wry};

use crate::{actions, platform};

pub const TRAY_ID: &str = "main";
pub const ID_START_LAST: &str = "start-last";
pub const ID_START_15: &str = "start-15";
pub const ID_START_25: &str = "start-25";
pub const ID_START_45: &str = "start-45";
pub const ID_CUSTOM: &str = "custom";
pub const ID_PAUSE_RESUME: &str = "pause-resume";
pub const ID_END: &str = "end";
pub const ID_SHOW_HIDE: &str = "show-hide";
pub const ID_TODAY: &str = "today";
pub const ID_SETTINGS: &str = "settings";
pub const ID_QUIT: &str = "quit";

/// Presets offered in the menu, by stable id.
pub const PRESETS: [(&str, u32); 3] = [(ID_START_15, 15), (ID_START_25, 25), (ID_START_45, 45)];

/// Pause glyph (text presentation) used before a paused countdown.
pub const PAUSED_PREFIX: &str = "\u{23F8}\u{FE0E} ";

// ---------------------------------------------------------------- pure model

/// Everything the tray shows for one snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayModel {
    pub title: Option<String>,
    pub start_last: String,
    /// Start items (last, presets, custom) are enabled in idle and awaitingChoice.
    pub can_start: bool,
    pub pause_resume: String,
    /// Pause/Resume and End are enabled while a session exists.
    pub has_session: bool,
    pub end: String,
    pub show_hide: String,
    pub today: String,
}

pub fn title_for(s: &Snapshot) -> Option<String> {
    if !s.settings.menu_bar_title {
        return None;
    }
    s.session.as_ref().map(|v| match v.status {
        RunStatus::Paused => format!("{PAUSED_PREFIX}{}", v.remaining_label),
        RunStatus::Running => v.remaining_label.clone(),
    })
}

pub fn model(s: &Snapshot) -> TrayModel {
    let paused = s
        .session
        .as_ref()
        .is_some_and(|v| v.status == RunStatus::Paused);
    let is_break = s
        .session
        .as_ref()
        .is_some_and(|v| v.kind == SessionKind::Break);
    TrayModel {
        title: title_for(s),
        start_last: format!(
            "Start last duration ({} min)",
            s.settings.last_focus_minutes
        ),
        can_start: matches!(s.phase, Phase::Idle | Phase::AwaitingChoice),
        pause_resume: if paused { "Resume" } else { "Pause" }.to_string(),
        has_session: s.session.is_some(),
        end: if is_break { "End break" } else { "End session" }.to_string(),
        show_hide: if s.companion_visible {
            "Hide companion"
        } else {
            "Show companion"
        }
        .to_string(),
        today: format!("Today: {} focused", s.today.focus_label),
    }
}

impl TrayModel {
    /// Item ids with their (changeable) text.
    fn texts(&self) -> [(&'static str, &String); 5] {
        [
            (ID_START_LAST, &self.start_last),
            (ID_PAUSE_RESUME, &self.pause_resume),
            (ID_END, &self.end),
            (ID_SHOW_HIDE, &self.show_hide),
            (ID_TODAY, &self.today),
        ]
    }
}

/// One menu or title change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Title(Option<String>),
    Text(&'static str, String),
    Enabled(&'static str, bool),
}

/// The calls needed to go from `old` (None: nothing applied yet) to `new`.
pub fn changes(old: Option<&TrayModel>, new: &TrayModel) -> Vec<Change> {
    let mut out = Vec::new();
    if old.map(|o| &o.title) != Some(&new.title) {
        out.push(Change::Title(new.title.clone()));
    }
    let old_texts = old.map(TrayModel::texts);
    for (i, (id, text)) in new.texts().into_iter().enumerate() {
        if old_texts.as_ref().map(|t| t[i].1) != Some(text) {
            out.push(Change::Text(id, text.clone()));
        }
    }
    if old.map(|o| o.can_start) != Some(new.can_start) {
        for id in [
            ID_START_LAST,
            ID_START_15,
            ID_START_25,
            ID_START_45,
            ID_CUSTOM,
        ] {
            out.push(Change::Enabled(id, new.can_start));
        }
    }
    if old.map(|o| o.has_session) != Some(new.has_session) {
        for id in [ID_PAUSE_RESUME, ID_END] {
            out.push(Change::Enabled(id, new.has_session));
        }
    }
    out
}

/// What a menu click does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    StartFocus(Option<u32>),
    Custom,
    PauseResume,
    End,
    ShowHide,
    Settings,
    Quit,
    None,
}

pub fn action_for(id: &str) -> MenuAction {
    if let Some((_, minutes)) = PRESETS.iter().find(|(pid, _)| *pid == id) {
        return MenuAction::StartFocus(Some(*minutes));
    }
    match id {
        ID_START_LAST => MenuAction::StartFocus(None),
        ID_CUSTOM => MenuAction::Custom,
        ID_PAUSE_RESUME => MenuAction::PauseResume,
        ID_END => MenuAction::End,
        ID_SHOW_HIDE => MenuAction::ShowHide,
        ID_SETTINGS => MenuAction::Settings,
        ID_QUIT => MenuAction::Quit,
        _ => MenuAction::None,
    }
}

// ---------------------------------------------------------------- runtime

struct Items {
    start_last: MenuItem<Wry>,
    start_15: MenuItem<Wry>,
    start_25: MenuItem<Wry>,
    start_45: MenuItem<Wry>,
    custom: MenuItem<Wry>,
    pause_resume: MenuItem<Wry>,
    end: MenuItem<Wry>,
    show_hide: MenuItem<Wry>,
    today: MenuItem<Wry>,
}

impl Items {
    fn get(&self, id: &str) -> Option<&MenuItem<Wry>> {
        Some(match id {
            ID_START_LAST => &self.start_last,
            ID_START_15 => &self.start_15,
            ID_START_25 => &self.start_25,
            ID_START_45 => &self.start_45,
            ID_CUSTOM => &self.custom,
            ID_PAUSE_RESUME => &self.pause_resume,
            ID_END => &self.end,
            ID_SHOW_HIDE => &self.show_hide,
            ID_TODAY => &self.today,
            _ => return None,
        })
    }
}

#[derive(Default)]
struct Slot {
    /// Latest model waiting to be applied, with its snapshot seq.
    pending: Option<TrayModel>,
    pending_seq: u64,
    applied: Option<TrayModel>,
}

pub struct TrayState {
    tray: TrayIcon<Wry>,
    items: Items,
    slot: Mutex<Slot>,
}

/// Build the tray once (setup, main thread).
pub fn build(app: &AppHandle, initial: &Snapshot) -> tauri::Result<()> {
    let m = model(initial);
    let item = |id: &str, text: &str, enabled: bool| {
        MenuItem::with_id(app, id, text, enabled, None::<&str>)
    };
    let items = Items {
        start_last: item(ID_START_LAST, &m.start_last, m.can_start)?,
        start_15: item(ID_START_15, "Start 15 min", m.can_start)?,
        start_25: item(ID_START_25, "Start 25 min", m.can_start)?,
        start_45: item(ID_START_45, "Start 45 min", m.can_start)?,
        custom: item(ID_CUSTOM, "Custom duration…", m.can_start)?,
        pause_resume: item(ID_PAUSE_RESUME, &m.pause_resume, m.has_session)?,
        end: item(ID_END, &m.end, m.has_session)?,
        show_hide: item(ID_SHOW_HIDE, &m.show_hide, true)?,
        today: item(ID_TODAY, &m.today, false)?,
    };
    let name = app.package_info().name.clone();
    let settings = item(ID_SETTINGS, "Settings…", true)?;
    let quit = item(ID_QUIT, &format!("Quit {name}"), true)?;
    let menu = Menu::with_items(
        app,
        &[
            &items.start_last,
            &items.start_15,
            &items.start_25,
            &items.start_45,
            &items.custom,
            &PredefinedMenuItem::separator(app)?,
            &items.pause_resume,
            &items.end,
            &PredefinedMenuItem::separator(app)?,
            &items.show_hide,
            &items.today,
            &PredefinedMenuItem::separator(app)?,
            &settings,
            &quit,
        ],
    )?;
    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!(
            "../icons/tray-template.png"
        ))?)
        .icon_as_template(true)
        .tooltip(&name)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event: MenuEvent| on_menu(app, event.id().as_ref()))
        .build(app)?;
    platform::style_tray(&tray);
    if let Some(title) = &m.title {
        tray.set_title(Some(title))?;
    }
    app.manage(TrayState {
        tray,
        items,
        slot: Mutex::new(Slot {
            pending: None,
            pending_seq: initial.seq,
            applied: Some(m),
        }),
    });
    Ok(())
}

fn on_menu(app: &AppHandle, id: &str) {
    match action_for(id) {
        MenuAction::StartFocus(minutes) => actions::run_logged(
            app,
            Command::StartFocus {
                minutes,
                task_label: None,
            },
        ),
        MenuAction::Custom => actions::open_picker(app),
        MenuAction::PauseResume => actions::pause_or_resume(app),
        MenuAction::End => actions::run_logged(app, Command::End),
        MenuAction::ShowHide => actions::toggle_companion(app),
        MenuAction::Settings => actions::open_settings(app, SettingsTab::General),
        MenuAction::Quit => app.exit(0),
        MenuAction::None => {}
    }
}

/// Queue the tray state for `snapshot` and apply it on the main thread.
pub fn update(app: &AppHandle, snapshot: &Snapshot) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    {
        let mut slot = state.slot.lock().unwrap_or_else(PoisonError::into_inner);
        if snapshot.seq <= slot.pending_seq {
            return; // older than what is already queued or applied
        }
        slot.pending_seq = snapshot.seq;
        slot.pending = Some(model(snapshot));
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || flush(&handle)) {
        log::debug!("tray update not scheduled: {e}");
    }
}

/// Main thread: apply the newest pending model.
fn flush(app: &AppHandle) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    let todo = {
        let mut slot = state.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(next) = slot.pending.take() else {
            return;
        };
        let todo = changes(slot.applied.as_ref(), &next);
        slot.applied = Some(next);
        todo
    };
    for change in todo {
        let result = match &change {
            Change::Title(title) => state.tray.set_title(title.as_deref()),
            Change::Text(id, text) => state.items.get(id).map_or(Ok(()), |i| i.set_text(text)),
            Change::Enabled(id, on) => state.items.get(id).map_or(Ok(()), |i| i.set_enabled(*on)),
        };
        if let Err(e) = result {
            log::warn!("tray update {change:?} failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use focus_core::snapshot::Phase;
    use focus_core::timer::{RunStatus, SessionKind};

    use super::*;
    use crate::shell::tests::{session, snapshot};

    fn running(label: &str) -> Snapshot {
        snapshot(
            1,
            Phase::Focus,
            Some(session(SessionKind::Focus, RunStatus::Running, label)),
        )
    }

    #[test]
    fn title_is_the_countdown_while_running() {
        assert_eq!(title_for(&running("24:59")), Some("24:59".into()));
    }

    #[test]
    fn title_has_a_text_pause_glyph_when_paused() {
        let s = snapshot(
            1,
            Phase::Focus,
            Some(session(SessionKind::Focus, RunStatus::Paused, "12:00")),
        );
        assert_eq!(title_for(&s), Some("\u{23F8}\u{FE0E} 12:00".into()));
        let b = snapshot(
            1,
            Phase::Break,
            Some(session(SessionKind::Break, RunStatus::Paused, "04:10")),
        );
        assert_eq!(title_for(&b), Some("\u{23F8}\u{FE0E} 04:10".into()));
    }

    #[test]
    fn no_title_without_a_session_or_when_turned_off() {
        assert_eq!(title_for(&snapshot(1, Phase::Idle, None)), None);
        assert_eq!(title_for(&snapshot(1, Phase::AwaitingChoice, None)), None);
        let mut off = running("10:00");
        off.settings.menu_bar_title = false;
        assert_eq!(title_for(&off), None);
    }

    #[test]
    fn menu_state_follows_the_phase() {
        let idle = model(&snapshot(1, Phase::Idle, None));
        assert!(idle.can_start && !idle.has_session);
        assert_eq!(idle.pause_resume, "Pause");
        assert_eq!(idle.start_last, "Start last duration (25 min)");
        assert_eq!(idle.today, "Today: 0 min focused");

        let awaiting = model(&snapshot(1, Phase::AwaitingChoice, None));
        assert!(awaiting.can_start && !awaiting.has_session);

        let focus = model(&running("20:00"));
        assert!(!focus.can_start && focus.has_session);
        assert_eq!(focus.pause_resume, "Pause");
        assert_eq!(focus.end, "End session");

        let paused = model(&snapshot(
            1,
            Phase::Focus,
            Some(session(SessionKind::Focus, RunStatus::Paused, "20:00")),
        ));
        assert_eq!(paused.pause_resume, "Resume");

        let brk = model(&snapshot(
            1,
            Phase::Break,
            Some(session(SessionKind::Break, RunStatus::Running, "05:00")),
        ));
        assert!(!brk.can_start && brk.has_session);
        assert_eq!(brk.end, "End break");
    }

    #[test]
    fn labels_follow_settings_visibility_and_today() {
        let mut s = snapshot(1, Phase::Idle, None);
        s.settings.last_focus_minutes = 45;
        s.companion_visible = false;
        s.today.focus_label = "1 h 05 min".into();
        let m = model(&s);
        assert_eq!(m.start_last, "Start last duration (45 min)");
        assert_eq!(m.show_hide, "Show companion");
        assert_eq!(m.today, "Today: 1 h 05 min focused");
        s.companion_visible = true;
        assert_eq!(model(&s).show_hide, "Hide companion");
    }

    #[test]
    fn first_application_sets_everything_then_only_differences() {
        let a = model(&running("24:59"));
        let all = changes(None, &a);
        assert!(all.contains(&Change::Title(Some("24:59".into()))));
        assert!(all.contains(&Change::Enabled(ID_CUSTOM, false)));
        assert!(all.contains(&Change::Enabled(ID_END, true)));
        assert!(all.contains(&Change::Text(ID_TODAY, "Today: 0 min focused".into())));

        assert!(changes(Some(&a), &a).is_empty(), "unchanged → no calls");

        let b = model(&running("24:58"));
        assert_eq!(
            changes(Some(&a), &b),
            vec![Change::Title(Some("24:58".into()))]
        );

        let idle = model(&snapshot(2, Phase::Idle, None));
        let diff = changes(Some(&a), &idle);
        assert!(diff.contains(&Change::Title(None)));
        for id in [
            ID_START_LAST,
            ID_START_15,
            ID_START_25,
            ID_START_45,
            ID_CUSTOM,
        ] {
            assert!(diff.contains(&Change::Enabled(id, true)), "{id}");
        }
        assert!(diff.contains(&Change::Enabled(ID_PAUSE_RESUME, false)));
        assert!(diff.contains(&Change::Enabled(ID_END, false)));
    }

    #[test]
    fn menu_ids_map_to_actions() {
        assert_eq!(action_for("start-last"), MenuAction::StartFocus(None));
        assert_eq!(action_for("start-15"), MenuAction::StartFocus(Some(15)));
        assert_eq!(action_for("start-25"), MenuAction::StartFocus(Some(25)));
        assert_eq!(action_for("start-45"), MenuAction::StartFocus(Some(45)));
        assert_eq!(action_for("custom"), MenuAction::Custom);
        assert_eq!(action_for("pause-resume"), MenuAction::PauseResume);
        assert_eq!(action_for("end"), MenuAction::End);
        assert_eq!(action_for("show-hide"), MenuAction::ShowHide);
        assert_eq!(action_for("settings"), MenuAction::Settings);
        assert_eq!(action_for("quit"), MenuAction::Quit);
        assert_eq!(action_for("today"), MenuAction::None);
        assert_eq!(action_for("whatever"), MenuAction::None);
    }
}
