//! Corrupt files, the previous-copy fallback, newer schemas (read-only), crash
//! leftovers and the never-delete rule (SPEC §9).
//! Run with `cargo test -p focus-core --test store_recovery`.

mod store_support;

use std::fs;
use std::path::Path;

use focus_core::store::{
    LoadNote, PersistedState, Store, StoreError, PREV_FILE, STATE_FILE, TMP_FILE,
};
use serde_json::json;
use store_support::*;

fn corrupt_name() -> String {
    format!("state.corrupt-{STAMP}.json")
}

fn newer_name() -> String {
    format!("state.newer-{STAMP}.json")
}

fn write(dir: &Path, name: &str, bytes: &[u8]) {
    fs::write(dir.join(name), bytes).unwrap();
}

fn read(dir: &Path, name: &str) -> Vec<u8> {
    fs::read(dir.join(name)).unwrap()
}

fn newer_doc(points: u64) -> Vec<u8> {
    serde_json::to_vec_pretty(&json!({
        "schemaVersion": 2,
        "friendship": { "totalPoints": points, "brandNewField": [1, 2] },
        "somethingElse": "from the future"
    }))
    .unwrap()
}

// ---- corrupt state.json ----------------------------------------------------

#[test]
fn store_corrupt_file_is_moved_aside_and_defaults_used() {
    let dir = tempfile::tempdir().unwrap();
    let garbage = b"{\"schemaVersion\": 1, \"settings\": {\"sound\": tru";
    write(dir.path(), STATE_FILE, garbage);

    let store = Store::new(dir.path());
    let report = store.load(t0());
    match &report.note {
        LoadNote::CorruptBackedUp { backup, error } => {
            assert_eq!(backup, &dir.path().join(corrupt_name()));
            assert!(!error.is_empty());
        }
        other => panic!("expected CorruptBackedUp, got {other:?}"),
    }
    assert_eq!(report.state, PersistedState::default());
    assert!(!store.is_read_only());
    // Moved, not copied: the bytes are kept exactly; state.json is gone until the next save.
    assert_eq!(read(dir.path(), &corrupt_name()), garbage);
    assert!(!dir.path().join(STATE_FILE).exists());

    store.save(&full_state()).unwrap();
    assert_eq!(
        read(dir.path(), &corrupt_name()),
        garbage,
        "backup untouched"
    );
    assert_eq!(Store::new(dir.path()).load(t0()).note, LoadNote::Loaded);
}

#[test]
fn store_corrupt_variants_are_all_backed_up() {
    let valid = to_json(&full_state());
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty file", Vec::new()),
        ("whitespace", b"  \n\t ".to_vec()),
        ("truncated mid-write", valid[..valid.len() / 2].to_vec()),
        ("trailing garbage", [valid.clone(), b"}{".to_vec()].concat()),
        ("not utf-8", vec![0xff, 0xfe, 0x00, 0x7b]),
        ("NUL padding", vec![0u8; 64]),
        ("array", b"[]".to_vec()),
        ("null", b"null".to_vec()),
        ("bad version", br#"{"schemaVersion": "one"}"#.to_vec()),
        ("version zero", br#"{"schemaVersion": 0}"#.to_vec()),
        ("wrong shape", br#"{"schemaVersion": 1, "history": {"s1": 1}}"#.to_vec()),
        ("bad enum", br#"{"schemaVersion": 1, "activeSession": {"id":"s1","kind":"nap","plannedMs":1,"startedAt":"2026-03-02T09:00:00Z","accruedMs":0,"status":"running","checkpointAt":"2026-03-02T09:00:00Z"}}"#.to_vec()),
        ("bad timestamp", br#"{"schemaVersion": 1, "history": [{"id":"s1","kind":"focus","startedAt":"yesterday","endedAt":"2026-03-02T09:00:00Z","plannedMs":1,"activeMs":1,"outcome":"completed","endReason":"completed"}]}"#.to_vec()),
    ];
    for (what, bytes) in cases {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), STATE_FILE, &bytes);
        let report = Store::new(dir.path()).load(t0());
        assert!(
            matches!(report.note, LoadNote::CorruptBackedUp { .. }),
            "{what}: {:?}",
            report.note
        );
        assert_eq!(report.state, PersistedState::default(), "{what}");
        assert_eq!(read(dir.path(), &corrupt_name()), bytes, "{what}");
    }
}

#[test]
fn store_corrupt_backup_name_gets_a_numeric_suffix_when_taken() {
    let dir = tempfile::tempdir().unwrap();
    let taken0 = corrupt_name();
    let taken1 = format!("state.corrupt-{STAMP}-1.json");
    write(dir.path(), &taken0, b"older corrupt #0");
    write(dir.path(), &taken1, b"older corrupt #1");
    write(dir.path(), STATE_FILE, b"new garbage");

    let report = Store::new(dir.path()).load(t0());
    let expected = dir.path().join(format!("state.corrupt-{STAMP}-2.json"));
    assert!(
        matches!(&report.note, LoadNote::CorruptBackedUp { backup, .. } if *backup == expected),
        "{:?}",
        report.note
    );
    assert_eq!(read(dir.path(), &taken0), b"older corrupt #0");
    assert_eq!(read(dir.path(), &taken1), b"older corrupt #1");
    assert_eq!(fs::read(&expected).unwrap(), b"new garbage");
}

#[cfg(unix)]
#[test]
fn store_corrupt_backup_name_skips_dangling_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join(corrupt_name()))
        .unwrap();
    write(dir.path(), STATE_FILE, b"garbage");
    let report = Store::new(dir.path()).load(t0());
    let expected = dir.path().join(format!("state.corrupt-{STAMP}-1.json"));
    assert!(
        matches!(&report.note, LoadNote::CorruptBackedUp { backup, .. } if *backup == expected)
    );
    assert!(fs::symlink_metadata(dir.path().join(corrupt_name()))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn store_unreadable_state_path_is_moved_aside() {
    // A directory named state.json cannot be read as a file.
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(STATE_FILE)).unwrap();
    fs::write(dir.path().join(STATE_FILE).join("inner.txt"), b"keep me").unwrap();

    let store = Store::new(dir.path());
    let report = store.load(t0());
    match &report.note {
        LoadNote::CorruptBackedUp { backup, error } => {
            assert_eq!(backup, &dir.path().join(corrupt_name()));
            assert!(error.contains("unreadable"), "{error}");
        }
        other => panic!("expected CorruptBackedUp, got {other:?}"),
    }
    assert_eq!(
        fs::read(dir.path().join(corrupt_name()).join("inner.txt")).unwrap(),
        b"keep me"
    );
    store.save(&full_state()).unwrap();
    assert_eq!(Store::new(dir.path()).load(t0()).state, full_state());
}

#[cfg(unix)]
#[test]
fn store_corrupt_file_that_cannot_be_moved_makes_the_store_read_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    fs::create_dir(&app).unwrap();
    write(&app, STATE_FILE, b"garbage");
    fs::set_permissions(&app, fs::Permissions::from_mode(0o555)).unwrap();
    // Root ignores directory permissions; then this scenario cannot be produced.
    let probe = app.join("probe");
    if fs::write(&probe, b"").is_ok() {
        fs::remove_file(&probe).unwrap();
        fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: directory permissions are not enforced for this user");
        return;
    }

    let store = Store::new(&app);
    let report = store.load(t0());
    fs::set_permissions(&app, fs::Permissions::from_mode(0o755)).unwrap();
    match &report.note {
        LoadNote::CorruptBackedUp { backup, error } => {
            assert_eq!(backup, &app.join(STATE_FILE));
            assert!(error.contains("saving is disabled"), "{error}");
        }
        other => panic!("expected CorruptBackedUp, got {other:?}"),
    }
    assert!(store.is_read_only());
    assert!(matches!(
        store.save(&full_state()),
        Err(StoreError::ReadOnly)
    ));
    assert_eq!(read(&app, STATE_FILE), b"garbage");
}

// ---- state.prev.json ---------------------------------------------------------

#[test]
fn store_first_save_of_each_process_copies_state_to_prev() {
    let dir = tempfile::tempdir().unwrap();

    // Process 1: fresh install. No state.json yet, so nothing to copy.
    let p1 = Store::new(dir.path());
    assert_eq!(p1.load(t0()).note, LoadNote::Missing);
    p1.save(&other_state(10)).unwrap();
    p1.save(&other_state(11)).unwrap();
    assert!(!dir.path().join(PREV_FILE).exists());
    let after_p1 = read(dir.path(), STATE_FILE);

    // Process 2: the first save keeps process 1's last state; later saves do not copy.
    let p2 = Store::new(dir.path());
    assert_eq!(p2.load(t0()).state, other_state(11));
    p2.save(&other_state(20)).unwrap();
    assert_eq!(read(dir.path(), PREV_FILE), after_p1);
    p2.save(&other_state(21)).unwrap();
    p2.save(&other_state(22)).unwrap();
    assert_eq!(read(dir.path(), PREV_FILE), after_p1);
    let after_p2 = read(dir.path(), STATE_FILE);

    // Process 3.
    let p3 = Store::new(dir.path());
    p3.load(t0());
    p3.save(&other_state(30)).unwrap();
    assert_eq!(read(dir.path(), PREV_FILE), after_p2);
    assert_eq!(p3.prev_path(), dir.path().join(PREV_FILE));

    assert_eq!(file_names(dir.path()), [STATE_FILE, PREV_FILE]);
}

#[test]
fn store_prev_copy_keeps_raw_bytes() {
    // Hand-written (older-style) content is copied byte for byte, not re-serialized.
    let dir = tempfile::tempdir().unwrap();
    let original = br#"{"settings":{"sound":false},"history":[]}"#;
    write(dir.path(), STATE_FILE, original);
    let store = Store::new(dir.path());
    assert_eq!(store.load(t0()).note, LoadNote::Loaded);
    store.save(&full_state()).unwrap();
    assert_eq!(read(dir.path(), PREV_FILE), original);
}

#[test]
fn store_corrupt_state_recovers_from_prev() {
    let dir = tempfile::tempdir().unwrap();
    let good = to_json(&full_state());
    write(dir.path(), PREV_FILE, &good);
    write(dir.path(), STATE_FILE, b"\0\0\0 torn write");

    let store = Store::new(dir.path());
    let report = store.load(t0());
    match &report.note {
        LoadNote::RecoveredFromPrevious { backup, error } => {
            assert_eq!(backup, &dir.path().join(corrupt_name()));
            assert!(!error.is_empty());
        }
        other => panic!("expected RecoveredFromPrevious, got {other:?}"),
    }
    assert_eq!(report.state, full_state());
    assert!(!store.is_read_only());
    assert_eq!(read(dir.path(), &corrupt_name()), b"\0\0\0 torn write");
    assert_eq!(read(dir.path(), PREV_FILE), good, "prev is kept");
    // state.json is restored from prev so a crash before the next save loses nothing.
    assert_eq!(read(dir.path(), STATE_FILE), good);
    assert!(!dir.path().join(TMP_FILE).exists());
}

#[test]
fn store_recovered_state_survives_a_crash_before_the_next_save() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), PREV_FILE, &to_json(&full_state()));
    write(dir.path(), STATE_FILE, b"garbage");
    Store::new(dir.path()).load(t0());
    // "Crash": no save. Next process.
    let report = Store::new(dir.path()).load(t0());
    assert_eq!(report.note, LoadNote::Loaded);
    assert_eq!(report.state, full_state());
}

#[test]
fn store_recovery_then_save_keeps_every_good_copy() {
    let dir = tempfile::tempdir().unwrap();
    let good = to_json(&full_state());
    write(dir.path(), PREV_FILE, &good);
    write(dir.path(), STATE_FILE, b"garbage");
    let store = Store::new(dir.path());
    store.load(t0());
    store.save(&other_state(99)).unwrap();
    assert_eq!(read(dir.path(), PREV_FILE), good);
    assert_eq!(Store::new(dir.path()).load(t0()).state, other_state(99));
    assert_eq!(
        file_names(dir.path()),
        [
            corrupt_name(),
            STATE_FILE.to_string(),
            PREV_FILE.to_string()
        ]
    );
}

#[test]
fn store_corrupt_state_and_corrupt_prev_fall_back_to_defaults() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), PREV_FILE, b"also garbage");
    write(dir.path(), STATE_FILE, b"garbage");
    let store = Store::new(dir.path());
    let report = store.load(t0());
    assert!(
        matches!(&report.note, LoadNote::CorruptBackedUp { backup, .. }
            if *backup == dir.path().join(corrupt_name())),
        "{:?}",
        report.note
    );
    assert_eq!(report.state, PersistedState::default());
    assert_eq!(
        read(dir.path(), PREV_FILE),
        b"also garbage",
        "prev untouched"
    );
    assert!(!dir.path().join(STATE_FILE).exists());
    assert!(!store.is_read_only());
}

#[test]
fn store_missing_state_does_not_use_prev() {
    // SPEC: missing → defaults (e.g. the user removed state.json on purpose).
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), PREV_FILE, &to_json(&full_state()));
    let report = Store::new(dir.path()).load(t0());
    assert_eq!(report.note, LoadNote::Missing);
    assert_eq!(report.state, PersistedState::default());
}

#[test]
fn store_corrupt_state_with_newer_prev_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let newer = newer_doc(5000);
    write(dir.path(), PREV_FILE, &newer);
    write(dir.path(), STATE_FILE, b"garbage");
    let store = Store::new(dir.path());
    let report = store.load(t0());
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: dir.path().join(newer_name()),
            version: 2
        }
    );
    assert_eq!(report.state, PersistedState::default());
    assert!(store.is_read_only());
    assert!(matches!(
        store.save(&full_state()),
        Err(StoreError::ReadOnly)
    ));
    assert_eq!(read(dir.path(), &corrupt_name()), b"garbage");
    assert_eq!(read(dir.path(), &newer_name()), newer);
    assert_eq!(read(dir.path(), PREV_FILE), newer);
    // The newest good data is back in state.json, so the next run is read-only too.
    assert_eq!(read(dir.path(), STATE_FILE), newer);
    let next = Store::new(dir.path());
    assert!(matches!(
        next.load(t0()).note,
        LoadNote::NewerSchemaReadOnly { version: 2, .. }
    ));
    assert!(next.is_read_only());
}

// ---- newer schema ------------------------------------------------------------

#[test]
fn store_newer_schema_is_backed_up_and_store_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let newer = newer_doc(5000);
    write(dir.path(), STATE_FILE, &newer);
    write(dir.path(), PREV_FILE, &to_json(&full_state()));

    let store = Store::new(dir.path());
    let report = store.load(t0());
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: dir.path().join(newer_name()),
            version: 2
        }
    );
    assert_eq!(report.state, PersistedState::default());
    assert!(store.is_read_only());

    // Saving is refused for the whole run, and nothing on disk changes.
    for _ in 0..3 {
        match store.save(&PersistedState::default()) {
            Err(StoreError::ReadOnly) => {}
            other => panic!("expected ReadOnly, got {other:?}"),
        }
    }
    assert_eq!(read(dir.path(), STATE_FILE), newer);
    assert_eq!(read(dir.path(), &newer_name()), newer);
    assert_eq!(read(dir.path(), PREV_FILE), to_json(&full_state()));
    assert_eq!(
        file_names(dir.path()),
        [STATE_FILE.to_string(), newer_name(), PREV_FILE.to_string()]
    );
}

#[test]
fn store_read_only_lasts_for_the_whole_run() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), STATE_FILE, &newer_doc(1));
    let store = Store::new(dir.path());
    store.load(t0());
    assert!(store.is_read_only());
    // Even if the file is later replaced by a valid v1 file, this run stays read-only.
    write(dir.path(), STATE_FILE, &to_json(&full_state()));
    assert_eq!(store.load(t0()).note, LoadNote::Loaded);
    assert!(store.is_read_only());
    assert!(matches!(
        store.save(&full_state()),
        Err(StoreError::ReadOnly)
    ));
}

#[test]
fn store_newer_backup_name_gets_a_numeric_suffix_when_taken() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &newer_name(), b"earlier backup");
    write(dir.path(), STATE_FILE, &newer_doc(2));
    let report = Store::new(dir.path()).load(t0());
    let expected = dir.path().join(format!("state.newer-{STAMP}-1.json"));
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: expected.clone(),
            version: 2
        }
    );
    assert_eq!(read(dir.path(), &newer_name()), b"earlier backup");
    assert_eq!(fs::read(expected).unwrap(), newer_doc(2));
}

// ---- crash leftovers -----------------------------------------------------------

#[test]
fn store_load_ignores_a_leftover_temp_file() {
    // Crash after writing state.json.tmp but before the rename.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), STATE_FILE, &to_json(&full_state()));
    let half_written = to_json(&other_state(77));
    let half_written = &half_written[..half_written.len() / 3];
    write(dir.path(), TMP_FILE, half_written);

    let store = Store::new(dir.path());
    let report = store.load(t0());
    assert_eq!(report.note, LoadNote::Loaded);
    assert_eq!(report.state, full_state());
    assert_eq!(
        read(dir.path(), TMP_FILE),
        half_written,
        "load never touches it"
    );

    // The next save overwrites the leftover and renames it into place.
    store.save(&other_state(78)).unwrap();
    assert!(!dir.path().join(TMP_FILE).exists());
    assert_eq!(Store::new(dir.path()).load(t0()).state, other_state(78));
}

#[test]
fn store_load_ignores_a_complete_temp_file_too() {
    // Even a complete, newer-looking temp file is not trusted: it was never renamed.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), STATE_FILE, &to_json(&full_state()));
    write(dir.path(), TMP_FILE, &to_json(&other_state(500)));
    assert_eq!(Store::new(dir.path()).load(t0()).state, full_state());
}

#[test]
fn store_temp_file_alone_counts_as_missing() {
    // Crash during the very first save.
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), TMP_FILE, b"{\"schemaVer");
    let store = Store::new(dir.path());
    let report = store.load(t0());
    assert_eq!(report.note, LoadNote::Missing);
    assert_eq!(report.state, PersistedState::default());
    assert_eq!(read(dir.path(), TMP_FILE), b"{\"schemaVer");
    store.save(&full_state()).unwrap();
    assert_eq!(file_names(dir.path()), [STATE_FILE]);
}

// ---- never delete ----------------------------------------------------------------

#[test]
fn store_never_deletes_user_files() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "notes.txt", b"unrelated user file");
    write(d, "state.corrupt-20200101T000000Z.json", b"ancient");
    write(d, "state.newer-20200101T000000Z.json", b"ancient newer");

    // Run 1: corrupt with no prev → defaults, then saves.
    write(d, STATE_FILE, b"garbage 1");
    let s = Store::new(d);
    s.load(t0());
    s.save(&other_state(1)).unwrap();
    s.save(&other_state(2)).unwrap();

    // Run 2: normal run; its first save creates state.prev.json.
    let s = Store::new(d);
    assert_eq!(s.load(t0()).note, LoadNote::Loaded);
    s.save(&other_state(3)).unwrap();

    // Run 3: corrupt again (same stamp → numeric suffix), prev recovers.
    write(d, STATE_FILE, b"garbage 2");
    let s = Store::new(d);
    let report = s.load(t0());
    assert!(matches!(
        report.note,
        LoadNote::RecoveredFromPrevious { .. }
    ));
    assert_eq!(report.state, other_state(2));
    s.save(&other_state(4)).unwrap();

    // Run 4: newer schema → read-only.
    write(d, STATE_FILE, &newer_doc(9));
    let s = Store::new(d);
    s.load(t0());
    assert!(s.save(&other_state(5)).is_err());

    let names = file_names(d);
    for expected in [
        "notes.txt",
        "state.corrupt-20200101T000000Z.json",
        "state.newer-20200101T000000Z.json",
        &corrupt_name(),
        &format!("state.corrupt-{STAMP}-1.json"),
        &newer_name(),
        PREV_FILE,
        STATE_FILE,
    ] {
        assert!(
            names.iter().any(|n| n == expected),
            "{expected} missing: {names:?}"
        );
    }
    assert_eq!(read(d, "notes.txt"), b"unrelated user file");
    assert_eq!(read(d, &corrupt_name()), b"garbage 1");
    assert_eq!(
        read(d, &format!("state.corrupt-{STAMP}-1.json")),
        b"garbage 2"
    );
    assert_eq!(read(d, STATE_FILE), newer_doc(9));
}
