//! Pack discovery, precedence, resolution for rendering and `pack://` file
//! serving (SPEC §7). Disk packs live in temp dirs; the bundled pack is built
//! from in-memory files like the shell does.

mod packs {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    use focus_core::packs::{
        parse_manifest, render_keys, BundledPack, PackFormatKind, PackRegistry, PackSource,
        Rendering, BASE_STATES, MAX_ASSET_BYTES, MAX_MANIFEST_BYTES, MAX_PACK_DIRS, PLACEHOLDER_ID,
    };
    use focus_core::rewards::{UnlockTable, BUILTIN_UNLOCKS_JSON};

    const PREFIX: &str = "pack://localhost/";
    const SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' width='96' height='96'/>";

    // ---------------------------------------------------------------- helpers

    fn table() -> UnlockTable {
        serde_json::from_str(BUILTIN_UNLOCKS_JSON).expect("data/unlocks.json deserializes")
    }

    fn leak(b: Vec<u8>) -> &'static [u8] {
        Box::leak(b.into_boxed_slice())
    }

    fn manifest_json(id: &str, name: &str, sheets: &[(&str, &str)]) -> String {
        let anims: serde_json::Map<String, serde_json::Value> = sheets
            .iter()
            .map(|(k, sheet)| {
                (
                    k.to_string(),
                    serde_json::json!({"sheet": sheet, "frameWidth": 96, "frameHeight": 96, "frames": 1, "fps": 1}),
                )
            })
            .collect();
        serde_json::json!({"format": 1, "id": id, "name": name, "license": "CC0-1.0", "animations": anims})
            .to_string()
    }

    fn bundled(files: &[(&str, &[u8])]) -> BundledPack {
        BundledPack {
            files: files
                .iter()
                .map(|(p, b)| (p.to_string(), leak(b.to_vec())))
                .collect(),
        }
    }

    fn placeholder_bundle() -> BundledPack {
        let json = manifest_json(
            PLACEHOLDER_ID,
            "Pip",
            &[("idle", "idle.svg"), ("focus", "focus.svg")],
        );
        bundled(&[
            ("pack.json", json.as_bytes()),
            ("idle.svg", SVG),
            ("focus.svg", b"<svg/>"),
            ("LICENSE.md", b"CC0"),
        ])
    }

    /// A BundledPack built from a directory on disk, as the shell's include_dir walk does.
    fn bundle_dir(root: &Path) -> BundledPack {
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, &'static [u8])>) {
            let mut entries: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            entries.sort();
            for p in entries {
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    let rel = p
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.push((rel, leak(fs::read(&p).unwrap())));
                }
            }
        }
        let mut files = Vec::new();
        walk(root, root, &mut files);
        BundledPack { files }
    }

    fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Write a valid one-animation native pack into `<dir>/<folder>`.
    fn native(dir: &Path, folder: &str, id: &str, name: &str) -> PathBuf {
        let root = dir.join(folder);
        write(
            root.join("pack.json"),
            manifest_json(id, name, &[("idle", "idle.svg")]),
        );
        write(root.join("idle.svg"), SVG);
        root
    }

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for e in fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let target = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_dir(&e.path(), &target);
            } else {
                fs::copy(e.path(), target).unwrap();
            }
        }
    }

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/packs")
            .join(name)
    }

    fn ids(r: &PackRegistry) -> Vec<String> {
        r.list().into_iter().map(|i| i.id).collect()
    }

    fn assert_unique_ids(r: &PackRegistry) {
        let all = ids(r);
        let set: BTreeSet<&String> = all.iter().collect();
        assert_eq!(set.len(), all.len(), "listing ids must be unique: {all:?}");
    }

    fn info(r: &PackRegistry, id: &str) -> focus_core::packs::PackInfo {
        r.info(id)
            .unwrap_or_else(|| panic!("{id} not listed in {:?}", ids(r)))
            .clone()
    }

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    // ---------------------------------------------------------------- bundled

    #[test]
    fn bundled_placeholder_is_listed_first_valid_and_served_from_memory() {
        let r = PackRegistry::load(placeholder_bundle(), None, None);
        let list = r.list();
        assert_eq!(list.len(), 1);
        let p = &list[0];
        assert_eq!((p.id.as_str(), p.name.as_str()), (PLACEHOLDER_ID, "Pip"));
        assert_eq!(
            (p.source, p.format_kind, p.valid),
            (PackSource::Bundled, PackFormatKind::Native, true)
        );
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert!(r.is_valid(PLACEHOLDER_ID));
        assert_eq!(
            r.read_file(PLACEHOLDER_ID, "idle.svg"),
            Some((SVG.to_vec(), "image/svg+xml"))
        );
        assert_eq!(
            r.read_file(PLACEHOLDER_ID, "pack.json").map(|(_, m)| m),
            Some("application/json")
        );
        assert_eq!(
            r.read_file(PLACEHOLDER_ID, "LICENSE.md"),
            None,
            "disallowed type"
        );
        assert_eq!(r.read_file(PLACEHOLDER_ID, "missing.svg"), None);
        assert_eq!(r.read_file(PLACEHOLDER_ID, "../idle.svg"), None);
        assert_eq!(r.read_file(PLACEHOLDER_ID, "/idle.svg"), None);
    }

    #[test]
    fn bundled_paths_are_normalized_and_hidden_files_ignored() {
        let json = manifest_json(PLACEHOLDER_ID, "Pip", &[("idle", "art/idle.svg")]);
        let b = bundled(&[
            ("./pack.json", json.as_bytes()),
            ("art/idle.svg", SVG),
            (".DS_Store", b"x"),
            ("art/.x.svg", SVG),
        ]);
        let r = PackRegistry::load(b, None, None);
        assert!(r.is_valid(PLACEHOLDER_ID), "{:?}", r.list());
        assert!(r.read_file(PLACEHOLDER_ID, "art/idle.svg").is_some());
        assert_eq!(r.read_file(PLACEHOLDER_ID, ".DS_Store"), None);
        assert_eq!(r.read_file(PLACEHOLDER_ID, "art/.x.svg"), None);
    }

    #[test]
    fn broken_bundle_still_lists_placeholder_and_renders_static_companion() {
        for (b, needle) in [
            (BundledPack::default(), "no pack.json"),
            (bundled(&[("pack.json", b"{oops")]), "pack.json:"),
            (
                bundled(&[("pack.json", &[0xffu8, 0xfe, 0x00][..])]),
                "UTF-8",
            ),
            (
                bundled(&[(
                    "pack.json",
                    manifest_json(PLACEHOLDER_ID, "Pip", &[("idle", "gone.svg")]).as_bytes(),
                )]),
                "missing",
            ),
        ] {
            let r = PackRegistry::load(b, None, None);
            let p = info(&r, PLACEHOLDER_ID);
            assert!(!p.valid);
            assert!(
                p.errors.iter().any(|e| e.contains(needle)),
                "{needle}: {:?}",
                p.errors
            );
            let rp = r.resolve_pack(PLACEHOLDER_ID, &table(), PREFIX);
            assert_eq!(rp.fallback_from, None);
            assert_eq!(rp.info.id, PLACEHOLDER_ID);
            assert!(rp.animations.values().all(Option::is_none));
            assert!(BASE_STATES.iter().all(|k| rp.animations.contains_key(*k)));
            assert_eq!(
                r.resolve_pack("other", &table(), PREFIX)
                    .fallback_from
                    .as_deref(),
                Some("other")
            );
            assert_eq!(
                r.read_file(PLACEHOLDER_ID, "pack.json"),
                None,
                "invalid packs are not served"
            );
        }
    }

    #[test]
    fn bundled_pack_must_use_the_placeholder_id() {
        let json = manifest_json("pip", "Pip", &[("idle", "idle.svg")]);
        let r = PackRegistry::load(
            bundled(&[("pack.json", json.as_bytes()), ("idle.svg", SVG)]),
            None,
            None,
        );
        let p = info(&r, PLACEHOLDER_ID);
        assert!(!p.valid);
        assert!(
            p.errors.iter().any(|e| e.contains("must use the id")),
            "{:?}",
            p.errors
        );
        assert_eq!(r.info("pip"), None);
    }

    // ---------------------------------------------------------------- discovery

    #[test]
    fn user_pack_from_fixture_resolves_with_fallbacks_and_encoded_urls() {
        let user = tmp();
        copy_dir(&fixture("fixture-sprout"), &user.path().join("sprout-dir"));
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let p = info(&r, "fixture-sprout");
        assert!(p.valid, "{:?}", p.errors);
        assert_eq!(
            (p.source, p.format_kind),
            (PackSource::User, PackFormatKind::Native)
        );
        assert_eq!(p.license.as_deref(), Some("CC0-1.0"));
        assert!(
            !p.warnings.iter().any(|w| w.contains("license")),
            "{:?}",
            p.warnings
        );

        let t = table();
        let rp = r.resolve_pack("fixture-sprout", &t, PREFIX);
        assert_eq!(rp.fallback_from, None);
        assert_eq!(rp.rendering, Rendering::Smooth);
        let m = r.manifest("fixture-sprout").unwrap();
        let keys: Vec<String> = rp.animations.keys().cloned().collect();
        let mut expected = render_keys(&t, m);
        expected.sort();
        assert_eq!(keys, expected, "resolves exactly the render keys");

        let idle = rp.animations["idle"].clone().unwrap();
        assert_eq!(idle.url, "pack://localhost/fixture-sprout/sheet.svg");
        assert_eq!(
            (idle.key.as_str(), idle.source_key.as_str()),
            ("idle", "idle")
        );
        assert_eq!(
            (idle.sheet_columns, idle.sheet_rows, idle.still_frame),
            (4, 2, 0)
        );

        let paused = rp.animations["paused"].clone().unwrap();
        assert_eq!(
            (paused.key.as_str(), paused.source_key.as_str()),
            ("paused", "focus")
        );
        assert_eq!((paused.row, paused.frames, paused.still_frame), (1, 2, 1));
        assert_eq!(paused.frame_durations_ms, Some(vec![900, 300]));

        let react = rp.animations["react"].clone().unwrap();
        assert_eq!(
            react.url,
            "pack://localhost/fixture-sprout/react%20sheet.svg"
        );
        assert!(!react.looping);
        assert_eq!(
            (react.sheet_columns, react.sheet_rows),
            (3, 1),
            "defaults: column + frames, row + 1"
        );

        assert_eq!(
            rp.animations["completed"].as_ref().unwrap().source_key,
            "idle"
        );
        assert_eq!(rp.animations["break"].as_ref().unwrap().source_key, "idle");
        for k in ["blink", "wave", "stretch", "sleepy", "celebrate"] {
            assert_eq!(rp.animations[k], None, "{k}");
        }
        assert_eq!(
            rp.palettes["evening"].character_filter.as_deref(),
            Some("brightness(0.9) saturate(0.9)")
        );
        // The URL path decodes back to a servable file.
        let (bytes, mime) = r.read_file("fixture-sprout", "react sheet.svg").unwrap();
        assert!(bytes.starts_with(b"<svg"));
        assert_eq!(mime, "image/svg+xml");
    }

    #[test]
    fn missing_or_empty_directories_are_fine() {
        let empty = tmp();
        let r = PackRegistry::load(
            placeholder_bundle(),
            Some(Path::new("/nonexistent/packs")),
            Some(empty.path()),
        );
        assert_eq!(ids(&r), vec![PLACEHOLDER_ID]);
        let file = empty.path().join("not-a-dir");
        write(&file, "x");
        let r = PackRegistry::load(placeholder_bundle(), Some(&file), None);
        assert_eq!(ids(&r), vec![PLACEHOLDER_ID]);
    }

    #[test]
    fn list_is_bundled_first_then_by_name() {
        let user = tmp();
        native(user.path(), "one", "zeta", "beta");
        native(user.path(), "two", "alpha", "Gamma");
        native(user.path(), "three", "mid", "Alpha");
        let dev = tmp();
        native(dev.path(), "four", "devpack", "delta");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), Some(dev.path()));
        let names: Vec<String> = r.list().into_iter().map(|i| i.name).collect();
        assert_eq!(names, vec!["Pip", "Alpha", "beta", "delta", "Gamma"]);
        assert_eq!(info(&r, "devpack").source, PackSource::Dev);
    }

    #[test]
    fn invalid_packs_are_listed_with_errors_and_never_activated() {
        let user = tmp();
        write(user.path().join("broken/pack.json"), "{ not json");
        write(user.path().join("Bad Name/pack.json"), "[]");
        let m = user.path().join("nosheet");
        write(
            m.join("pack.json"),
            manifest_json("nosheet", "No sheet", &[("idle", "idle.png")]),
        );
        write(user.path().join("empty-folder/readme.txt"), "hi");
        fs::create_dir_all(user.path().join(".hidden-pack")).unwrap();
        native(&user.path().join(".git"), "x", "x", "x");
        write(user.path().join("loose-file.json"), "{}");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_unique_ids(&r);

        let broken = info(&r, "broken");
        assert!(
            !broken.valid && broken.errors[0].starts_with("pack.json: manifest is not valid JSON"),
            "{broken:?}"
        );
        assert_eq!(broken.name, "broken");
        let bad = info(&r, "user:Bad Name");
        assert!(
            !bad.valid && bad.errors[0].contains("JSON object"),
            "{bad:?}"
        );
        let nosheet = info(&r, "nosheet");
        assert!(
            !nosheet.valid && nosheet.errors.iter().any(|e| e.contains("missing")),
            "{nosheet:?}"
        );
        assert_eq!(
            nosheet.name, "No sheet",
            "metadata survives validation errors"
        );
        let empty = info(&r, "empty-folder");
        assert!(empty.errors[0].contains("no pack.json"), "{empty:?}");
        assert_eq!(
            r.list().len(),
            5,
            "hidden folders and loose files are skipped: {:?}",
            ids(&r)
        );

        for id in ["broken", "nosheet", "empty-folder", "user:Bad Name"] {
            assert!(!r.is_valid(id));
            assert!(r.manifest(id).is_none());
            assert!(r.unavailable_reason(id).is_some());
            let rp = r.resolve_pack(id, &table(), PREFIX);
            assert_eq!(rp.info.id, PLACEHOLDER_ID);
            assert_eq!(rp.fallback_from.as_deref(), Some(id));
            assert!(rp.animations["idle"].is_some());
        }
        assert_eq!(r.read_file("nosheet", "pack.json"), None);
    }

    #[test]
    fn unavailable_reasons() {
        let user = tmp();
        native(user.path(), "good", "good", "Good");
        write(user.path().join("bad/pack.json"), "{");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_eq!(r.unavailable_reason("good"), None);
        assert_eq!(r.unavailable_reason(PLACEHOLDER_ID), None);
        assert!(r
            .unavailable_reason("bad")
            .unwrap()
            .contains("not valid JSON"));
        assert!(r.unavailable_reason("ghost").unwrap().contains("no pack"));
    }

    #[test]
    fn disk_packs_cannot_claim_the_placeholder_id() {
        let user = tmp();
        native(user.path(), "my-copy", PLACEHOLDER_ID, "Copied placeholder");
        native(
            user.path(),
            PLACEHOLDER_ID,
            "other",
            "Folder named placeholder",
        );
        let dev = tmp();
        native(dev.path(), "dev-copy", PLACEHOLDER_ID, "Dev placeholder");
        write(dev.path().join("dev-copy/idle.svg"), b"<svg id='dev'/>");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), Some(dev.path()));
        assert_unique_ids(&r);
        for id in ["user:my-copy", "dev:dev-copy"] {
            let p = info(&r, id);
            assert!(!p.valid);
            assert!(
                p.errors
                    .iter()
                    .any(|e| e.contains("reserved for the bundled pack")),
                "{p:?}"
            );
        }
        // A native pack in a folder called "placeholder" is fine: its manifest id counts.
        assert!(r.is_valid("other"));
        let bundled = info(&r, PLACEHOLDER_ID);
        assert_eq!((bundled.source, bundled.valid), (PackSource::Bundled, true));
        assert_eq!(
            r.read_file(PLACEHOLDER_ID, "idle.svg"),
            Some((SVG.to_vec(), "image/svg+xml"))
        );
        assert_eq!(
            r.resolve_pack(PLACEHOLDER_ID, &table(), PREFIX).info.source,
            PackSource::Bundled
        );
    }

    #[test]
    fn dev_overrides_user_on_id_collision() {
        let user = tmp();
        native(user.path(), "shared", "shared", "User copy");
        let dev = tmp();
        let d = native(dev.path(), "shared-dev", "shared", "Dev copy");
        write(d.join("idle.svg"), b"<svg id='dev'/>");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), Some(dev.path()));
        assert_eq!(ids(&r), vec![PLACEHOLDER_ID, "shared"]);
        let p = info(&r, "shared");
        assert_eq!((p.name.as_str(), p.source), ("Dev copy", PackSource::Dev));
        assert!(
            p.warnings
                .iter()
                .any(|w| w.contains("overrides the user pack")),
            "{:?}",
            p.warnings
        );
        assert_eq!(
            r.read_file("shared", "idle.svg").unwrap().0,
            b"<svg id='dev'/>".to_vec()
        );
        // Without the dev dir the user pack is back.
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_eq!(info(&r, "shared").source, PackSource::User);
    }

    #[test]
    fn dev_override_applies_even_when_the_dev_pack_is_broken() {
        let user = tmp();
        native(user.path(), "shared", "shared", "User copy");
        let dev = tmp();
        write(dev.path().join("shared/pack.json"), "{");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), Some(dev.path()));
        assert_eq!(ids(&r), vec![PLACEHOLDER_ID, "shared"]);
        let p = info(&r, "shared");
        assert_eq!((p.source, p.valid), (PackSource::Dev, false));
        assert_eq!(
            r.resolve_pack("shared", &table(), PREFIX)
                .fallback_from
                .as_deref(),
            Some("shared")
        );
    }

    #[test]
    fn duplicate_ids_within_a_source() {
        let user = tmp();
        native(user.path(), "a-dir", "dup", "First");
        native(user.path(), "b-dir", "dup", "Second");
        write(user.path().join("foo/pack.json"), "{ broken");
        native(user.path(), "zzz", "foo", "Real foo");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_unique_ids(&r);
        assert_eq!(info(&r, "dup").name, "First");
        let loser = info(&r, "user:b-dir");
        assert!(
            !loser.valid
                && loser
                    .errors
                    .iter()
                    .any(|e| e.contains("duplicate pack id \"dup\"")),
            "{loser:?}"
        );
        // A parsed manifest owns its id over a broken pack whose id was guessed from the folder.
        let foo = info(&r, "foo");
        assert!(foo.valid && foo.name == "Real foo", "{foo:?}");
        assert!(!info(&r, "user:foo").valid);
        assert_eq!(r.read_file("user:b-dir", "idle.svg"), None);
    }

    #[test]
    fn folder_scan_is_capped() {
        let user = tmp();
        for i in 0..(MAX_PACK_DIRS + 3) {
            fs::create_dir_all(user.path().join(format!("d{i:04}"))).unwrap();
        }
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_eq!(r.list().len(), MAX_PACK_DIRS + 1);
    }

    #[test]
    fn metadata_is_cleaned_and_bounded() {
        let user = tmp();
        let root = user.path().join("meta");
        let json = serde_json::json!({
            "format": 1, "id": "meta", "name": "  My\u{7}\n  Pack  ", "author": "\tA. Author ",
            "description": "d".repeat(600),
            "animations": {"idle": {"sheet": "idle.svg", "frameWidth": 1, "frameHeight": 1, "frames": 1, "fps": 1}}
        });
        write(root.join("pack.json"), json.to_string());
        write(root.join("idle.svg"), SVG);
        native(user.path(), "noname", "noname", "   ");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let p = info(&r, "meta");
        assert!(p.valid, "{p:?}");
        assert_eq!(p.name, "My Pack");
        assert_eq!(p.author.as_deref(), Some("A. Author"));
        assert_eq!(p.description.as_ref().map(|d| d.chars().count()), Some(500));
        assert!(
            p.warnings
                .iter()
                .any(|w| w.contains("description truncated")),
            "{:?}",
            p.warnings
        );
        assert!(
            p.warnings
                .iter()
                .any(|w| w == "no license declared in pack.json"),
            "{:?}",
            p.warnings
        );
        assert_eq!(info(&r, "noname").name, "noname");
    }

    #[test]
    fn manifest_size_is_capped_on_disk() {
        let user = tmp();
        let root = native(user.path(), "big", "big", "Big");
        let mut json = manifest_json("big", "Big", &[("idle", "idle.svg")]);
        json.push_str(&" ".repeat(MAX_MANIFEST_BYTES as usize));
        write(root.join("pack.json"), json);
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let p = info(&r, "big");
        assert!(
            !p.valid && p.errors[0].contains("larger than 256 KiB"),
            "{p:?}"
        );
    }

    #[test]
    fn utf8_bom_is_accepted() {
        let user = tmp();
        let root = native(user.path(), "bom", "bom", "Bom");
        let json = format!(
            "\u{feff}{}",
            manifest_json("bom", "Bom", &[("idle", "idle.svg")])
        );
        write(root.join("pack.json"), json);
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert!(r.is_valid("bom"), "{:?}", r.info("bom"));
    }

    // ---------------------------------------------------------------- resolve_pack

    #[test]
    fn resolve_pack_falls_back_to_the_placeholder() {
        let r = PackRegistry::load(placeholder_bundle(), None, None);
        let t = table();
        let rp = r.resolve_pack("ghost", &t, PREFIX);
        assert_eq!(rp.fallback_from.as_deref(), Some("ghost"));
        assert_eq!(rp.info.id, PLACEHOLDER_ID);
        let idle = rp.animations["idle"].as_ref().unwrap();
        assert_eq!(idle.url, "pack://localhost/placeholder/idle.svg");
        assert_eq!(rp.animations["focus"].as_ref().unwrap().source_key, "focus");
        assert_eq!(rp.animations["paused"].as_ref().unwrap().source_key, "idle");
        let own = r.resolve_pack(PLACEHOLDER_ID, &t, PREFIX);
        assert_eq!(own.fallback_from, None);
        assert_eq!(own.animations, rp.animations);
        // Every render key is present, with null when unresolved.
        let m = parse_manifest(&manifest_json(
            PLACEHOLDER_ID,
            "Pip",
            &[("idle", "idle.svg"), ("focus", "focus.svg")],
        ))
        .unwrap();
        assert_eq!(own.animations.len(), render_keys(&t, &m).len());
        assert_eq!(own.animations["wave"], None);
        assert_eq!(own.animations["react"], None);
    }

    #[test]
    fn resolve_pack_url_encoding_and_prefixes() {
        let user = tmp();
        let root = user.path().join("enc");
        write(
            root.join("pack.json"),
            manifest_json(
                "enc",
                "Enc",
                &[
                    ("idle", "sub dir/idle#1.png"),
                    ("focus", "ü/ß?.svg"),
                    ("break", "a%b.webp"),
                ],
            ),
        );
        write(root.join("sub dir/idle#1.png"), b"png");
        write(root.join("ü/ß?.svg"), SVG);
        write(root.join("a%b.webp"), b"webp");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert!(r.is_valid("enc"), "{:?}", r.info("enc"));
        let t = table();
        let rp = r.resolve_pack("enc", &t, PREFIX);
        assert_eq!(
            rp.animations["idle"].as_ref().unwrap().url,
            "pack://localhost/enc/sub%20dir/idle%231.png"
        );
        assert_eq!(
            rp.animations["focus"].as_ref().unwrap().url,
            "pack://localhost/enc/%C3%BC/%C3%9F%3F.svg"
        );
        assert_eq!(
            rp.animations["break"].as_ref().unwrap().url,
            "pack://localhost/enc/a%25b.webp"
        );
        let win = r.resolve_pack("enc", &t, "http://pack.localhost");
        assert_eq!(
            win.animations["idle"].as_ref().unwrap().url,
            "http://pack.localhost/enc/sub%20dir/idle%231.png"
        );
        // The protocol handler decodes the URL path; the decoded path is what read_file takes.
        assert_eq!(
            r.read_file("enc", "sub dir/idle#1.png").unwrap(),
            (b"png".to_vec(), "image/png")
        );
        assert_eq!(r.read_file("enc", "ü/ß?.svg").unwrap().1, "image/svg+xml");
    }

    #[test]
    fn resolve_pack_sanitizes_palettes_and_carries_rendering() {
        let user = tmp();
        let root = user.path().join("pal");
        let json = serde_json::json!({
            "format": 1, "id": "pal", "name": "Pal", "license": "MIT", "rendering": "pixelated",
            "animations": {"idle": {"sheet": "idle.png", "frameWidth": 16, "frameHeight": 16, "frames": 2, "fps": 3,
                                     "loop": false, "stillFrame": 1, "frameDurationsMs": [100, 200]}},
            "palettes": {
                "evening": {"background": "url(file:///etc/passwd)", "accent": "#e9a86b",
                            "characterFilter": "blur(9px)", "label": "Eve"},
                "Bad Id": {"accent": "#fff"},
                "moss": {}
            }
        });
        write(root.join("pack.json"), json.to_string());
        write(root.join("idle.png"), b"png");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let p = info(&r, "pal");
        assert!(p.valid, "{p:?}");
        assert!(
            p.warnings
                .iter()
                .any(|w| w.starts_with("palettes.evening.background")),
            "{:?}",
            p.warnings
        );
        assert!(
            p.warnings
                .iter()
                .any(|w| w.starts_with("palettes.evening.characterFilter")),
            "{:?}",
            p.warnings
        );
        let rp = r.resolve_pack("pal", &table(), PREFIX);
        assert_eq!(rp.rendering, Rendering::Pixelated);
        let eve = &rp.palettes["evening"];
        assert_eq!(
            (eve.background.as_deref(), eve.accent.as_deref()),
            (None, Some("#e9a86b"))
        );
        assert_eq!(
            (eve.character_filter.as_deref(), eve.label.as_deref()),
            (None, Some("Eve"))
        );
        assert!(!rp.palettes.contains_key("Bad Id"));
        assert!(rp.palettes.contains_key("moss"));
        let idle = rp.animations["idle"].as_ref().unwrap();
        assert!(!idle.looping);
        assert_eq!(
            (idle.still_frame, idle.frame_durations_ms.clone()),
            (1, Some(vec![100, 200]))
        );
        assert_eq!(
            (idle.frame_width, idle.frame_height, idle.fps),
            (16, 16, 3.0)
        );
    }

    // ---------------------------------------------------------------- read_file

    #[test]
    fn read_file_refuses_unsafe_requests() {
        let user = tmp();
        let root = native(user.path(), "safe", "safe", "Safe");
        write(root.join(".secret.png"), b"s");
        write(root.join("notes.txt"), b"n");
        write(root.join("sub/x.png"), b"x");
        write(user.path().join("outside.png"), b"o");
        write(user.path().join("broken/pack.json"), "{");
        write(user.path().join("broken/x.png"), b"x");
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_eq!(
            r.read_file("safe", "idle.svg"),
            Some((SVG.to_vec(), "image/svg+xml"))
        );
        assert_eq!(
            r.read_file("safe", "sub/x.png"),
            Some((b"x".to_vec(), "image/png"))
        );
        for bad in [
            "../outside.png",
            "sub/../../outside.png",
            "/etc/passwd",
            "sub\\x.png",
            ".secret.png",
            "notes.txt",
            "missing.png",
            "sub",
            "sub/",
            "",
            "idle.svg\0.png",
            "C:/idle.svg",
        ] {
            assert_eq!(r.read_file("safe", bad), None, "{bad:?}");
        }
        assert_eq!(r.read_file("ghost", "idle.svg"), None);
        assert_eq!(
            r.read_file("broken", "x.png"),
            None,
            "invalid packs are not served"
        );
        assert_eq!(r.read_file("user:broken", "x.png"), None);
    }

    #[test]
    fn read_file_and_validation_cap_asset_size() {
        let user = tmp();
        let root = native(user.path(), "sizes", "sizes", "Sizes");
        let big = fs::File::create(root.join("big.png")).unwrap();
        big.set_len(MAX_ASSET_BYTES + 1).unwrap(); // sparse
        let exact = fs::File::create(root.join("exact.png")).unwrap();
        exact.set_len(MAX_ASSET_BYTES).unwrap();
        let bigpack = user.path().join("bigsheet");
        write(
            bigpack.join("pack.json"),
            manifest_json("bigsheet", "Big sheet", &[("idle", "idle.png")]),
        );
        fs::File::create(bigpack.join("idle.png"))
            .unwrap()
            .set_len(MAX_ASSET_BYTES + 1)
            .unwrap();
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert_eq!(r.read_file("sizes", "big.png"), None);
        assert_eq!(
            r.read_file("sizes", "exact.png")
                .map(|(b, _)| b.len() as u64),
            Some(MAX_ASSET_BYTES)
        );
        let p = info(&r, "bigsheet");
        assert!(
            !p.valid && p.errors.iter().any(|e| e.contains("too large")),
            "{p:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_pack_are_refused() {
        use std::os::unix::fs::symlink;
        let user = tmp();
        let outside = tmp();
        write(outside.path().join("secret.png"), b"secret");
        write(outside.path().join("dir/y.png"), b"y");
        let root = native(user.path(), "links", "links", "Links");
        write(root.join("real.png"), b"real");
        write(root.join(".hidden.png"), b"h");
        symlink(outside.path().join("secret.png"), root.join("escape.png")).unwrap();
        symlink(outside.path().join("dir"), root.join("linked")).unwrap();
        symlink(root.join("real.png"), root.join("alias.png")).unwrap();
        symlink(root.join(".hidden.png"), root.join("visible.png")).unwrap();
        symlink(root.join("pack.json"), root.join("manifest.png")).unwrap();
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert!(r.is_valid("links"), "{:?}", r.info("links"));
        assert_eq!(r.read_file("links", "escape.png"), None);
        assert_eq!(r.read_file("links", "linked/y.png"), None);
        assert_eq!(
            r.read_file("links", "visible.png"),
            None,
            "a link to a hidden file is refused"
        );
        assert_eq!(
            r.read_file("links", "manifest.png"),
            Some((fs::read(root.join("pack.json")).unwrap(), "image/png"))
        );
        assert_eq!(
            r.read_file("links", "alias.png"),
            Some((b"real".to_vec(), "image/png"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_sheets_and_manifests_escaping_the_pack_make_it_invalid() {
        use std::os::unix::fs::symlink;
        let user = tmp();
        let outside = tmp();
        write(outside.path().join("idle.svg"), SVG);
        write(
            outside.path().join("pack.json"),
            manifest_json("evil", "Evil", &[("idle", "idle.svg")]),
        );
        let a = user.path().join("sheet-escape");
        write(
            a.join("pack.json"),
            manifest_json("sheet-escape", "S", &[("idle", "idle.svg")]),
        );
        symlink(outside.path().join("idle.svg"), a.join("idle.svg")).unwrap();
        let b = user.path().join("manifest-escape");
        fs::create_dir_all(&b).unwrap();
        symlink(outside.path().join("pack.json"), b.join("pack.json")).unwrap();
        write(b.join("idle.svg"), SVG);
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let s = info(&r, "sheet-escape");
        assert!(
            !s.valid && s.errors.iter().any(|e| e.contains("missing")),
            "{s:?}"
        );
        let m = info(&r, "manifest-escape");
        assert!(
            !m.valid && m.errors[0].contains("cannot be read inside the pack folder"),
            "{m:?}"
        );
        assert_eq!(r.info("evil"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_pack_folder_is_followed() {
        use std::os::unix::fs::symlink;
        let user = tmp();
        let elsewhere = tmp();
        let real = native(elsewhere.path(), "real", "linked-pack", "Linked");
        symlink(&real, user.path().join("linked")).unwrap();
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        assert!(r.is_valid("linked-pack"), "{:?}", r.list());
        assert_eq!(
            r.read_file("linked-pack", "idle.svg").unwrap().0,
            SVG.to_vec()
        );
    }

    // ---------------------------------------------------------------- shipped pack

    /// Loads the repo's `packs/placeholder` exactly as the shell embeds it. Skips
    /// (passes with a note) until the placeholder art has landed.
    #[test]
    fn shipped_placeholder_pack_is_valid() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/placeholder");
        if !dir.join("pack.json").exists() {
            eprintln!(
                "SKIP shipped_placeholder_pack_is_valid: {} has no pack.json yet",
                dir.display()
            );
            return;
        }
        let r = PackRegistry::load(bundle_dir(&dir), None, None);
        let p = info(&r, PLACEHOLDER_ID);
        assert!(p.valid, "packs/placeholder is invalid: {:?}", p.errors);
        assert!(p.license.is_some(), "the shipped pack declares a license");
        if !p.warnings.is_empty() {
            eprintln!("packs/placeholder warnings: {:#?}", p.warnings);
        }
        let rp = r.resolve_pack(PLACEHOLDER_ID, &table(), PREFIX);
        assert_eq!(rp.fallback_from, None);
        for k in BASE_STATES {
            assert!(
                rp.animations[k].is_some(),
                "base state {k} does not resolve"
            );
        }
        // Every resolved URL is servable after the protocol handler's percent-decoding.
        for (k, a) in rp
            .animations
            .iter()
            .filter_map(|(k, a)| a.as_ref().map(|a| (k, a)))
        {
            let encoded = a
                .url
                .strip_prefix("pack://localhost/placeholder/")
                .expect("placeholder URL");
            let rel = percent_encoding::percent_decode_str(encoded)
                .decode_utf8()
                .unwrap();
            assert!(
                r.read_file(PLACEHOLDER_ID, &rel).is_some(),
                "{k}: {rel} is not servable"
            );
        }
    }
}
