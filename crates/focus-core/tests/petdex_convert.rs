//! Petdex / Codex pet conversion (SPEC §7.2). Sheets are synthetic headers or
//! the original fixture atlas; nothing is fetched.

mod petdex {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use focus_core::packs::petdex::{
        convert_petdex, image_dims, rows_for_dims, SheetInfo, NO_LICENSE_WARNING, PETDEX_MAPPING,
        ROW_IDLE, ROW_JUMPING, ROW_WAITING, ROW_WAVING,
    };
    use focus_core::packs::{
        validate_manifest, BundledPack, PackError, PackFormatKind, PackRegistry, PackSource,
        Rendering, MAX_ASSET_BYTES, PLACEHOLDER_ID,
    };
    use focus_core::rewards::{UnlockTable, BUILTIN_UNLOCKS_JSON};

    const V1: (u32, u32) = (1536, 1872);
    const V2: (u32, u32) = (1536, 2288);
    const CI_FIXTURE: &str = r#"{"id":"ci-pet","displayName":"CI Pet","description":"ci","spriteVersionNumber":2,"spritesheetPath":"spritesheet.png"}"#;

    // ---------------------------------------------------------------- image headers

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b
    }

    fn riff(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut b = b"RIFF\x00\x00\x00\x00WEBP".to_vec();
        b.extend_from_slice(fourcc);
        b.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        b.extend_from_slice(payload);
        b
    }

    fn webp_vp8x(w: u32, h: u32) -> Vec<u8> {
        let mut p = vec![0x10, 0, 0, 0];
        p.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
        p.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
        riff(b"VP8X", &p)
    }

    fn webp_vp8l(w: u32, h: u32) -> Vec<u8> {
        let bits = (w - 1) | ((h - 1) << 14);
        let mut p = vec![0x2f];
        p.extend_from_slice(&bits.to_le_bytes());
        p.extend_from_slice(&[0; 8]);
        riff(b"VP8L", &p)
    }

    fn webp_vp8(w: u16, h: u16) -> Vec<u8> {
        let mut p = vec![0x30, 0x01, 0x00, 0x9d, 0x01, 0x2a];
        p.extend_from_slice(&w.to_le_bytes());
        p.extend_from_slice(&h.to_le_bytes());
        p.extend_from_slice(&[0; 8]);
        riff(b"VP8 ", &p)
    }

    fn gif(w: u16, h: u16) -> Vec<u8> {
        let mut b = b"GIF89a".to_vec();
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b
    }

    // ---------------------------------------------------------------- helpers

    /// (file name, size in bytes, readable dimensions)
    type Sheet<'a> = (&'a str, u64, Option<(u32, u32)>);

    fn sheets(files: &[Sheet]) -> BTreeMap<String, SheetInfo> {
        files
            .iter()
            .map(|(n, len, dims)| {
                (
                    n.to_string(),
                    SheetInfo {
                        len: *len,
                        dims: *dims,
                    },
                )
            })
            .collect()
    }

    fn convert(
        json: &str,
        dir: &str,
        files: &BTreeMap<String, SheetInfo>,
    ) -> Result<focus_core::packs::petdex::PetdexConversion, PackError> {
        convert_petdex(json, dir, &|rel: &str| files.get(rel).copied())
    }

    fn has(list: &[String], needle: &str) -> bool {
        list.iter().any(|s| s.contains(needle))
    }

    fn table() -> UnlockTable {
        serde_json::from_str(BUILTIN_UNLOCKS_JSON).expect("data/unlocks.json deserializes")
    }

    fn placeholder_bundle() -> BundledPack {
        let json = r#"{"format":1,"id":"placeholder","name":"Pip","license":"CC0-1.0",
            "animations":{"idle":{"sheet":"idle.svg","frameWidth":96,"frameHeight":96,"frames":1,"fps":1}}}"#;
        BundledPack {
            files: vec![
                ("pack.json".into(), json.as_bytes()),
                ("idle.svg".into(), b"<svg/>"),
            ],
        }
    }

    fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let path = path.as_ref();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn pet_dir(parent: &Path, folder: &str, pet_json: &str, sheet: (&str, Vec<u8>)) -> PathBuf {
        let root = parent.join(folder);
        write(root.join("pet.json"), pet_json);
        write(root.join(sheet.0), sheet.1);
        root
    }

    // ---------------------------------------------------------------- image_dims

    #[test]
    fn image_dims_reads_headers() {
        assert_eq!(image_dims(&png(1536, 1872)), Some(V1));
        assert_eq!(image_dims(&webp_vp8x(1536, 2288)), Some(V2));
        assert_eq!(image_dims(&webp_vp8l(1536, 1872)), Some(V1));
        assert_eq!(image_dims(&webp_vp8(1536, 2288)), Some(V2));
        assert_eq!(
            image_dims(&webp_vp8(0x4000 | 192, 234)),
            Some((192, 234)),
            "VP8 scale bits are masked"
        );
        assert_eq!(image_dims(&gif(192, 234)), Some((192, 234)));
        // The extremes of each encoding.
        assert_eq!(image_dims(&webp_vp8x(1 << 24, 1)), Some((1 << 24, 1)));
        assert_eq!(image_dims(&webp_vp8l(16384, 16384)), Some((16384, 16384)));
    }

    #[test]
    fn image_dims_rejects_garbage() {
        assert_eq!(image_dims(b""), None);
        assert_eq!(
            image_dims(b"<svg xmlns='http://www.w3.org/2000/svg'/>"),
            None
        );
        assert_eq!(
            image_dims(&[0xff, 0xd8, 0xff, 0xe0, 0, 0x10]),
            None,
            "JPEG is not parsed"
        );
        assert_eq!(image_dims(&png(1536, 1872)[..20]), None, "truncated");
        assert_eq!(image_dims(&png(0, 10)), None, "zero width");
        let mut bad_ihdr = png(10, 10);
        bad_ihdr[12..16].copy_from_slice(b"IDAT");
        assert_eq!(image_dims(&bad_ihdr), None);
        let mut bad_vp8l = webp_vp8l(10, 10);
        bad_vp8l[20] = 0;
        assert_eq!(image_dims(&bad_vp8l), None);
        let mut bad_vp8 = webp_vp8(10, 10);
        bad_vp8[23] = 0;
        assert_eq!(image_dims(&bad_vp8), None);
        assert_eq!(image_dims(&riff(b"ALPH", &[0; 20])), None);
        assert_eq!(image_dims(&webp_vp8x(10, 10)[..28]), None, "truncated VP8X");
    }

    #[test]
    fn rows_from_dimensions() {
        assert_eq!(rows_for_dims(1536, 1872), Ok(9));
        assert_eq!(rows_for_dims(1536, 2288), Ok(11));
        assert_eq!(rows_for_dims(768, 936), Ok(9), "half scale");
        assert_eq!(rows_for_dims(192, 234), Ok(9), "eighth scale");
        assert_eq!(rows_for_dims(3072, 4576), Ok(11), "double scale");
        for (w, h) in [
            (1500, 1872),
            (1536, 1800),
            (1536, 2000),
            (1536, 1536),
            (0, 0),
            (8, 9),
            (1536, 1873),
        ] {
            let e = rows_for_dims(w, h).unwrap_err();
            assert!(e.contains(&format!("{w}×{h}")), "{e}");
        }
    }

    // ---------------------------------------------------------------- conversion

    #[test]
    fn converts_the_petdex_ci_fixture() {
        let files = sheets(&[("spritesheet.png", 100_000, Some(V2))]);
        let conv = convert(CI_FIXTURE, "ci-pet", &files).unwrap();
        let m = &conv.manifest;
        assert_eq!(
            (m.format, m.id.as_str(), m.name.as_str()),
            (1, "ci-pet", "CI Pet")
        );
        assert_eq!(m.description.as_deref(), Some("ci"));
        assert_eq!((m.author.clone(), m.license.clone()), (None, None));
        assert_eq!(m.rendering, Rendering::Smooth);
        assert!(m.fallbacks.is_empty() && m.palettes.is_empty());
        assert_eq!(conv.warnings, vec![NO_LICENSE_WARNING.to_string()]);
        for a in m.animations.values() {
            assert_eq!(a.sheet, "spritesheet.png");
            assert_eq!((a.frame_width, a.frame_height), (192, 208));
            assert_eq!(
                (a.sheet_columns, a.sheet_rows, a.column),
                (Some(8), Some(11), 0)
            );
        }
        let (errors, warnings) = validate_manifest(m, &|p| files.contains_key(p));
        assert!(
            errors.is_empty() && warnings.is_empty(),
            "{errors:?} {warnings:?}"
        );
    }

    #[test]
    fn row_mapping_matches_the_table() {
        let files = sheets(&[("spritesheet.webp", 10, Some(V1))]);
        let m = convert(r#"{"displayName":"P"}"#, "p", &files)
            .unwrap()
            .manifest;
        let expect = [
            ("idle", ROW_IDLE, true),
            ("focus", ROW_IDLE, true),
            ("break", ROW_IDLE, true),
            ("paused", ROW_WAITING, true),
            ("completed", ROW_JUMPING, false),
            ("react", ROW_WAVING, false),
            ("wave", ROW_WAVING, false),
            ("celebrate", ROW_JUMPING, false),
            ("long_session", ROW_JUMPING, false),
        ];
        assert_eq!(m.animations.len(), expect.len());
        assert_eq!(PETDEX_MAPPING.len(), expect.len());
        for (key, row, looping) in expect {
            let a = &m.animations[key];
            assert_eq!(a.row, row.row, "{key}");
            assert_eq!(a.looping, looping, "{key}");
            assert_eq!(a.frames as usize, row.durations_ms.len(), "{key}");
            assert_eq!(
                a.frame_durations_ms.as_deref(),
                Some(row.durations_ms),
                "{key}"
            );
            assert!(
                (a.fps - a.frames as f32 * 1000.0 / row.durations_ms.iter().sum::<u32>() as f32)
                    .abs()
                    < 1e-4
            );
            assert_eq!(
                a.still_frame,
                Some(if looping { 0 } else { a.frames - 1 }),
                "{key}"
            );
            assert_eq!(a.sheet_rows, Some(9));
        }
        assert_eq!(
            m.animations["idle"].frame_durations_ms,
            Some(vec![280, 110, 110, 140, 140, 320])
        );
        assert_eq!(
            m.animations["paused"].frame_durations_ms,
            Some(vec![150, 150, 150, 150, 150, 260])
        );
        // Never the failure row, locomotion, review, or v2 look rows.
        assert!(m.animations.values().all(|a| [0, 3, 4, 6].contains(&a.row)));
        for k in [
            "stretch",
            "umbrella",
            "sleepy",
            "acorn_gift",
            "firefly",
            "hum",
            "lantern",
            "stargaze",
            "blink",
        ] {
            assert!(!m.animations.contains_key(k), "{k} must stay unmapped");
        }
    }

    #[test]
    fn sprite_version_rules() {
        let v1 = sheets(&[("spritesheet.png", 10, Some(V1))]);
        assert_eq!(
            convert("{}", "a", &v1).unwrap().manifest.animations["idle"].sheet_rows,
            Some(9)
        );
        assert_eq!(
            convert(r#"{"spriteVersionNumber":null}"#, "a", &v1)
                .unwrap()
                .manifest
                .animations["idle"]
                .sheet_rows,
            Some(9)
        );
        for bad in ["3", "0", "\"2\"", "1.5", "true", "-1", "[2]"] {
            let json = format!(r#"{{"spriteVersionNumber":{bad}}}"#);
            match convert(&json, "a", &v1) {
                Err(PackError::Invalid(e)) => assert!(e.contains("spriteVersionNumber"), "{e}"),
                other => panic!("{bad}: {other:?}"),
            }
        }
    }

    #[test]
    fn dimensions_win_over_the_version_number() {
        let files = sheets(&[("spritesheet.png", 10, Some(V1))]);
        let conv = convert(r#"{"spriteVersionNumber":2}"#, "a", &files).unwrap();
        assert_eq!(conv.manifest.animations["idle"].sheet_rows, Some(9));
        assert!(
            has(
                &conv.warnings,
                "spritesheet has 9 rows but spriteVersionNumber is 2"
            ),
            "{:?}",
            conv.warnings
        );
        let files = sheets(&[("spritesheet.png", 10, Some(V2))]);
        let conv = convert(r#"{"spriteVersionNumber":1}"#, "a", &files).unwrap();
        assert_eq!(conv.manifest.animations["idle"].sheet_rows, Some(11));
        assert!(has(&conv.warnings, "has 11 rows"));
    }

    #[test]
    fn unreadable_dimensions_trust_the_version_number() {
        let files = sheets(&[("spritesheet.png", 10, None)]);
        for (v, rows) in [("1", 9), ("2", 11)] {
            let conv = convert(&format!(r#"{{"spriteVersionNumber":{v}}}"#), "a", &files).unwrap();
            assert_eq!(conv.manifest.animations["paused"].sheet_rows, Some(rows));
            assert!(
                has(&conv.warnings, "could not read the spritesheet size"),
                "{:?}",
                conv.warnings
            );
        }
    }

    #[test]
    fn bad_grids_are_errors_and_scaled_grids_are_fine() {
        let bad = sheets(&[("spritesheet.png", 10, Some((1500, 1872)))]);
        assert!(
            matches!(convert("{}", "a", &bad), Err(PackError::Invalid(e)) if e.contains("8 columns"))
        );
        let scaled = sheets(&[("spritesheet.png", 10, Some((768, 1144)))]);
        let m = convert("{}", "a", &scaled).unwrap().manifest;
        assert_eq!(m.animations["idle"].sheet_rows, Some(11));
        assert_eq!(
            (
                m.animations["idle"].frame_width,
                m.animations["idle"].frame_height
            ),
            (192, 208)
        );
    }

    #[test]
    fn spritesheet_path_and_fallbacks() {
        // Declared and present: used.
        let files = sheets(&[
            ("custom.webp", 10, Some(V1)),
            ("spritesheet.webp", 10, Some(V1)),
        ]);
        let conv = convert(r#"{"spritesheetPath":"custom.webp"}"#, "a", &files).unwrap();
        assert_eq!(conv.manifest.animations["idle"].sheet, "custom.webp");
        assert_eq!(conv.warnings, vec![NO_LICENSE_WARNING.to_string()]);
        // Absent: spritesheet.webp, then spritesheet.png.
        let both = sheets(&[
            ("spritesheet.webp", 10, Some(V1)),
            ("spritesheet.png", 10, Some(V1)),
        ]);
        assert_eq!(
            convert("{}", "a", &both).unwrap().manifest.animations["idle"].sheet,
            "spritesheet.webp"
        );
        let png_only = sheets(&[("spritesheet.png", 10, Some(V1))]);
        assert_eq!(
            convert("{}", "a", &png_only).unwrap().manifest.animations["idle"].sheet,
            "spritesheet.png"
        );
        // Declared but missing: falls back with a warning.
        let conv = convert(r#"{"spritesheetPath":"gone.png"}"#, "a", &png_only).unwrap();
        assert_eq!(conv.manifest.animations["idle"].sheet, "spritesheet.png");
        assert!(
            has(&conv.warnings, "\"gone.png\" was not found"),
            "{:?}",
            conv.warnings
        );
        // Declared but not a plain raster file name: ignored with a warning.
        for p in [
            "../x.png",
            "sub/x.png",
            "/abs.png",
            ".x.png",
            "x.svg",
            "x y.png",
            "x.json",
        ] {
            let json = serde_json::json!({ "spritesheetPath": p }).to_string();
            let conv = convert(&json, "a", &png_only).unwrap();
            assert_eq!(
                conv.manifest.animations["idle"].sheet, "spritesheet.png",
                "{p}"
            );
            assert!(
                has(&conv.warnings, "is not a plain"),
                "{p}: {:?}",
                conv.warnings
            );
        }
        let conv = convert(r#"{"spritesheetPath":42}"#, "a", &png_only).unwrap();
        assert!(has(&conv.warnings, "is not a string"));
        // Nothing found.
        let none = sheets(&[]);
        match convert(r#"{"spritesheetPath":"mine.png"}"#, "a", &none) {
            Err(PackError::Invalid(e)) => {
                assert!(
                    e.contains("mine.png")
                        && e.contains("spritesheet.webp")
                        && e.contains("spritesheet.png"),
                    "{e}"
                )
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn oversized_sheets_are_errors() {
        let files = sheets(&[("spritesheet.png", MAX_ASSET_BYTES + 1, Some(V1))]);
        assert!(
            matches!(convert("{}", "a", &files), Err(PackError::Invalid(e)) if e.contains("larger than 16 MiB"))
        );
        let files = sheets(&[("spritesheet.png", MAX_ASSET_BYTES, Some(V1))]);
        assert!(convert("{}", "a", &files).is_ok());
    }

    #[test]
    fn ids_come_from_the_folder_then_pet_json() {
        let files = sheets(&[("spritesheet.png", 10, Some(V1))]);
        let conv = convert(r#"{"id":"Other"}"#, "boba-cat", &files).unwrap();
        assert_eq!(conv.manifest.id, "boba-cat");
        let conv = convert(r#"{"id":"Cool_Pet 2"}"#, "My Pet", &files).unwrap();
        assert_eq!(conv.manifest.id, "cool_pet-2");
        assert!(
            has(
                &conv.warnings,
                "folder name \"My Pet\" is not a valid pack id"
            ),
            "{:?}",
            conv.warnings
        );
        let conv = convert(r#"{"id":"!!!"}"#, "My Pet", &files).unwrap();
        assert_eq!(
            conv.manifest.id, "my-pet",
            "sanitized folder name as a last resort"
        );
        assert_eq!(
            convert("{}", "!!!", &files),
            Err(PackError::InvalidId("!!!".into()))
        );
        assert_eq!(
            convert(r#"{"id":42}"#, "ÄÖ", &files),
            Err(PackError::InvalidId("ÄÖ".into()))
        );
    }

    #[test]
    fn names_and_descriptions_are_cleaned() {
        let files = sheets(&[("spritesheet.png", 10, Some(V1))]);
        let m = convert("{}", "nameless", &files).unwrap().manifest;
        assert_eq!((m.name.as_str(), m.description.clone()), ("nameless", None));
        let json = serde_json::json!({"displayName": "  Boba\u{0}\n Cat ", "description": "x".repeat(900)}).to_string();
        let m = convert(&json, "boba", &files).unwrap().manifest;
        assert_eq!(m.name, "Boba Cat");
        assert_eq!(m.description.map(|d| d.chars().count()), Some(500));
        let m = convert(r#"{"displayName":"   "}"#, "blank", &files)
            .unwrap()
            .manifest;
        assert_eq!(m.name, "blank");
    }

    #[test]
    fn malformed_pet_json() {
        let files = sheets(&[("spritesheet.png", 10, Some(V1))]);
        assert!(matches!(
            convert("{nope", "a", &files),
            Err(PackError::Json(_))
        ));
        assert!(matches!(
            convert("[]", "a", &files),
            Err(PackError::Invalid(_))
        ));
        let big = format!("{{\"description\":\"{}\"}}", "x".repeat(300 * 1024));
        assert!(
            matches!(convert(&big, "a", &files), Err(PackError::Invalid(e)) if e.contains("larger than"))
        );
    }

    // ---------------------------------------------------------------- registry

    #[test]
    fn registry_loads_a_petdex_folder() {
        let user = tempfile::tempdir().unwrap();
        pet_dir(
            user.path(),
            "boba",
            CI_FIXTURE,
            ("spritesheet.png", {
                let mut b = png(V2.0, V2.1);
                b.extend_from_slice(&[0; 64]);
                b
            }),
        );
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let p = r
            .info("boba")
            .cloned()
            .unwrap_or_else(|| panic!("{:?}", r.list()));
        assert!(p.valid, "{p:?}");
        assert_eq!(
            (p.source, p.format_kind),
            (PackSource::User, PackFormatKind::Petdex)
        );
        assert_eq!(
            (p.name.as_str(), p.author.clone(), p.license.clone()),
            ("CI Pet", None, None)
        );
        assert!(
            p.warnings.iter().any(|w| w == NO_LICENSE_WARNING),
            "{:?}",
            p.warnings
        );
        assert!(
            !p.warnings.iter().any(|w| w.contains("pack.json")),
            "{:?}",
            p.warnings
        );

        let rp = r.resolve_pack("boba", &table(), "pack://localhost/");
        assert_eq!(rp.fallback_from, None);
        assert_eq!(rp.rendering, Rendering::Smooth);
        let paused = rp.animations["paused"].as_ref().unwrap();
        assert_eq!(
            (
                paused.source_key.as_str(),
                paused.row,
                paused.sheet_columns,
                paused.sheet_rows
            ),
            ("paused", 6, 8, 11)
        );
        assert_eq!(paused.url, "pack://localhost/boba/spritesheet.png");
        assert_eq!(
            paused.frame_durations_ms.as_deref(),
            Some(ROW_WAITING.durations_ms)
        );
        let completed = rp.animations["completed"].as_ref().unwrap();
        assert!(!completed.looping);
        assert_eq!(completed.still_frame, 4);
        for k in [
            "stretch",
            "sleepy",
            "umbrella",
            "acorn_gift",
            "hum",
            "blink",
        ] {
            assert_eq!(rp.animations[k], None, "{k}");
        }
        assert!(rp.animations["wave"].is_some() && rp.animations["react"].is_some());
        assert_eq!(
            r.read_file("boba", "spritesheet.png").map(|(_, m)| m),
            Some("image/png")
        );
        assert_eq!(
            r.read_file("boba", "pet.json").map(|(_, m)| m),
            Some("application/json")
        );
    }

    #[test]
    fn registry_loads_the_fixture_pet() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/petdex");
        let r = PackRegistry::load(placeholder_bundle(), None, Some(&fixtures));
        let p = r
            .info("fixture-pet")
            .cloned()
            .unwrap_or_else(|| panic!("{:?}", r.list()));
        assert!(p.valid, "{p:?}");
        assert_eq!(
            (p.source, p.format_kind),
            (PackSource::Dev, PackFormatKind::Petdex)
        );
        assert_eq!(
            p.warnings,
            vec![NO_LICENSE_WARNING.to_string()],
            "real PNG header is read: no size warning"
        );
        let rp = r.resolve_pack("fixture-pet", &table(), "pack://localhost/");
        let idle = rp.animations["idle"].as_ref().unwrap();
        assert_eq!(
            (idle.sheet_columns, idle.sheet_rows, idle.frames),
            (8, 9, 6)
        );
        let (bytes, mime) = r.read_file("fixture-pet", "spritesheet.png").unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(image_dims(&bytes), Some((192, 234)));
    }

    #[test]
    fn registry_petdex_edge_cases() {
        let user = tempfile::tempdir().unwrap();
        // pack.json wins over pet.json in the same folder.
        let both = pet_dir(
            user.path(),
            "both",
            CI_FIXTURE,
            ("spritesheet.png", png(V2.0, V2.1)),
        );
        write(
            both.join("pack.json"),
            r#"{"format":1,"id":"both","name":"Native both","license":"MIT",
                "animations":{"idle":{"sheet":"spritesheet.png","frameWidth":192,"frameHeight":208,"frames":6,"fps":5,"sheetColumns":8,"sheetRows":11}}}"#,
        );
        // A native pack claiming a Petdex folder's id wins the id.
        pet_dir(
            user.path(),
            "taken",
            "{}",
            ("spritesheet.png", png(V1.0, V1.1)),
        );
        let native = user.path().join("native-taken");
        write(
            native.join("pack.json"),
            r#"{"format":1,"id":"taken","name":"Native taken","license":"MIT",
                "animations":{"idle":{"sheet":"i.png","frameWidth":1,"frameHeight":1,"frames":1,"fps":1}}}"#,
        );
        write(native.join("i.png"), png(1, 1));
        // A Petdex folder called "placeholder" is rejected.
        pet_dir(
            user.path(),
            PLACEHOLDER_ID,
            "{}",
            ("spritesheet.png", png(V1.0, V1.1)),
        );
        // Broken pet.json and a bad grid are listed with errors.
        pet_dir(
            user.path(),
            "broken",
            "{",
            ("spritesheet.png", png(V1.0, V1.1)),
        );
        pet_dir(
            user.path(),
            "badgrid",
            "{}",
            ("spritesheet.png", png(1000, 1000)),
        );
        pet_dir(user.path(), "nosheet", "{}", ("other.png", png(V1.0, V1.1)));

        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let info = |id: &str| {
            r.info(id)
                .cloned()
                .unwrap_or_else(|| panic!("{id} not in {:?}", r.list()))
        };
        let ids: Vec<String> = r.list().into_iter().map(|i| i.id).collect();
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");

        let both = info("both");
        assert_eq!(
            (both.format_kind, both.name.as_str(), both.valid),
            (PackFormatKind::Native, "Native both", true)
        );
        let taken = info("taken");
        assert_eq!(
            (taken.format_kind, taken.name.as_str()),
            (PackFormatKind::Native, "Native taken")
        );
        let loser = info("user:taken");
        assert_eq!(loser.format_kind, PackFormatKind::Petdex);
        assert!(
            !loser.valid && loser.errors.iter().any(|e| e.contains("duplicate pack id")),
            "{loser:?}"
        );
        let reserved = info("user:placeholder");
        assert!(
            reserved.errors.iter().any(|e| e.contains("reserved")),
            "{reserved:?}"
        );
        assert_eq!(info(PLACEHOLDER_ID).source, PackSource::Bundled);
        let broken = info("broken");
        assert_eq!(broken.format_kind, PackFormatKind::Petdex);
        assert!(
            broken.errors[0].starts_with("pet.json: manifest is not valid JSON"),
            "{broken:?}"
        );
        assert!(info("badgrid").errors[0].contains("8 columns"));
        assert!(info("nosheet").errors[0].contains("spritesheet not found"));
    }

    #[cfg(unix)]
    #[test]
    fn registry_refuses_petdex_sheets_linked_from_outside() {
        use std::os::unix::fs::symlink;
        let user = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path().join("spritesheet.png"), png(V1.0, V1.1));
        let root = user.path().join("linked");
        write(root.join("pet.json"), "{}");
        symlink(
            outside.path().join("spritesheet.png"),
            root.join("spritesheet.png"),
        )
        .unwrap();
        let r = PackRegistry::load(placeholder_bundle(), Some(user.path()), None);
        let p = r.info("linked").cloned().unwrap();
        assert!(
            !p.valid && p.errors[0].contains("spritesheet not found"),
            "{p:?}"
        );
    }
}
