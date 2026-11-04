//! Corrupt files, the previous-copy fallback, newer schemas (read-only), crash
//! leftovers and the never-delete rule (SPEC §9).
//! Run with `cargo test -p focus-core --test store_recovery`.

mod store_support;

use std::fs;
use std::path::Path;

use focus_core::store::{
    backup_stamp, LoadNote, PersistedState, Store, StoreError, PREV_FILE, STATE_FILE, TMP_FILE,
};
use serde_json::json;
use store_support::*;

fn corrupt_name() -> String {
    format!("state.corrupt-{STAMP}.json")
}

fn newer_name() -> String {
    format!("state.newer-{STAMP}.json")
}

/// The newer-schema backups in `dir`, sorted.
fn newer_backups(dir: &Path) -> Vec<String> {
    file_names(dir)
        .into_iter()
        .filter(|n| n.starts_with("state.newer-"))
        .collect()
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

/// Make `dir` read-only (0555). Returns false (and restores 0755) when directory
/// permissions are not enforced for this user (root, CAP_DAC_OVERRIDE): the
/// scenario cannot be produced then and the caller skips. Run the binary under
/// `setpriv --bounding-set -dac_override,-dac_read_search --inh-caps
/// -dac_override,-dac_read_search` to exercise these tests as root.
#[cfg(unix)]
fn lock_dir(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).unwrap();
    let probe = dir.join("probe");
    if fs::write(&probe, b"").is_ok() {
        fs::remove_file(&probe).unwrap();
        unlock_dir(dir);
        eprintln!("skipped: directory permissions are not enforced for this user");
        return false;
    }
    true
}

#[cfg(unix)]
fn unlock_dir(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn store_corrupt_file_that_cannot_be_moved_makes_the_store_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    fs::create_dir(&app).unwrap();
    write(&app, STATE_FILE, b"garbage");
    if !lock_dir(&app) {
        return;
    }

    let store = Store::new(&app);
    let report = store.load(t0());
    unlock_dir(&app);
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

#[cfg(unix)]
#[test]
fn store_unmovable_corrupt_file_with_a_good_prev_recovers_but_is_read_only() {
    // The note is RecoveredFromPrevious, yet nothing was moved or restored and every
    // save fails: the shell must check is_read_only() after load (SPEC §9).
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    fs::create_dir(&app).unwrap();
    let good = to_json(&full_state());
    write(&app, PREV_FILE, &good);
    write(&app, STATE_FILE, b"garbage");
    if !lock_dir(&app) {
        return;
    }

    let store = Store::new(&app);
    let report = store.load(t0());
    unlock_dir(&app);
    match &report.note {
        LoadNote::RecoveredFromPrevious { backup, error } => {
            assert_eq!(
                backup,
                &app.join(STATE_FILE),
                "not moved: the original path"
            );
            assert!(error.contains("could not move it aside"), "{error}");
            assert!(error.contains("saving is disabled"), "{error}");
        }
        other => panic!("expected RecoveredFromPrevious, got {other:?}"),
    }
    assert_eq!(report.state, full_state());
    assert!(store.is_read_only());
    // Read-only even though the directory is writable again.
    match store.save(&full_state()) {
        Err(e @ StoreError::ReadOnly) => {
            assert!(e.to_string().contains("could not be moved aside"), "{e}")
        }
        other => panic!("expected ReadOnly, got {other:?}"),
    }
    // Nothing was moved, restored or created.
    assert_eq!(read(&app, STATE_FILE), b"garbage");
    assert_eq!(read(&app, PREV_FILE), good);
    assert_eq!(file_names(&app), [STATE_FILE, PREV_FILE]);
}

#[cfg(unix)]
#[test]
fn store_newer_file_in_an_unwritable_dir_is_left_as_its_own_backup() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    fs::create_dir(&app).unwrap();
    let newer = newer_doc(5000);
    write(&app, STATE_FILE, &newer);
    if !lock_dir(&app) {
        return;
    }
    let store = Store::new(&app);
    let report = store.load(t0());
    unlock_dir(&app);
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: app.join(STATE_FILE),
            version: 2
        }
    );
    assert!(store.is_read_only());
    assert_eq!(file_names(&app), [STATE_FILE]);
    assert_eq!(read(&app, STATE_FILE), newer);
}

#[cfg(unix)]
#[test]
fn store_identical_newer_backup_is_found_in_an_unwritable_dir() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    fs::create_dir(&app).unwrap();
    let newer = newer_doc(5000);
    write(&app, STATE_FILE, &newer);
    write(&app, "state.newer-20200101T000000Z.json", &newer);
    if !lock_dir(&app) {
        return;
    }
    let report = Store::new(&app).load(t0());
    unlock_dir(&app);
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: app.join("state.newer-20200101T000000Z.json"),
            version: 2
        }
    );
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
    // The newest good data is back in state.json, so the next run is read-only too,
    // and it reuses the backup this run made instead of adding another copy.
    assert_eq!(read(dir.path(), STATE_FILE), newer);
    let next = Store::new(dir.path());
    assert_eq!(
        next.load(at(24 * 60, 0)).note,
        LoadNote::NewerSchemaReadOnly {
            backup: dir.path().join(newer_name()),
            version: 2
        }
    );
    assert!(next.is_read_only());
    assert_eq!(newer_backups(dir.path()), [newer_name()]);
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

#[test]
fn store_repeated_loads_of_the_same_newer_file_keep_one_backup() {
    // Regression: every launch of an older build used to add another identical copy,
    // and nothing ever deletes them.
    let dir = tempfile::tempdir().unwrap();
    let newer = newer_doc(5000);
    write(dir.path(), STATE_FILE, &newer);
    let first = dir.path().join(newer_name());
    for day in 0..5 {
        let store = Store::new(dir.path());
        let report = store.load(at(day * 24 * 60, 0));
        assert_eq!(
            report.note,
            LoadNote::NewerSchemaReadOnly {
                backup: first.clone(),
                version: 2
            },
            "day {day}"
        );
        assert_eq!(report.state, PersistedState::default());
        assert!(store.is_read_only(), "day {day}");
    }
    // The same second as the first copy (would otherwise get a `-1` suffix).
    let again = Store::new(dir.path());
    again.load(t0());
    assert!(again.is_read_only());

    assert_eq!(newer_backups(dir.path()), [newer_name()]);
    assert_eq!(
        file_names(dir.path()),
        [STATE_FILE.to_string(), newer_name()]
    );
    assert_eq!(read(dir.path(), &newer_name()), newer);
    assert_eq!(read(dir.path(), STATE_FILE), newer);
}

#[test]
fn store_changed_newer_file_gets_its_own_backup() {
    // The newer build ran in between and wrote more: that content is backed up too,
    // and the earlier copy is kept. (5000 and 6000 serialize to the same length, so
    // this also checks that equal size alone is not taken as "identical".)
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    assert_eq!(newer_doc(5000).len(), newer_doc(6000).len());
    write(d, STATE_FILE, &newer_doc(5000));
    Store::new(d).load(t0());

    write(d, STATE_FILE, &newer_doc(6000));
    let day1 = at(24 * 60, 0);
    let second = format!("state.newer-{}.json", backup_stamp(day1));
    assert_eq!(
        Store::new(d).load(day1).note,
        LoadNote::NewerSchemaReadOnly {
            backup: d.join(&second),
            version: 2
        }
    );
    assert_eq!(newer_backups(d), [newer_name(), second.clone()]);
    assert_eq!(read(d, &newer_name()), newer_doc(5000));
    assert_eq!(read(d, &second), newer_doc(6000));

    // Back to the first content: its existing copy is reused.
    write(d, STATE_FILE, &newer_doc(5000));
    assert_eq!(
        Store::new(d).load(at(2 * 24 * 60, 0)).note,
        LoadNote::NewerSchemaReadOnly {
            backup: d.join(newer_name()),
            version: 2
        }
    );
    assert_eq!(newer_backups(d), [newer_name(), second]);
}

#[test]
fn store_identical_newer_backup_reuse_picks_the_first_by_name() {
    // E.g. duplicates left by an earlier build of this app: none is added or removed.
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let newer = newer_doc(5000);
    write(d, "state.newer-20260101T000000Z.json", b"something else");
    write(d, "state.newer-20260201T000000Z-1.json", &newer);
    write(d, "state.newer-20260201T000000Z.json", &newer);
    write(d, "state.newer-20260202T000000Z.json", &newer);
    write(d, STATE_FILE, &newer);
    let before = file_names(d);

    let report = Store::new(d).load(t0());
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: d.join("state.newer-20260201T000000Z-1.json"),
            version: 2
        }
    );
    assert_eq!(file_names(d), before);
    assert_eq!(
        read(d, "state.newer-20260101T000000Z.json"),
        b"something else"
    );
}

#[test]
fn store_identical_newer_backup_must_be_a_regular_file_with_the_newer_name() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let newer = newer_doc(5000);
    write(d, STATE_FILE, &newer);
    // Same bytes, but not a newer-schema backup by name.
    write(d, "state.corrupt-20200101T000000Z.json", &newer);
    write(d, "state.newer-20200101T000000Z.json.bak", &newer);
    write(d, "old-state.newer-20200101T000000Z.json", &newer);
    write(d, PREV_FILE, &newer);
    // A directory with a backup name.
    fs::create_dir(d.join("state.newer-20200101T000000Z.json")).unwrap();
    // A symlink with a backup name pointing at identical bytes: its target can
    // change or vanish, so it is not trusted as the backup.
    let elsewhere = tempfile::tempdir().unwrap();
    write(elsewhere.path(), "copy.json", &newer);
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        elsewhere.path().join("copy.json"),
        d.join("state.newer-20200102T000000Z.json"),
    )
    .unwrap();

    let report = Store::new(d).load(t0());
    assert_eq!(
        report.note,
        LoadNote::NewerSchemaReadOnly {
            backup: d.join(newer_name()),
            version: 2
        }
    );
    assert_eq!(read(d, &newer_name()), newer);
    assert!(d.join("state.newer-20200101T000000Z.json").is_dir());
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
