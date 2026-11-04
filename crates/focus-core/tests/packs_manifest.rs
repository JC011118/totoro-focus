//! Pack manifest parsing, validation, resolution, render keys and path safety
//! (SPEC §7). UnlockTable values are built literally or with serde (never with
//! `UnlockTable::parse`, which another module owns).

mod packs {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use focus_core::packs::{
        is_valid_pack_id, mime_for, parse_manifest, render_keys, resolve, safe_relative_path,
        validate_manifest, AnimationSpec, PackError, PackManifest, PaletteSpec, Rendering,
        BASE_STATES, EXTRA_STATES, MAX_MANIFEST_BYTES, PACK_FORMAT,
    };
    use focus_core::rewards::{Rules, UnlockDef, UnlockKind, UnlockTable, BUILTIN_UNLOCKS_JSON};

    // ---------------------------------------------------------------- helpers

    fn anim(sheet: &str, frames: u32) -> AnimationSpec {
        AnimationSpec {
            sheet: sheet.into(),
            frame_width: 96,
            frame_height: 96,
            frames,
            fps: 4.0,
            looping: true,
            row: 0,
            column: 0,
            sheet_columns: None,
            sheet_rows: None,
            still_frame: None,
            frame_durations_ms: None,
        }
    }

    fn manifest(anims: &[&str]) -> PackManifest {
        PackManifest {
            format: PACK_FORMAT,
            id: "test".into(),
            name: "Test".into(),
            author: None,
            license: Some("CC0-1.0".into()),
            description: None,
            rendering: Rendering::Smooth,
            animations: anims
                .iter()
                .map(|k| (k.to_string(), anim(&format!("{k}.svg"), 4)))
                .collect(),
            fallbacks: BTreeMap::new(),
            palettes: BTreeMap::new(),
        }
    }

    fn with_fallbacks(mut m: PackManifest, fb: &[(&str, &str)]) -> PackManifest {
        m.fallbacks = fb
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        m
    }

    fn all_exist(_: &str) -> bool {
        true
    }

    fn validate(m: &PackManifest) -> (Vec<String>, Vec<String>) {
        validate_manifest(m, &all_exist)
    }

    fn has(list: &[String], needle: &str) -> bool {
        list.iter().any(|s| s.contains(needle))
    }

    fn def(id: &str, points: u64, kind: UnlockKind) -> UnlockDef {
        UnlockDef {
            id: id.into(),
            points,
            kind,
            trigger: "manual".into(),
            label: id.into(),
            description: String::new(),
            palette: None,
        }
    }

    fn small_table() -> UnlockTable {
        UnlockTable {
            format: 1,
            rules: Rules {
                points_per_active_minute: 1,
                completion_bonus: 5,
                long_session_minutes: 45,
            },
            unlocks: vec![
                def("base", 0, UnlockKind::States),
                def("wave", 25, UnlockKind::Animation),
                def("umbrella", 100, UnlockKind::IdleVariant),
                def("evening", 160, UnlockKind::Palette),
                def("celebrate", 350, UnlockKind::Animation),
            ],
        }
    }

    /// The shipped table, deserialized directly (no `UnlockTable::parse`).
    fn shipped_table() -> UnlockTable {
        serde_json::from_str(BUILTIN_UNLOCKS_JSON).expect("data/unlocks.json deserializes")
    }

    fn src(m: &PackManifest, key: &str) -> Option<String> {
        resolve(m, key).map(|(k, _)| k)
    }

    // ---------------------------------------------------------------- parse

    #[test]
    fn parse_minimal_manifest_applies_defaults() {
        let m = parse_manifest(
            r#"{"format":1,"id":"mini","name":"Mini",
                "animations":{"idle":{"sheet":"idle.png","frameWidth":32,"frameHeight":32,"frames":2,"fps":2}}}"#,
        )
        .unwrap();
        assert_eq!(m.id, "mini");
        assert_eq!(m.rendering, Rendering::Smooth);
        assert!(m.fallbacks.is_empty() && m.palettes.is_empty());
        assert_eq!(
            (m.author.clone(), m.license.clone(), m.description.clone()),
            (None, None, None)
        );
        let idle = &m.animations["idle"];
        assert!(idle.looping, "loop defaults to true");
        assert_eq!((idle.row, idle.column), (0, 0));
        assert_eq!(
            (idle.sheet_columns, idle.sheet_rows, idle.still_frame),
            (None, None, None)
        );
        assert_eq!(idle.frame_durations_ms, None);
    }

    #[test]
    fn parse_spec_example_round_trips() {
        let json = r##"{
          "format": 1, "id": "placeholder", "name": "Pip (placeholder)",
          "author": "someone", "license": "CC0-1.0", "description": "d",
          "rendering": "pixelated",
          "animations": {
            "idle": { "sheet": "idle.svg", "frameWidth": 96, "frameHeight": 96,
                      "frames": 4, "fps": 4, "loop": true, "row": 0, "column": 0,
                      "sheetColumns": 4, "sheetRows": 1, "stillFrame": 0, "frameDurationsMs": null },
            "completed": { "sheet": "c.svg", "frameWidth": 96, "frameHeight": 96, "frames": 2,
                           "fps": 6, "loop": false, "frameDurationsMs": [100, 400] }
          },
          "fallbacks": { "paused": "idle" },
          "palettes": { "evening": { "background": "#123456", "characterFilter": "brightness(0.9) saturate(0.9)" } }
        }"##;
        let m = parse_manifest(json).unwrap();
        assert_eq!(m.rendering, Rendering::Pixelated);
        assert!(!m.animations["completed"].looping);
        assert_eq!(
            m.animations["completed"].frame_durations_ms,
            Some(vec![100, 400])
        );
        assert_eq!(m.animations["idle"].sheet_columns, Some(4));
        assert_eq!(m.fallbacks["paused"], "idle");
        assert_eq!(m.palettes["evening"].background.as_deref(), Some("#123456"));
        let again = parse_manifest(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(again, m);
    }

    #[test]
    fn parse_errors_are_typed() {
        assert!(matches!(
            parse_manifest("{not json"),
            Err(PackError::Json(_))
        ));
        assert!(matches!(parse_manifest(""), Err(PackError::Json(_))));
        assert!(matches!(
            parse_manifest("[1,2]"),
            Err(PackError::Invalid(_))
        ));
        assert!(matches!(
            parse_manifest("\"pack\""),
            Err(PackError::Invalid(_))
        ));
        // Missing required fields and wrong types are JSON shape errors.
        assert!(matches!(
            parse_manifest(r#"{"id":"a","name":"A"}"#),
            Err(PackError::Json(_))
        ));
        assert!(matches!(
            parse_manifest(r#"{"format":1,"name":"A"}"#),
            Err(PackError::Json(_))
        ));
        let neg = r#"{"format":1,"id":"a","name":"A","animations":{"idle":{"sheet":"i.png","frameWidth":-1,"frameHeight":1,"frames":1,"fps":1}}}"#;
        assert!(matches!(parse_manifest(neg), Err(PackError::Json(_))));
        let fps = r#"{"format":1,"id":"a","name":"A","animations":{"idle":{"sheet":"i.png","frameWidth":1,"frameHeight":1,"frames":1,"fps":"fast"}}}"#;
        assert!(matches!(parse_manifest(fps), Err(PackError::Json(_))));
        let rendering = r#"{"format":1,"id":"a","name":"A","rendering":"blurry"}"#;
        assert!(matches!(parse_manifest(rendering), Err(PackError::Json(_))));
    }

    #[test]
    fn parse_checks_format_before_shape() {
        assert_eq!(
            parse_manifest(r#"{"format":2,"id":"a","name":"A"}"#),
            Err(PackError::UnsupportedFormat(2))
        );
        // A future format with a different shape still reports the format.
        assert_eq!(
            parse_manifest(r#"{"format":7,"packId":"whatever"}"#),
            Err(PackError::UnsupportedFormat(7))
        );
        assert_eq!(
            parse_manifest(r#"{"format":0,"id":"a","name":"A"}"#),
            Err(PackError::UnsupportedFormat(0))
        );
        assert_eq!(
            parse_manifest(r#"{"format":99999999999,"id":"a","name":"A"}"#),
            Err(PackError::UnsupportedFormat(u32::MAX))
        );
    }

    #[test]
    fn parse_rejects_bad_ids() {
        for bad in [
            "",
            "Bad",
            "has space",
            "../up",
            "-lead",
            "_lead",
            "a.b",
            "é",
            &"a".repeat(65),
        ] {
            let json = serde_json::json!({"format": 1, "id": bad, "name": "x"}).to_string();
            assert_eq!(
                parse_manifest(&json),
                Err(PackError::InvalidId(bad.to_string())),
                "{bad:?}"
            );
        }
        assert!(parse_manifest(r#"{"format":1,"id":"ok-id_9","name":"x"}"#).is_ok());
    }

    #[test]
    fn parse_rejects_oversized_manifest() {
        let pad = " ".repeat(MAX_MANIFEST_BYTES as usize);
        let json = format!(r#"{{"format":1,"id":"a","name":"A"}}{pad}"#);
        assert!(matches!(parse_manifest(&json), Err(PackError::Invalid(_))));
    }

    #[test]
    fn parse_ignores_unknown_fields() {
        let m =
            parse_manifest(r#"{"format":1,"id":"a","name":"A","displayScale":2,"homepage":"x"}"#)
                .unwrap();
        assert_eq!(m.id, "a");
    }

    #[test]
    fn pack_error_messages_read_well() {
        assert_eq!(
            PackError::UnsupportedFormat(2).to_string(),
            "unsupported pack format 2"
        );
        assert_eq!(
            PackError::InvalidId("X".into()).to_string(),
            "invalid pack id \"X\""
        );
        assert!(PackError::Json("eof".into())
            .to_string()
            .contains("not valid JSON"));
    }

    #[test]
    fn id_pattern_is_exported() {
        assert!(is_valid_pack_id("placeholder"));
        assert!(!is_valid_pack_id("Placeholder"));
    }

    // ---------------------------------------------------------------- validate

    #[test]
    fn valid_pack_has_no_errors_and_reports_borrowed_states() {
        let (errors, warnings) = validate(&manifest(&["idle"]));
        assert!(errors.is_empty(), "{errors:?}");
        for k in ["focus", "paused", "completed", "break"] {
            assert!(
                has(
                    &warnings,
                    &format!("no \"{k}\" animation; it uses \"idle\"")
                ),
                "{k}: {warnings:?}"
            );
        }
        assert!(!has(&warnings, "\"idle\" animation"));
        // A complete pack has no warnings at all.
        let (errors, warnings) = validate(&manifest(&BASE_STATES));
        assert!(
            errors.is_empty() && warnings.is_empty(),
            "{errors:?} {warnings:?}"
        );
    }

    #[test]
    fn a_pack_needs_some_base_state() {
        let (errors, _) = validate(&manifest(&["wave", "react"]));
        assert!(has(&errors, "no base state animation"), "{errors:?}");
        let (errors, _) = validate(&manifest(&[]));
        assert!(has(&errors, "no base state animation"));
        // Any base state is enough, as is a base state reached through fallbacks.
        let (errors, warnings) = validate(&manifest(&["focus"]));
        assert!(errors.is_empty(), "{errors:?}");
        assert!(has(
            &warnings,
            "no \"idle\" animation; the built-in companion is shown instead"
        ));
        let (errors, _) = validate(&with_fallbacks(manifest(&["wave"]), &[("idle", "wave")]));
        assert!(errors.is_empty(), "{errors:?}");
    }

    type Mutate = Box<dyn Fn(&mut AnimationSpec)>;

    #[test]
    fn frame_geometry_errors() {
        let cases: Vec<(&str, Mutate)> = vec![
            ("frame size 0×96", Box::new(|a| a.frame_width = 0)),
            ("frame size 96×0", Box::new(|a| a.frame_height = 0)),
            ("exceeds 4096×4096", Box::new(|a| a.frame_width = 4097)),
            ("frames must be at least 1", Box::new(|a| a.frames = 0)),
            ("frames 257 exceeds 256", Box::new(|a| a.frames = 257)),
            ("fps 0", Box::new(|a| a.fps = 0.0)),
            ("fps -1", Box::new(|a| a.fps = -1.0)),
            ("fps 60.5", Box::new(|a| a.fps = 60.5)),
            ("fps NaN", Box::new(|a| a.fps = f32::NAN)),
            ("fps inf", Box::new(|a| a.fps = f32::INFINITY)),
            (
                "frames run past the sheet",
                Box::new(|a| a.sheet_columns = Some(3)),
            ),
            (
                "frames run past the sheet",
                Box::new(|a| {
                    a.column = 2;
                    a.sheet_columns = Some(5);
                }),
            ),
            (
                "row 1 is outside the sheet",
                Box::new(|a| {
                    a.row = 1;
                    a.sheet_rows = Some(1);
                }),
            ),
            ("sheet grid", Box::new(|a| a.sheet_rows = Some(300))),
            (
                "implied sheet size",
                Box::new(|a| {
                    a.frame_width = 4096;
                    a.frames = 5;
                }),
            ),
            (
                "stillFrame 4 is out of range",
                Box::new(|a| a.still_frame = Some(4)),
            ),
            (
                "frameDurationsMs has 3 entries",
                Box::new(|a| a.frame_durations_ms = Some(vec![1, 2, 3])),
            ),
            (
                "between 1 and 60000",
                Box::new(|a| a.frame_durations_ms = Some(vec![100, 0, 100, 100])),
            ),
            (
                "between 1 and 60000",
                Box::new(|a| a.frame_durations_ms = Some(vec![100, 60_001, 100, 100])),
            ),
        ];
        for (needle, mutate) in cases {
            let mut m = manifest(&["idle"]);
            mutate(m.animations.get_mut("idle").unwrap());
            let (errors, _) = validate(&m);
            assert!(has(&errors, needle), "{needle}: {errors:?}");
            assert!(
                errors.iter().all(|e| e.starts_with("animations.idle: ")),
                "{errors:?}"
            );
        }
    }

    #[test]
    fn frame_geometry_edges_are_accepted() {
        let mut m = manifest(&["idle"]);
        {
            let a = m.animations.get_mut("idle").unwrap();
            a.fps = 60.0;
            a.frames = 2;
            a.column = 6;
            a.row = 8;
            a.sheet_columns = Some(8);
            a.sheet_rows = Some(9);
            a.still_frame = Some(1);
            a.frame_durations_ms = Some(vec![1, 60_000]);
        }
        let (errors, _) = validate(&m);
        assert!(errors.is_empty(), "{errors:?}");
        m.animations.get_mut("idle").unwrap().fps = 0.001;
        assert!(validate(&m).0.is_empty());
    }

    #[test]
    fn sheet_path_errors() {
        for sheet in [
            "../idle.png",
            "/abs/idle.png",
            "a\\b.png",
            ".hidden.png",
            "C:/x.png",
            "sub/./x.png",
            "",
            "a//b.png",
        ] {
            let mut m = manifest(&["idle"]);
            m.animations.get_mut("idle").unwrap().sheet = sheet.into();
            let (errors, _) = validate(&m);
            assert!(has(&errors, "unsafe sheet path"), "{sheet:?}: {errors:?}");
        }
        for sheet in ["idle.bmp", "idle.json", "idle", "idle.svgz", "idle.html"] {
            let mut m = manifest(&["idle"]);
            m.animations.get_mut("idle").unwrap().sheet = sheet.into();
            let (errors, _) = validate(&m);
            assert!(has(&errors, "must be a .png"), "{sheet:?}: {errors:?}");
        }
        for sheet in [
            "idle.png",
            "idle.PNG",
            "sheets/idle.webp",
            "a.gif",
            "b.jpg",
            "c.jpeg",
            "d.svg",
        ] {
            let mut m = manifest(&["idle"]);
            m.animations.get_mut("idle").unwrap().sheet = sheet.into();
            assert!(validate(&m).0.is_empty(), "{sheet:?}");
        }
    }

    #[test]
    fn missing_sheets_are_errors_and_file_exists_gets_normalized_paths() {
        let mut m = manifest(&["idle", "focus"]);
        m.animations.get_mut("focus").unwrap().sheet = "sheets/focus.png".into();
        let seen = RefCell::new(Vec::new());
        let exists = |p: &str| {
            seen.borrow_mut().push(p.to_string());
            p != "sheets/focus.png"
        };
        let (errors, _) = validate_manifest(&m, &exists);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("animations.focus: sheet file \"sheets/focus.png\" is missing")
        );
        let mut seen = seen.into_inner();
        seen.sort();
        assert_eq!(
            seen,
            vec!["idle.svg".to_string(), "sheets/focus.png".to_string()]
        );
    }

    #[test]
    fn unsafe_sheets_never_reach_file_exists() {
        let mut m = manifest(&["idle"]);
        m.animations.get_mut("idle").unwrap().sheet = "../../etc/passwd.png".into();
        let called = RefCell::new(false);
        let (errors, _) = validate_manifest(&m, &|_| {
            *called.borrow_mut() = true;
            true
        });
        assert!(!errors.is_empty());
        assert!(!*called.borrow());
    }

    #[test]
    fn format_id_and_key_errors() {
        let mut m = manifest(&["idle"]);
        m.format = 3;
        m.id = "Bad Id".into();
        m.animations.insert("Wave Big".into(), anim("w.svg", 2));
        let (errors, _) = validate(&m);
        assert!(has(&errors, "unsupported pack format 3"), "{errors:?}");
        assert!(has(&errors, "invalid pack id \"Bad Id\""), "{errors:?}");
        assert!(
            has(&errors, "animation key \"Wave Big\" is not valid"),
            "{errors:?}"
        );
    }

    #[test]
    fn empty_name_is_a_warning() {
        let mut m = manifest(&BASE_STATES);
        m.name = "  ".into();
        let (errors, warnings) = validate(&m);
        assert!(errors.is_empty());
        assert!(has(&warnings, "name is empty"));
    }

    #[test]
    fn fallback_warnings() {
        let m = with_fallbacks(
            manifest(&["idle", "focus"]),
            &[
                ("paused", "nowhere"),
                ("focus", "idle"),
                ("Bad Key", "idle"),
                ("hum", "celebrate"),
            ],
        );
        let (errors, warnings) = validate(&m);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(
            has(&warnings, "fallback target \"nowhere\" (for \"paused\")"),
            "{warnings:?}"
        );
        assert!(
            has(&warnings, "fallback for \"focus\" is unused"),
            "{warnings:?}"
        );
        assert!(has(&warnings, "uses an invalid key"), "{warnings:?}");
        assert!(
            has(&warnings, "fallback target \"celebrate\" (for \"hum\")"),
            "{warnings:?}"
        );
        // Targets that chain on (another fallback, or the built-in chain) are known.
        let m = with_fallbacks(
            manifest(&["idle"]),
            &[("wave", "completed"), ("hum", "wave")],
        );
        let (_, warnings) = validate(&m);
        assert!(!has(&warnings, "fallback target"), "{warnings:?}");
    }

    #[test]
    fn fallback_cycles_warn_once() {
        let m = with_fallbacks(
            manifest(&["idle"]),
            &[("a", "b"), ("b", "c"), ("c", "a"), ("wave", "wave")],
        );
        let (errors, warnings) = validate(&m);
        assert!(errors.is_empty());
        let cycles: Vec<&String> = warnings
            .iter()
            .filter(|w| w.contains("fallback cycle"))
            .collect();
        assert_eq!(cycles.len(), 2, "{warnings:?}");
        assert!(has(&warnings, "fallback cycle among a → b → c"));
        assert!(has(&warnings, "fallback cycle among wave"));
    }

    #[test]
    fn palette_warnings_are_prefixed_and_bad_ids_reported() {
        let mut m = manifest(&BASE_STATES);
        m.palettes.insert(
            "evening".into(),
            PaletteSpec {
                background: Some("url(x)".into()),
                accent: Some("#fff".into()),
                ..Default::default()
            },
        );
        m.palettes.insert("Bad Id".into(), PaletteSpec::default());
        let (errors, warnings) = validate(&m);
        assert!(
            errors.is_empty(),
            "palette problems are never errors: {errors:?}"
        );
        assert!(
            has(&warnings, "palettes.evening.background: dropped"),
            "{warnings:?}"
        );
        assert!(
            has(&warnings, "palette id \"Bad Id\" is not a valid id"),
            "{warnings:?}"
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
    }

    // ---------------------------------------------------------------- resolve

    #[test]
    fn own_animation_wins() {
        let m = with_fallbacks(manifest(&["idle", "paused"]), &[("paused", "idle")]);
        let (k, spec) = resolve(&m, "paused").unwrap();
        assert_eq!(k, "paused");
        assert_eq!(spec.sheet, "paused.svg");
    }

    #[test]
    fn builtin_chain_for_base_states() {
        let m = manifest(&["idle"]);
        for k in ["focus", "paused", "break", "completed", "idle"] {
            assert_eq!(src(&m, k).as_deref(), Some("idle"), "{k}");
        }
        for k in [
            "react",
            "blink",
            "wave",
            "celebrate",
            "umbrella",
            "sleepy",
            "unknown",
        ] {
            assert_eq!(src(&m, k), None, "{k} must not fall back to idle");
        }
        assert_eq!(src(&manifest(&[]), "focus"), None);
        assert_eq!(
            src(&manifest(&["focus"]), "idle"),
            None,
            "idle has no built-in fallback"
        );
    }

    #[test]
    fn pack_fallbacks_come_before_the_builtin_chain() {
        let m = with_fallbacks(
            manifest(&["idle", "focus", "completed"]),
            &[
                ("paused", "focus"),
                ("react", "idle"),
                ("celebrate", "completed"),
                ("long_session", "celebrate"),
            ],
        );
        assert_eq!(src(&m, "paused").as_deref(), Some("focus"));
        assert_eq!(src(&m, "react").as_deref(), Some("idle"));
        assert_eq!(src(&m, "celebrate").as_deref(), Some("completed"));
        assert_eq!(
            src(&m, "long_session").as_deref(),
            Some("completed"),
            "chains follow several hops"
        );
        assert_eq!(src(&m, "blink"), None);
    }

    #[test]
    fn chains_continue_through_the_builtin_chain() {
        // celebrate → completed (no art) → built-in → idle.
        let m = with_fallbacks(manifest(&["idle"]), &[("celebrate", "completed")]);
        assert_eq!(src(&m, "celebrate").as_deref(), Some("idle"));
        // A dead-end pack fallback for a base state still reaches idle.
        let m = with_fallbacks(manifest(&["idle"]), &[("paused", "nowhere")]);
        assert_eq!(src(&m, "paused").as_deref(), Some("idle"));
    }

    #[test]
    fn cycles_are_cut() {
        let m = with_fallbacks(
            manifest(&["idle"]),
            &[("paused", "focus"), ("focus", "paused")],
        );
        assert_eq!(src(&m, "paused").as_deref(), Some("idle"));
        assert_eq!(src(&m, "focus").as_deref(), Some("idle"));
        let m = with_fallbacks(
            manifest(&["idle"]),
            &[("wave", "hum"), ("hum", "wave"), ("blink", "blink")],
        );
        assert_eq!(src(&m, "wave"), None);
        assert_eq!(src(&m, "hum"), None);
        assert_eq!(src(&m, "blink"), None);
        // idle → focus → (built-in) idle: no animation anywhere, terminates.
        let m = with_fallbacks(manifest(&[]), &[("idle", "focus")]);
        assert_eq!(src(&m, "idle"), None);
        assert_eq!(src(&m, "focus"), None);
    }

    #[test]
    fn long_chains_terminate() {
        let mut m = manifest(&["idle"]);
        for i in 0..5000 {
            m.fallbacks.insert(format!("k{i}"), format!("k{}", i + 1));
        }
        m.fallbacks.insert("k5000".into(), "k0".into());
        assert_eq!(src(&m, "k0"), None);
        m.fallbacks.insert("k5000".into(), "paused".into());
        assert_eq!(src(&m, "k0").as_deref(), Some("idle"));
    }

    // ---------------------------------------------------------------- render keys

    #[test]
    fn render_keys_order_and_dedupe() {
        let m = manifest(&["idle", "wave", "zzz_extra", "aaa_extra"]);
        let keys = render_keys(&small_table(), &m);
        let expected: Vec<String> = BASE_STATES
            .iter()
            .chain(EXTRA_STATES.iter())
            .chain(["wave", "umbrella", "celebrate", "aaa_extra", "zzz_extra"].iter())
            .map(|s| s.to_string())
            .collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn render_keys_skip_states_and_palette_unlocks() {
        let keys = render_keys(&shipped_table(), &manifest(&["idle"]));
        for k in [
            "wave",
            "stretch",
            "umbrella",
            "sleepy",
            "celebrate",
            "acorn_gift",
            "long_session",
            "firefly",
            "hum",
            "lantern",
            "stargaze",
        ] {
            assert!(keys.iter().any(|x| x == k), "{k} missing from {keys:?}");
        }
        for k in [
            "base",
            "evening",
            "autumn",
            "dawn",
            "snow",
            "moss",
            "bloom",
            "old_friends",
        ] {
            assert!(!keys.iter().any(|x| x == k), "{k} must not be a render key");
        }
        let unique: std::collections::BTreeSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
        assert_eq!(
            &keys[..7],
            &[
                "idle",
                "focus",
                "paused",
                "completed",
                "break",
                "react",
                "blink"
            ]
        );
    }

    // ---------------------------------------------------------------- paths

    #[test]
    fn safe_relative_paths_accepted() {
        for (p, comps) in [
            ("idle.png", vec!["idle.png"]),
            ("sheets/idle.png", vec!["sheets", "idle.png"]),
            ("a b/c d.svg", vec!["a b", "c d.svg"]),
            ("ünï/çødé.png", vec!["ünï", "çødé.png"]),
            ("~home.png", vec!["~home.png"]),
            ("x..y.png", vec!["x..y.png"]),
        ] {
            let got = safe_relative_path(p).unwrap_or_else(|| panic!("{p:?} rejected"));
            assert_eq!(got, comps.iter().collect::<PathBuf>(), "{p:?}");
        }
    }

    #[test]
    fn unsafe_relative_paths_rejected() {
        let long = "a/".repeat(300) + "x.png";
        let deep = "d/".repeat(16) + "x.png";
        for p in [
            "",
            "/etc/passwd",
            "//server/share.png",
            "..",
            "../x.png",
            "a/../b.png",
            "a/..",
            ".",
            "./a.png",
            "a/./b.png",
            ".hidden.png",
            "a/.git/config",
            ".DS_Store",
            "a\\b.png",
            "..\\x.png",
            "C:/x.png",
            "c:x.png",
            "a:b.png",
            "a\0b.png",
            "a\nb.png",
            "a\u{7f}.png",
            "a//b.png",
            "a/",
            "/",
            &long,
            &deep,
        ] {
            assert_eq!(safe_relative_path(p), None, "{p:?}");
        }
        // Exactly at the depth limit is fine.
        assert!(safe_relative_path(&("d/".repeat(15) + "x.png")).is_some());
    }

    #[test]
    fn mime_types() {
        for (p, m) in [
            ("a.png", "image/png"),
            ("a.PNG", "image/png"),
            ("dir/a.webp", "image/webp"),
            ("a.svg", "image/svg+xml"),
            ("pack.json", "application/json"),
            ("a.gif", "image/gif"),
            ("a.jpg", "image/jpeg"),
            ("a.JPEG", "image/jpeg"),
        ] {
            assert_eq!(mime_for(Path::new(p)), Some(m), "{p}");
        }
        for p in [
            "a.txt",
            "a.html",
            "a.js",
            "a.css",
            "LICENSE",
            "png",
            "a.png.exe",
            "a.svgz",
            "a.",
        ] {
            assert_eq!(mime_for(Path::new(p)), None, "{p}");
        }
    }
}
