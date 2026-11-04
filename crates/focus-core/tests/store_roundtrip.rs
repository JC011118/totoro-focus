//! Persistence round trips, JSON shape, schema handling and field tolerance
//! (SPEC §9). Run with `cargo test -p focus-core --test store_roundtrip`.

mod store_support;

use std::fs;
use std::sync::Arc;

use focus_core::history::{EndReason, Outcome};
use focus_core::settings::{Settings, WidgetPosition, DEFAULT_HOTKEY};
use focus_core::store::{
    migrate, LoadNote, PersistedState, Store, StoreError, PREV_FILE, SCHEMA_VERSION, STATE_FILE,
    TMP_FILE,
};
use focus_core::timer::{PauseReason, RunStatus};
use serde_json::json;
use store_support::*;

fn load_ok(store: &Store) -> PersistedState {
    let report = store.load(t0());
    assert_eq!(report.note, LoadNote::Loaded);
    report.state
}

#[test]
fn store_roundtrip_fully_populated_state() {
    let dir = tempfile::tempdir().unwrap();
    let state = full_state();
    Store::new(dir.path()).save(&state).unwrap();

    let loaded = load_ok(&Store::new(dir.path()));
    assert_eq!(loaded, state);
    // Spot-check the parts most likely to drift.
    let cp = loaded.active_session.as_ref().unwrap();
    assert_eq!(
        cp.task_label.as_deref(),
        Some("Draft \"the\" report — ünïcödé 🌰")
    );
    assert_eq!(cp.started_at, at(100, 123));
    assert_eq!(cp.pause_reason, Some(PauseReason::ClockJump));
    assert_eq!(loaded.history[1].ended_utc_offset_minutes, Some(330));
    assert_eq!(loaded.friendship.awards["s2"].awarded_at, at(52, 999));
    assert_eq!(
        loaded.settings.widget_position,
        Some(WidgetPosition {
            x: -1234.5,
            y: 87.25
        })
    );
}

#[test]
fn store_roundtrip_default_state() {
    let dir = tempfile::tempdir().unwrap();
    Store::new(dir.path())
        .save(&PersistedState::default())
        .unwrap();
    assert_eq!(load_ok(&Store::new(dir.path())), PersistedState::default());
}

#[test]
fn store_roundtrip_every_variant() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let reasons = [
        PauseReason::User,
        PauseReason::Suspend,
        PauseReason::ClockJump,
        PauseReason::Quit,
        PauseReason::Restored,
    ];
    for reason in reasons {
        let mut state = full_state();
        state.active_session.as_mut().unwrap().pause_reason = Some(reason);
        store.save(&state).unwrap();
        assert_eq!(load_ok(&store), state, "pause reason {reason:?}");
    }

    // Running break checkpoint without optional fields; no history; extreme numbers.
    let mut state = other_state(u64::MAX);
    state.history = vec![record("x", Outcome::Interrupted, EndReason::UserEnded)];
    state.history[0].planned_ms = u64::MAX;
    state.settings.hotkey = None;
    state.settings.widget_position = None;
    store.save(&state).unwrap();
    let loaded = load_ok(&store);
    assert_eq!(loaded, state);
    assert_eq!(
        loaded.active_session.as_ref().unwrap().status,
        RunStatus::Running
    );
    assert_eq!(
        loaded.settings.hotkey, None,
        "a disabled hotkey stays disabled"
    );

    // No checkpoint at all.
    state.active_session = None;
    store.save(&state).unwrap();
    assert_eq!(load_ok(&store), state);
}

#[test]
fn store_saved_json_is_camel_case_and_schema_versioned() {
    let dir = tempfile::tempdir().unwrap();
    Store::new(dir.path()).save(&full_state()).unwrap();
    let text = fs::read_to_string(dir.path().join(STATE_FILE)).unwrap();
    assert!(text.ends_with('\n'));
    assert!(
        text.contains("\n  \"schemaVersion\": 1"),
        "pretty-printed: {text}"
    );
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schemaVersion"], json!(SCHEMA_VERSION));
    for key in ["settings", "activeSession", "history", "friendship"] {
        assert!(v.get(key).is_some(), "missing top-level {key}");
    }
    let s = &v["settings"];
    for key in [
        "lastFocusMinutes",
        "sound",
        "reducedMotion",
        "hideDuringFocus",
        "menuBarTitle",
        "hotkey",
        "activePack",
        "palette",
        "widgetPosition",
    ] {
        assert!(s.get(key).is_some(), "missing settings.{key}");
    }
    let cp = &v["activeSession"];
    for key in [
        "plannedMs",
        "startedAt",
        "accruedMs",
        "checkpointAt",
        "pausedAt",
        "pauseReason",
        "ageAnchor",
        "taskLabel",
    ] {
        assert!(cp.get(key).is_some(), "missing activeSession.{key}");
    }
    assert_eq!(cp["pauseReason"], json!("clockJump"));
    assert_eq!(v["history"][1]["endReason"], json!("userEnded"));
    assert_eq!(v["history"][2]["endReason"], json!("pauseExpired"));
    assert_eq!(
        v["friendship"]["awards"]["s1"]["awardedAt"],
        json!("2026-03-02T09:25:00Z")
    );
}

#[test]
fn store_save_writes_the_current_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    let mut state = full_state();
    state.schema_version = 0;
    store.save(&state).unwrap();
    let loaded = load_ok(&store);
    assert_eq!(loaded.schema_version, SCHEMA_VERSION);
    assert_eq!(loaded.history, state.history);
}

#[test]
fn store_save_never_writes_a_file_load_rejects() {
    // JSON has no NaN/inf; serde_json would write `null`, which `WidgetPosition` rejects.
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut state = full_state();
        state.settings.widget_position = Some(WidgetPosition { x: bad, y: 10.0 });
        store.save(&state).unwrap();
        let loaded = load_ok(&store);
        assert_eq!(loaded.settings.widget_position, None);
        assert_eq!(loaded.friendship, state.friendship);
    }
}

#[test]
fn store_save_creates_missing_directories() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("a").join("b").join("app-data");
    let store = Store::new(&nested);
    store.save(&full_state()).unwrap();
    assert!(nested.join(STATE_FILE).is_file());
    assert_eq!(load_ok(&store), full_state());
}

#[test]
fn store_save_leaves_no_temp_file_and_no_prev_on_first_ever_save() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::new(dir.path());
    store.save(&full_state()).unwrap();
    store.save(&other_state(7)).unwrap();
    assert_eq!(file_names(dir.path()), [STATE_FILE]);
    assert!(!dir.path().join(TMP_FILE).exists());
    assert!(!dir.path().join(PREV_FILE).exists());
}

#[test]
fn store_save_reports_io_errors() {
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("not-a-dir");
    fs::write(&blocker, b"x").unwrap();
    let store = Store::new(blocker.join("app"));
    assert!(matches!(store.save(&full_state()), Err(StoreError::Io(_))));
    assert_eq!(fs::read(&blocker).unwrap(), b"x");
}

#[test]
fn store_load_missing_returns_defaults_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let app_dir = dir.path().join("never-created");
    let report = Store::new(&app_dir).load(t0());
    assert_eq!(report.note, LoadNote::Missing);
    assert_eq!(report.state, PersistedState::default());
    assert!(!app_dir.exists(), "load must not create the directory");

    let report = Store::new(dir.path()).load(t0());
    assert_eq!(report.note, LoadNote::Missing);
    assert!(file_names(dir.path()).is_empty());
}

#[test]
fn store_unknown_fields_are_tolerated_everywhere() {
    let dir = tempfile::tempdir().unwrap();
    let mut v = serde_json::to_value(full_state()).unwrap();
    v["futureTopLevel"] = json!({"nested": [1, 2, 3]});
    v["settings"]["futureSetting"] = json!("x");
    v["activeSession"]["futureField"] = json!(true);
    v["history"][0]["mood"] = json!("calm");
    v["friendship"]["futureLedger"] = json!([]);
    v["friendship"]["awards"]["s1"]["note"] = json!(null);
    v["settings"]["widgetPosition"]["z"] = json!(3);
    fs::write(dir.path().join(STATE_FILE), serde_json::to_vec(&v).unwrap()).unwrap();

    assert_eq!(load_ok(&Store::new(dir.path())), full_state());
}

#[test]
fn store_settings_missing_fields_use_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({
        "schemaVersion": 1,
        "settings": { "sound": false, "lastFocusMinutes": 45 }
    });
    fs::write(dir.path().join(STATE_FILE), doc.to_string()).unwrap();
    let state = load_ok(&Store::new(dir.path()));
    assert_eq!(
        state.settings,
        Settings {
            sound: false,
            last_focus_minutes: 45,
            ..Settings::default()
        }
    );
    assert_eq!(state.settings.hotkey.as_deref(), Some(DEFAULT_HOTKEY));
    assert_eq!(state.settings.palette, "auto");
    assert_eq!(state.settings.active_pack, "placeholder");
}

#[test]
fn store_settings_null_hotkey_means_disabled_not_default() {
    let dir = tempfile::tempdir().unwrap();
    let doc = json!({ "schemaVersion": 1, "settings": { "hotkey": null } });
    fs::write(dir.path().join(STATE_FILE), doc.to_string()).unwrap();
    assert_eq!(load_ok(&Store::new(dir.path())).settings.hotkey, None);
}

#[test]
fn store_missing_sections_use_defaults() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(STATE_FILE), r#"{"schemaVersion":1}"#).unwrap();
    assert_eq!(load_ok(&Store::new(dir.path())), PersistedState::default());

    fs::write(
        dir.path().join(STATE_FILE),
        r#"{"schemaVersion":1,"settings":{}}"#,
    )
    .unwrap();
    assert_eq!(load_ok(&Store::new(dir.path())), PersistedState::default());
}

#[test]
fn store_missing_schema_version_loads_as_version_1() {
    let dir = tempfile::tempdir().unwrap();
    let mut v = serde_json::to_value(full_state()).unwrap();
    v.as_object_mut().unwrap().remove("schemaVersion");
    fs::write(dir.path().join(STATE_FILE), v.to_string()).unwrap();
    let report = Store::new(dir.path()).load(t0());
    // Version 1 is current, so nothing needed migrating.
    assert_eq!(report.note, LoadNote::Loaded);
    assert_eq!(report.state, full_state());
}

#[test]
fn store_utf8_bom_is_tolerated() {
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = b"\xEF\xBB\xBF".to_vec();
    bytes.extend(to_json(&full_state()));
    fs::write(dir.path().join(STATE_FILE), bytes).unwrap();
    assert_eq!(load_ok(&Store::new(dir.path())), full_state());
}

#[test]
fn store_concurrent_saves_leave_a_valid_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::new(dir.path()));
    let handles: Vec<_> = (0..8u64)
        .map(|t| {
            let store = Arc::clone(&store);
            std::thread::spawn(move || {
                for i in 0..20 {
                    store.save(&other_state(t * 1000 + i)).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let loaded = load_ok(&Store::new(dir.path()));
    let points = loaded.friendship.total_points;
    assert!(points % 1000 < 20 && points / 1000 < 8, "points {points}");
    assert_eq!(loaded, other_state(points));
    assert_eq!(file_names(dir.path()), [STATE_FILE]);
}

// ---- migrate() -------------------------------------------------------------

#[test]
fn store_migrate_missing_or_null_version_is_1() {
    let mut v = serde_json::to_value(full_state()).unwrap();
    v.as_object_mut().unwrap().remove("schemaVersion");
    let (state, from) = migrate(v.clone()).unwrap();
    assert_eq!(from, 1);
    assert_eq!(state, full_state());
    assert_eq!(state.schema_version, SCHEMA_VERSION);

    v["schemaVersion"] = json!(null);
    assert_eq!(migrate(v).unwrap().1, 1);

    let (state, from) = migrate(json!({})).unwrap();
    assert_eq!((state, from), (PersistedState::default(), 1));
}

#[test]
fn store_migrate_current_version() {
    let v = serde_json::to_value(full_state()).unwrap();
    assert_eq!(migrate(v).unwrap(), (full_state(), SCHEMA_VERSION));
}

#[test]
fn store_migrate_newer_version_is_refused() {
    for version in [SCHEMA_VERSION + 1, 99, u32::MAX] {
        let mut v = serde_json::to_value(full_state()).unwrap();
        v["schemaVersion"] = json!(version);
        match migrate(v) {
            Err(StoreError::NewerSchema(n)) => assert_eq!(n, version),
            other => panic!("expected NewerSchema({version}), got {other:?}"),
        }
    }
    // A newer document may have any shape; the version alone decides.
    assert!(matches!(
        migrate(json!({"schemaVersion": 2, "settings": "something new"})),
        Err(StoreError::NewerSchema(2))
    ));
}

#[test]
fn store_migrate_invalid_documents() {
    let bad_versions = [
        json!(0),
        json!(-1),
        json!(1.5),
        json!("1"),
        json!(true),
        json!([1]),
        json!(u64::from(u32::MAX) + 1),
    ];
    for version in bad_versions {
        let doc = json!({ "schemaVersion": version });
        assert!(
            matches!(migrate(doc), Err(StoreError::Invalid(_))),
            "schemaVersion {version} should be invalid"
        );
    }
    for doc in [json!(null), json!([]), json!("state"), json!(42)] {
        assert!(matches!(migrate(doc), Err(StoreError::Invalid(_))));
    }
    let wrong_shapes = [
        json!({"schemaVersion": 1, "history": 5}),
        json!({"schemaVersion": 1, "history": null}),
        json!({"schemaVersion": 1, "settings": {"sound": "yes"}}),
        json!({"schemaVersion": 1, "activeSession": {"id": "s1"}}),
        json!({"schemaVersion": 1, "friendship": {"totalPoints": -3}}),
    ];
    for doc in wrong_shapes {
        assert!(matches!(migrate(doc), Err(StoreError::Invalid(_))));
    }
}

#[test]
fn store_settings_type_matches_file_defaults() {
    // `PersistedState::default()` uses `Settings::default()`; an empty settings object
    // must deserialize to exactly that (container-level serde default).
    let s: Settings = serde_json::from_value(json!({})).unwrap();
    assert_eq!(s, Settings::default());
}
