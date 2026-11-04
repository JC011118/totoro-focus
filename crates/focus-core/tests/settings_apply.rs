//! Settings validation, atomic patches and sanitizing (SPEC §4.10).

use focus_core::settings::{
    is_valid_id, Settings, SettingsPatch, UserSettingsPatch, WidgetPosition, DEFAULT_HOTKEY,
    DEFAULT_PACK,
};

fn patch() -> SettingsPatch {
    SettingsPatch::default()
}

#[test]
fn defaults() {
    let s = Settings::default();
    assert_eq!(s.last_focus_minutes, 25);
    assert!(s.sound);
    assert!(!s.reduced_motion);
    assert!(!s.hide_during_focus);
    assert!(s.menu_bar_title);
    assert_eq!(s.hotkey.as_deref(), Some(DEFAULT_HOTKEY));
    assert_eq!(s.active_pack, DEFAULT_PACK);
    assert_eq!(s.palette, "auto");
    assert_eq!(s.widget_position, None);
}

#[test]
fn apply_sets_every_field() {
    let mut s = Settings::default();
    let p = SettingsPatch {
        last_focus_minutes: Some(45),
        sound: Some(false),
        reduced_motion: Some(true),
        hide_during_focus: Some(true),
        menu_bar_title: Some(false),
        hotkey: Some(Some("Ctrl+Alt+Shift+K".into())),
        active_pack: Some("my-pack_2".into()),
        palette: Some("evening".into()),
        widget_position: Some(Some(WidgetPosition { x: 10.5, y: -20.0 })),
    };
    s.apply(&p, 1, 180).unwrap();
    assert_eq!(s.last_focus_minutes, 45);
    assert!(!s.sound && s.reduced_motion && s.hide_during_focus && !s.menu_bar_title);
    assert_eq!(s.hotkey.as_deref(), Some("Ctrl+Alt+Shift+K"));
    assert_eq!(s.active_pack, "my-pack_2");
    assert_eq!(s.palette, "evening");
    assert_eq!(
        s.widget_position,
        Some(WidgetPosition { x: 10.5, y: -20.0 })
    );
}

#[test]
fn apply_none_means_unchanged_and_some_none_clears() {
    let mut s = Settings {
        widget_position: Some(WidgetPosition { x: 1.0, y: 2.0 }),
        ..Settings::default()
    };
    let before = s.clone();
    s.apply(&patch(), 1, 180).unwrap();
    assert_eq!(s, before);
    s.apply(
        &SettingsPatch {
            hotkey: Some(None),
            widget_position: Some(None),
            ..patch()
        },
        1,
        180,
    )
    .unwrap();
    assert_eq!(s.hotkey, None);
    assert_eq!(s.widget_position, None);
}

#[test]
fn apply_is_all_or_nothing() {
    let bad = [
        SettingsPatch {
            sound: Some(false),
            last_focus_minutes: Some(0),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            last_focus_minutes: Some(181),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            active_pack: Some("Bad Pack".into()),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            active_pack: Some(String::new()),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            active_pack: Some("-lead".into()),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            active_pack: Some("a".repeat(65)),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            palette: Some("Evening".into()),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            palette: Some("../x".into()),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            hotkey: Some(Some("   ".into())),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            hotkey: Some(Some("K".repeat(65))),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            widget_position: Some(Some(WidgetPosition {
                x: f64::NAN,
                y: 0.0,
            })),
            ..patch()
        },
        SettingsPatch {
            sound: Some(false),
            widget_position: Some(Some(WidgetPosition {
                x: 0.0,
                y: f64::INFINITY,
            })),
            ..patch()
        },
    ];
    for p in bad {
        let mut s = Settings::default();
        let err = s.apply(&p, 1, 180);
        assert!(err.is_err(), "{p:?} should be rejected");
        assert!(!err.unwrap_err().is_empty());
        assert_eq!(s, Settings::default(), "nothing applied for {p:?}");
    }
}

#[test]
fn apply_boundaries() {
    let mut s = Settings::default();
    for m in [1, 180] {
        s.apply(
            &SettingsPatch {
                last_focus_minutes: Some(m),
                ..patch()
            },
            1,
            180,
        )
        .unwrap();
        assert_eq!(s.last_focus_minutes, m);
    }
    s.apply(
        &SettingsPatch {
            hotkey: Some(Some("K".repeat(64))),
            active_pack: Some("a".repeat(64)),
            palette: Some("auto".into()),
            ..patch()
        },
        1,
        180,
    )
    .unwrap();
}

#[test]
fn id_rule() {
    for ok in ["placeholder", "a", "0", "my-pack", "my_pack", "a1-b2_c3"] {
        assert!(is_valid_id(ok), "{ok}");
    }
    for bad in ["", "A", "_a", "-a", "a b", "a/b", "a.b", "é", "a\n"] {
        assert!(!is_valid_id(bad), "{bad:?}");
    }
}

#[test]
fn sanitized_repairs_each_field_to_its_default() {
    let broken = Settings {
        last_focus_minutes: 0,
        sound: false,
        reduced_motion: true,
        hide_during_focus: true,
        menu_bar_title: false,
        hotkey: Some(String::new()),
        active_pack: String::new(),
        palette: "Not Valid".into(),
        widget_position: Some(WidgetPosition {
            x: f64::NAN,
            y: 1.0,
        }),
    };
    let s = broken.sanitized(1, 180);
    assert_eq!(s.last_focus_minutes, 25);
    // Valid fields are kept.
    assert!(!s.sound && s.reduced_motion && s.hide_during_focus && !s.menu_bar_title);
    assert_eq!(s.hotkey.as_deref(), Some(DEFAULT_HOTKEY));
    assert_eq!(s.active_pack, DEFAULT_PACK);
    assert_eq!(s.palette, "auto");
    assert_eq!(s.widget_position, None);

    let too_long = Settings {
        last_focus_minutes: 181,
        hotkey: Some("x".repeat(65)),
        ..Settings::default()
    }
    .sanitized(1, 180);
    assert_eq!(too_long.last_focus_minutes, 25);
    assert_eq!(too_long.hotkey.as_deref(), Some(DEFAULT_HOTKEY));

    // A disabled hotkey stays disabled; a valid custom one is kept.
    let disabled = Settings {
        hotkey: None,
        ..Settings::default()
    };
    assert_eq!(disabled.clone().sanitized(1, 180), disabled);
    let custom = Settings {
        hotkey: Some("Super+F5".into()),
        last_focus_minutes: 180,
        palette: "moss".into(),
        widget_position: Some(WidgetPosition { x: -100.0, y: 5.0 }),
        ..Settings::default()
    };
    assert_eq!(custom.clone().sanitized(1, 180), custom);
    // The default itself is clamped into an unusual range.
    assert_eq!(Settings::default().sanitized(30, 60).last_focus_minutes, 30);
}

#[test]
fn user_patch_converts_without_privileged_fields() {
    let u = UserSettingsPatch {
        last_focus_minutes: Some(15),
        sound: Some(false),
        reduced_motion: Some(true),
        hide_during_focus: Some(true),
        menu_bar_title: Some(false),
        palette: Some("forest".into()),
    };
    let p: SettingsPatch = u.into();
    assert_eq!(p.last_focus_minutes, Some(15));
    assert_eq!(p.palette.as_deref(), Some("forest"));
    assert_eq!(p.hotkey, None);
    assert_eq!(p.active_pack, None);
    assert_eq!(p.widget_position, None);
}

#[test]
fn settings_json_defaults_missing_fields() {
    let s: Settings = serde_json::from_str(r#"{"sound": false}"#).unwrap();
    assert!(!s.sound);
    assert_eq!(s.last_focus_minutes, 25);
    let v = serde_json::to_value(Settings::default()).unwrap();
    assert_eq!(v["lastFocusMinutes"], 25);
    assert_eq!(v["hotkey"], DEFAULT_HOTKEY);
    let p: SettingsPatch =
        serde_json::from_str(r#"{"hotkey": null, "widgetPosition": null}"#).unwrap();
    assert_eq!(p.hotkey, Some(None));
    assert_eq!(p.widget_position, Some(None));
    let p: SettingsPatch = serde_json::from_str("{}").unwrap();
    assert_eq!(p, SettingsPatch::default());
}
