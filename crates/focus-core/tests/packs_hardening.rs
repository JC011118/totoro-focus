//! Pack loading under hostile input: manifests near the size cap with
//! thousands of fallbacks or animations must validate in bounded time, long
//! problem lists are capped, and named pipes (FIFOs) in pack folders must never
//! block loading or the `pack://` file reader (SPEC §7: a broken pack never
//! breaks the timer; the shell loads packs on the main thread).
//!
//! Work that could hang on a regression runs on a helper thread with a time
//! limit, so a regression fails the test instead of stalling the run.

mod packs {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use focus_core::packs::{
        validate_manifest, AnimationSpec, BundledPack, PackManifest, PackRegistry, Rendering,
        MAX_LISTED_PROBLEMS, MAX_MANIFEST_BYTES, PACK_FORMAT, PLACEHOLDER_ID,
    };

    /// Generous for a debug build on a busy machine. The fixed algorithms take
    /// milliseconds; the quadratic-or-worse cycle check this guards against took
    /// tens of seconds at a few thousand entries and about half an hour at the
    /// size cap, in release.
    const BUDGET: Duration = Duration::from_secs(10);
    const SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'/>";

    // ---------------------------------------------------------------- helpers

    /// Run `f` on a helper thread and return its result, or fail if it takes
    /// longer than `limit` (the thread is left behind; the test still ends).
    fn within<T: Send + 'static>(
        what: &str,
        limit: Duration,
        f: impl FnOnce() -> T + Send + 'static,
    ) -> T {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(f());
        });
        match rx.recv_timeout(limit) {
            Ok(v) => v,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                panic!("{what} did not finish within {limit:?}")
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => panic!("{what} panicked"),
        }
    }

    fn anim(sheet: &str) -> AnimationSpec {
        AnimationSpec {
            sheet: sheet.into(),
            frame_width: 96,
            frame_height: 96,
            frames: 1,
            fps: 1.0,
            looping: true,
            row: 0,
            column: 0,
            sheet_columns: None,
            sheet_rows: None,
            still_frame: None,
            frame_durations_ms: None,
        }
    }

    fn idle_manifest() -> PackManifest {
        PackManifest {
            format: PACK_FORMAT,
            id: "big".into(),
            name: "Big".into(),
            author: None,
            license: Some("CC0-1.0".into()),
            description: None,
            rendering: Rendering::Smooth,
            animations: [("idle".to_string(), anim("idle.svg"))].into(),
            fallbacks: BTreeMap::new(),
            palettes: BTreeMap::new(),
        }
    }

    fn key(i: usize) -> String {
        format!("k{i:05}")
    }

    /// `k00000 → k00001 → … → k{n-1} → last`.
    fn chain(n: usize, last: &str) -> BTreeMap<String, String> {
        (0..n)
            .map(|i| {
                let to = if i + 1 == n {
                    last.to_string()
                } else {
                    key(i + 1)
                };
                (key(i), to)
            })
            .collect()
    }

    fn validate_timed(m: PackManifest) -> (Vec<String>, Vec<String>) {
        within("validate_manifest", BUDGET, move || {
            validate_manifest(&m, &|_: &str| true)
        })
    }

    fn cycle_warnings(warnings: &[String]) -> Vec<&String> {
        warnings
            .iter()
            .filter(|w| w.starts_with("fallback cycle"))
            .collect()
    }

    fn placeholder_bundle() -> BundledPack {
        let json = r#"{"format":1,"id":"placeholder","name":"Pip","license":"CC0-1.0",
            "animations":{"idle":{"sheet":"idle.svg","frameWidth":96,"frameHeight":96,"frames":1,"fps":1}}}"#;
        BundledPack {
            files: vec![
                ("pack.json".into(), json.as_bytes()),
                ("idle.svg".into(), SVG),
            ],
        }
    }

    fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn load_timed(user: &Path) -> PackRegistry {
        let user = user.to_path_buf();
        within("PackRegistry::load", BUDGET, move || {
            PackRegistry::load(placeholder_bundle(), Some(&user), None)
        })
    }

    /// A pack.json (id `big`, one `idle` animation) with as many fallbacks
    /// from `edge(i)` as fit under `MAX_MANIFEST_BYTES`. Returns the JSON and
    /// the number of fallbacks.
    fn near_limit_manifest(edge: impl Fn(usize) -> (String, String)) -> (String, usize) {
        let head = r#"{"format":1,"id":"big","name":"Big","license":"CC0-1.0","animations":{"idle":{"sheet":"idle.svg","frameWidth":96,"frameHeight":96,"frames":1,"fps":1}},"fallbacks":{"#;
        let mut json = head.to_string();
        let mut n = 0;
        loop {
            let (from, to) = edge(n);
            let entry = format!("{}\"{from}\":\"{to}\"", if n == 0 { "" } else { "," });
            if (json.len() + entry.len() + 2) as u64 > MAX_MANIFEST_BYTES {
                break;
            }
            json.push_str(&entry);
            n += 1;
        }
        json.push_str("}}");
        assert!(json.len() as u64 <= MAX_MANIFEST_BYTES);
        serde_json::from_str::<serde_json::Value>(&json).expect("generated JSON parses");
        (json, n)
    }

    // ---------------------------------------------------------------- fallback graphs

    #[test]
    fn a_chain_of_fifteen_thousand_fallbacks_validates_quickly() {
        let mut m = idle_manifest();
        m.fallbacks = chain(15_000, "nowhere");
        let start = Instant::now();
        let (errors, warnings) = validate_timed(m);
        assert!(errors.is_empty(), "{errors:?}");
        assert!(cycle_warnings(&warnings).is_empty(), "{warnings:?}");
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.starts_with("fallback target"))
                .collect::<Vec<_>>(),
            vec!["fallback target \"nowhere\" (for \"k14999\") is not an animation in this pack"],
            "only the chain's end is unknown"
        );
        eprintln!("15,000-entry chain validated in {:?}", start.elapsed());
    }

    #[test]
    fn one_huge_cycle_is_reported_once_and_elided() {
        let mut m = idle_manifest();
        m.fallbacks = chain(15_000, &key(0));
        let (errors, warnings) = validate_timed(m);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            cycle_warnings(&warnings),
            vec![
                "fallback cycle among k00000 → k00001 → k00002 → k00003 → k00004 → k00005 → \
                 k00006 → k00007 → … (15000 keys); the cycle is cut"
            ]
        );
        let longest = warnings.iter().map(String::len).max().unwrap();
        assert!(longest < 200, "warnings stay short: {longest} bytes");
    }

    #[test]
    fn long_tails_into_a_cycle_report_the_cycle_once() {
        // 10,000 keys chained into a 5,000-key cycle, plus 2,000 more tails
        // joining the chain halfway: every walk ends at an already finished key.
        let mut m = idle_manifest();
        for i in 0..10_000 {
            m.fallbacks
                .insert(format!("t{i:05}"), format!("t{:05}", i + 1));
        }
        m.fallbacks.insert("t10000".into(), "c00000".into());
        for i in 0..5_000 {
            m.fallbacks
                .insert(format!("c{i:05}"), format!("c{:05}", (i + 1) % 5_000));
        }
        for i in 0..2_000 {
            m.fallbacks.insert(format!("j{i:05}"), "t05000".into());
        }
        let (errors, warnings) = validate_timed(m);
        assert!(errors.is_empty(), "{errors:?}");
        let cycles = cycle_warnings(&warnings);
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert!(cycles[0].contains("c00000 → c00001"), "{cycles:?}");
        assert!(cycles[0].contains("(5000 keys)"), "{cycles:?}");
    }

    #[test]
    fn thousands_of_small_cycles_are_each_reported() {
        let mut m = idle_manifest();
        for i in 0..4_000 {
            m.fallbacks.insert(format!("a{i:04}"), format!("b{i:04}"));
            m.fallbacks.insert(format!("b{i:04}"), format!("a{i:04}"));
        }
        let (errors, warnings) = validate_timed(m);
        assert!(errors.is_empty(), "{errors:?}");
        let cycles = cycle_warnings(&warnings);
        assert_eq!(cycles.len(), 4_000);
        assert_eq!(
            cycles[0],
            "fallback cycle among a0000 → b0000; the cycle is cut"
        );
    }

    #[test]
    fn cycles_through_base_states_still_resolve_after_the_fix() {
        // The cycle check only warns; resolution still reaches idle (SPEC §7).
        let mut m = idle_manifest();
        m.fallbacks = [
            ("paused".to_string(), "focus".to_string()),
            ("focus".to_string(), "paused".to_string()),
        ]
        .into();
        let (errors, warnings) = validate_timed(m.clone());
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            cycle_warnings(&warnings),
            vec!["fallback cycle among focus → paused; the cycle is cut"]
        );
        let src = |k: &str| focus_core::packs::resolve(&m, k).map(|(s, _)| s);
        assert_eq!(src("paused").as_deref(), Some("idle"));
        assert_eq!(src("focus").as_deref(), Some("idle"));
    }

    // ---------------------------------------------------------------- registry, end to end

    #[test]
    fn registry_loads_a_near_limit_fallback_chain_quickly() {
        let user = tempfile::tempdir().unwrap();
        let (json, n) = near_limit_manifest(|i| (key(i), key(i + 1)));
        assert!(n > 10_000, "the chain should be long: {n}");
        write(user.path().join("big/pack.json"), &json);
        write(user.path().join("big/idle.svg"), SVG);
        let start = Instant::now();
        let r = load_timed(user.path());
        eprintln!(
            "{n}-entry chain ({} bytes) loaded in {:?}",
            json.len(),
            start.elapsed()
        );
        let info = r.info("big").cloned().expect("listed");
        assert!(info.valid, "{info:?}");
        assert!(info.warnings.len() <= MAX_LISTED_PROBLEMS);
        let rp = r.resolve_pack("big", &focus_core_table(), "pack://localhost/");
        assert_eq!(rp.fallback_from, None);
        assert_eq!(
            rp.animations["paused"]
                .as_ref()
                .map(|a| a.source_key.as_str()),
            Some("idle")
        );
    }

    #[test]
    fn registry_loads_a_near_limit_cycle_quickly() {
        let user = tempfile::tempdir().unwrap();
        // Close the chain into one cycle: the last entry points back to k00000.
        let (open, n) = near_limit_manifest(|i| (key(i), key(i + 1)));
        let closed = open.replacen(
            &format!("\"{}\":\"{}\"", key(n - 1), key(n)),
            &format!("\"{}\":\"{}\"", key(n - 1), key(0)),
            1,
        );
        assert_ne!(open, closed);
        write(user.path().join("big/pack.json"), &closed);
        write(user.path().join("big/idle.svg"), SVG);
        let r = load_timed(user.path());
        let info = r.info("big").cloned().expect("listed");
        assert!(info.valid, "{info:?}");
        assert!(
            info.warnings
                .iter()
                .any(|w| w.contains(&format!("… ({n} keys); the cycle is cut"))),
            "{:?}",
            info.warnings
        );
    }

    #[test]
    fn thousands_of_warnings_are_capped_in_the_listing() {
        // Every fallback points to an unknown key: one warning each.
        let user = tempfile::tempdir().unwrap();
        let (json, n) = near_limit_manifest(|i| (key(i), "nowhere".into()));
        assert!(n > MAX_LISTED_PROBLEMS * 10);
        write(user.path().join("big/pack.json"), &json);
        write(user.path().join("big/idle.svg"), SVG);
        let r = load_timed(user.path());
        let info = r.info("big").cloned().expect("listed");
        assert!(info.valid, "warnings never invalidate: {info:?}");
        assert_eq!(info.warnings.len(), MAX_LISTED_PROBLEMS);
        let last = info.warnings.last().unwrap();
        assert!(
            last.starts_with("… and ") && last.ends_with(" more warnings"),
            "{last}"
        );
        // The full count: n unknown targets plus 4 borrowed base states.
        let more: usize = last
            .trim_start_matches("… and ")
            .trim_end_matches(" more warnings")
            .parse()
            .unwrap();
        assert_eq!(more + MAX_LISTED_PROBLEMS - 1, n + 4);
    }

    /// A pack.json with id `id` and 300 animations with `frames: 0` (300
    /// errors, plus "no base state animation").
    fn many_errors_json(id: &str) -> String {
        let anims: serde_json::Map<String, serde_json::Value> = (0..300)
            .map(|i| {
                (
                    format!("a{i:03}"),
                    serde_json::json!({"sheet": "idle.svg", "frameWidth": 96, "frameHeight": 96, "frames": 0, "fps": 1}),
                )
            })
            .collect();
        serde_json::json!({"format": 1, "id": id, "name": "Broken",
            "license": "CC0-1.0", "animations": anims})
        .to_string()
    }

    #[test]
    fn thousands_of_errors_are_capped_and_the_first_is_kept() {
        let user = tempfile::tempdir().unwrap();
        write(
            user.path().join("broken/pack.json"),
            many_errors_json("broken"),
        );
        write(user.path().join("broken/idle.svg"), SVG);
        let r = load_timed(user.path());
        let info = r.info("broken").cloned().expect("listed");
        assert!(!info.valid);
        assert_eq!(info.errors.len(), MAX_LISTED_PROBLEMS);
        assert_eq!(info.errors[0], "animations.a000: frames must be at least 1");
        // 300 frame errors plus "no base state animation".
        assert_eq!(
            info.errors.last().unwrap(),
            &format!("… and {} more errors", 301 - (MAX_LISTED_PROBLEMS - 1))
        );
        assert_eq!(
            r.unavailable_reason("broken").as_deref(),
            Some("animations.a000: frames must be at least 1")
        );
        assert!(!r.is_valid("broken"));
    }

    #[test]
    fn a_demoted_pack_lists_its_demotion_first_under_the_cap() {
        let user = tempfile::tempdir().unwrap();
        // Both folders claim "dup"; the parsed manifests tie on rank, so the
        // first folder name wins and "b-broken" is demoted.
        write(
            user.path().join("a-first/pack.json"),
            serde_json::json!({"format": 1, "id": "dup", "name": "First", "license": "MIT",
                "animations": {"idle": {"sheet": "idle.svg", "frameWidth": 96,
                "frameHeight": 96, "frames": 1, "fps": 1}}})
            .to_string(),
        );
        write(user.path().join("a-first/idle.svg"), SVG);
        write(
            user.path().join("b-broken/pack.json"),
            many_errors_json("dup"),
        );
        write(user.path().join("b-broken/idle.svg"), SVG);
        // A pack claiming the reserved id, with many errors too.
        write(
            user.path().join("c-reserved/pack.json"),
            many_errors_json(PLACEHOLDER_ID),
        );
        write(user.path().join("c-reserved/idle.svg"), SVG);
        let r = load_timed(user.path());
        assert!(r.is_valid("dup"));
        let loser = r.info("user:b-broken").cloned().expect("listed");
        assert!(!loser.valid);
        assert_eq!(loser.errors.len(), MAX_LISTED_PROBLEMS);
        assert_eq!(
            loser.errors[0],
            "duplicate pack id \"dup\": the pack in folder \"a-first\" already uses it"
        );
        assert_eq!(
            loser.errors[1],
            "animations.a000: frames must be at least 1"
        );
        assert_eq!(
            loser.errors.last().unwrap(),
            &format!("… and {} more errors", 302 - (MAX_LISTED_PROBLEMS - 1))
        );
        let reserved = r.info("user:c-reserved").cloned().expect("listed");
        assert_eq!(reserved.errors.len(), MAX_LISTED_PROBLEMS);
        assert!(
            reserved.errors[0].contains("reserved for the bundled pack"),
            "{reserved:?}"
        );
        assert!(r.is_valid(PLACEHOLDER_ID));
    }

    fn focus_core_table() -> focus_core::rewards::UnlockTable {
        serde_json::from_str(focus_core::rewards::BUILTIN_UNLOCKS_JSON)
            .expect("data/unlocks.json deserializes")
    }

    // ---------------------------------------------------------------- FIFOs (unix)

    #[cfg(unix)]
    fn mkfifo(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .expect("mkfifo runs");
        assert!(status.success(), "mkfifo {}", path.display());
        assert!(fs::metadata(path).unwrap().file_type().is_fifo());
    }

    #[cfg(unix)]
    use std::os::unix::fs::FileTypeExt;

    #[cfg(unix)]
    fn fixture_png() -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/petdex/fixture-pet/spritesheet.png"),
        )
        .unwrap()
    }

    #[cfg(unix)]
    fn native_pack(dir: &Path, folder: &str, idle_sheet: &str) -> std::path::PathBuf {
        let root = dir.join(folder);
        let json = serde_json::json!({"format": 1, "id": folder, "name": folder,
            "license": "CC0-1.0", "animations": {"idle": {"sheet": idle_sheet,
            "frameWidth": 96, "frameHeight": 96, "frames": 1, "fps": 1}}});
        write(root.join("pack.json"), json.to_string());
        root
    }

    #[cfg(unix)]
    #[test]
    fn fifos_never_block_loading() {
        let user = tempfile::tempdir().unwrap();
        let u = user.path();

        // A pet whose default spritesheet.webp is a FIFO: the probe skips it
        // and finds spritesheet.png.
        write(
            u.join("pet-webp-fifo/pet.json"),
            r#"{"id":"x","displayName":"Webp FIFO","spriteVersionNumber":1}"#,
        );
        mkfifo(&u.join("pet-webp-fifo/spritesheet.webp"));
        write(u.join("pet-webp-fifo/spritesheet.png"), fixture_png());

        // A pet whose declared spritesheet is a FIFO and nothing else exists.
        write(
            u.join("pet-only-fifo/pet.json"),
            r#"{"id":"y","displayName":"Only FIFO","spritesheetPath":"spritesheet.png"}"#,
        );
        mkfifo(&u.join("pet-only-fifo/spritesheet.png"));
        mkfifo(&u.join("pet-only-fifo/spritesheet.webp"));

        // Manifests that are FIFOs.
        mkfifo(&u.join("fifo-pack-json/pack.json"));
        mkfifo(&u.join("fifo-pet-json/pet.json"));

        // A native pack whose idle sheet is a FIFO, directly or via a symlink.
        let direct = native_pack(u, "sheet-fifo", "idle.svg");
        mkfifo(&direct.join("idle.svg"));
        let linked = native_pack(u, "sheet-link-fifo", "idle.svg");
        mkfifo(&linked.join("pipe.svg"));
        std::os::unix::fs::symlink("pipe.svg", linked.join("idle.svg")).unwrap();

        let r = load_timed(u);

        let pet = r.info("pet-webp-fifo").cloned().expect("listed");
        assert!(pet.valid, "{pet:?}");
        let rp = r.resolve_pack("pet-webp-fifo", &focus_core_table(), "pack://localhost/");
        assert_eq!(
            rp.animations["idle"].as_ref().map(|a| a.url.as_str()),
            Some("pack://localhost/pet-webp-fifo/spritesheet.png")
        );

        let only = r.info("pet-only-fifo").cloned().expect("listed");
        assert!(!only.valid);
        assert!(only.errors[0].contains("spritesheet not found"), "{only:?}");

        for id in ["fifo-pack-json", "fifo-pet-json"] {
            let i = r.info(id).cloned().expect("listed");
            assert!(!i.valid, "{i:?}");
            assert!(i.errors[0].contains("no pack.json"), "{i:?}");
        }
        for id in ["sheet-fifo", "sheet-link-fifo"] {
            let i = r.info(id).cloned().expect("listed");
            assert!(!i.valid, "{i:?}");
            assert!(
                i.errors
                    .iter()
                    .any(|e| e.contains("sheet file \"idle.svg\" is missing")),
                "{i:?}"
            );
        }
        assert!(r.is_valid(PLACEHOLDER_ID));
    }

    #[cfg(unix)]
    #[test]
    fn read_file_refuses_fifos_without_blocking() {
        let user = tempfile::tempdir().unwrap();
        let root = native_pack(user.path(), "np", "idle.svg");
        write(root.join("idle.svg"), SVG);
        mkfifo(&root.join("extra.png"));
        mkfifo(&root.join("sub/deep.json"));
        mkfifo(&root.join("pipe.webp"));
        std::os::unix::fs::symlink("pipe.webp", root.join("link.webp")).unwrap();

        let r = load_timed(user.path());
        assert!(r.is_valid("np"), "{:?}", r.info("np"));
        let r = std::sync::Arc::new(r);
        for rel in ["extra.png", "sub/deep.json", "pipe.webp", "link.webp"] {
            let reg = std::sync::Arc::clone(&r);
            let got = within(&format!("read_file({rel})"), BUDGET, move || {
                reg.read_file("np", rel)
            });
            assert_eq!(got, None, "{rel}");
        }
        // Regular files are still served.
        let (bytes, mime) = r.read_file("np", "idle.svg").unwrap();
        assert_eq!((bytes.as_slice(), mime), (SVG, "image/svg+xml"));
    }
}
