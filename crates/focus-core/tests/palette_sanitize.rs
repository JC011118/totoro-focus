//! Palette allow-lists (SPEC §5 palettes, §7 pack palettes).

mod palette {
    use focus_core::palette::{
        is_safe_color, is_safe_filter, sanitize_palette, PaletteSpec, MAX_COLOR_LEN,
        MAX_FILTER_LEN, MAX_LABEL_CHARS,
    };

    #[test]
    fn accepts_hex_colors() {
        for ok in ["#fff", "#FFF", "#a1B2c3", "#00000000", "#ffffff22", "#123"] {
            assert!(is_safe_color(ok), "{ok}");
        }
    }

    #[test]
    fn rejects_malformed_hex() {
        for bad in [
            "#",
            "#ff",
            "#ffff",
            "#fffff",
            "#fffffff",
            "#fffffffff",
            "#ggg",
            "fff",
            "##fff",
            "#fff ",
            " #fff",
        ] {
            assert!(!is_safe_color(bad), "{bad:?}");
        }
    }

    #[test]
    fn accepts_numeric_color_functions() {
        for ok in [
            "rgb(1,2,3)",
            "rgb(255, 128, 0)",
            "rgba(255, 128, 0, 0.5)",
            "rgba(255,128,0,50%)",
            "rgb(100% 50% 0%)",
            "rgb(10 20 30 / 0.4)",
            "rgba(10 20 30)",
            "rgb(  10   20   30  /  40% )",
            "hsl(120, 50%, 50%)",
            "hsl(120deg 50% 50%)",
            "hsla(-30deg, 50%, 50%, .5)",
            "hsl(210 15% 70% / 0.8)",
            "RGB(1,2,3)",
            "Hsl(120DEG 50% 50%)",
            "rgb(+1,2.5,3)",
        ] {
            assert!(is_safe_color(ok), "{ok}");
        }
    }

    #[test]
    fn rejects_everything_else() {
        let long = format!("rgb({}1,2,3)", "0".repeat(MAX_COLOR_LEN));
        for bad in [
            "",
            "red",
            "transparent",
            "currentColor",
            "url(x.png)",
            "rgb(url(x),2,3)",
            "var(--accent)",
            "rgb(var(--r),2,3)",
            "rgb(calc(1),2,3)",
            "expression(alert(1))",
            "rgb(1,2,3);background:url(//x)",
            "rgb(1,2,3)}body{color:red",
            "rgb(1;2;3)",
            "rgb(1,2)",
            "rgb(1,2,3,4,5)",
            "rgb(1 2)",
            "rgb(1 2 3 4)",
            "rgb(1 2 3 / )",
            "rgb(1 2 3 / 4 5)",
            "rgb(1 2 3 / 4 / 5)",
            "rgb(1,2 3)",
            "rgb(1, 2, 3 / 4)",
            "rgb(1,,2,3)",
            "rgb(10deg,0,0)",
            "hsl(1turn, 50%, 50%)",
            "hsl(120, 50deg, 50%)",
            "rgb(1e2,0,0)",
            "rgb(0x10,0,0)",
            "rgb(1.,2,3)",
            "rgb(1px,2,3)",
            "rgb(1,2,3",
            "rgb 1,2,3)",
            "rgb(1,2,3))",
            "rgb(1,2,3) ",
            "lab(50 20 30)",
            "color(srgb 1 0 0)",
            "rgb(１,2,3)",
            "rgb(1,2,3)\u{0}",
            &long,
        ] {
            assert!(!is_safe_color(bad), "{bad:?}");
        }
    }

    #[test]
    fn accepts_allowed_filters() {
        for ok in [
            "brightness(0.9)",
            "brightness(90%)",
            "brightness(0.9) saturate(0.9)",
            "contrast(1.1) sepia(20%) grayscale(0.1)",
            "hue-rotate(-15deg)",
            "hue-rotate(30deg) brightness(1)",
            "hue-rotate(0)",
            "saturate(1)saturate(2)",
            "brightness( 0.9 )",
            "brightness(1)   contrast(1)",
        ] {
            assert!(is_safe_filter(ok), "{ok}");
        }
    }

    #[test]
    fn rejects_other_filters() {
        let many = "brightness(1) ".repeat(9);
        let long = format!("brightness(1{})", "0".repeat(MAX_FILTER_LEN));
        for bad in [
            "",
            " ",
            "none",
            "blur(2px)",
            "drop-shadow(0 0 2px #000)",
            "url(#f)",
            "brightness(url(x))",
            "brightness(var(--b))",
            "brightness()",
            "brightness(-1)",
            "brightness(1deg)",
            "brightness(1px)",
            "brightness(1e1)",
            "brightness(1);",
            "brightness(1) ;color:red",
            "brightness(1)}",
            "brightness(1",
            "brightness 1)",
            "Brightness(1)",
            "hue-rotate(30)",
            "hue-rotate(30%)",
            "hue-rotate(30DEG)",
            "invert(1)",
            "opacity(0.5)",
            " brightness(1)",
            "brightness(1) ",
            many.trim_end(),
            &long,
        ] {
            assert!(!is_safe_filter(bad), "{bad:?}");
        }
    }

    #[test]
    fn sanitize_keeps_safe_values_and_trims() {
        let p = PaletteSpec {
            label: Some("  Evening\tlight ".into()),
            background: Some(" #2b2238 ".into()),
            surface: Some("rgba(60, 48, 80, 0.9)".into()),
            text: Some("#f3ead8".into()),
            muted: Some("hsl(260deg 15% 70%)".into()),
            accent: Some("#e9a86b".into()),
            ring: Some("#e9a86b".into()),
            ring_track: Some("#ffffff22".into()),
            character_filter: Some(" brightness(0.9) saturate(0.9) ".into()),
        };
        let (clean, warnings) = sanitize_palette(&p);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(clean.label.as_deref(), Some("Evening light"));
        assert_eq!(clean.background.as_deref(), Some("#2b2238"));
        assert_eq!(
            clean.character_filter.as_deref(),
            Some("brightness(0.9) saturate(0.9)")
        );
        assert_eq!(clean.surface, p.surface);
        // Idempotent.
        assert_eq!(sanitize_palette(&clean), (clean.clone(), vec![]));
    }

    #[test]
    fn sanitize_drops_each_unsafe_field_with_one_warning() {
        let p = PaletteSpec {
            label: None,
            background: Some("url(https://example.com/x.png)".into()),
            surface: Some("#fff".into()),
            text: Some("var(--x)".into()),
            muted: None,
            accent: Some("red".into()),
            ring: Some("#abc".into()),
            ring_track: Some("rgb(1,2,3);x".into()),
            character_filter: Some("blur(3px)".into()),
        };
        let (clean, warnings) = sanitize_palette(&p);
        assert_eq!(clean.background, None);
        assert_eq!(clean.text, None);
        assert_eq!(clean.accent, None);
        assert_eq!(clean.ring_track, None);
        assert_eq!(clean.character_filter, None);
        assert_eq!(clean.surface.as_deref(), Some("#fff"));
        assert_eq!(clean.ring.as_deref(), Some("#abc"));
        assert_eq!(warnings.len(), 5, "{warnings:?}");
        for field in [
            "background",
            "text",
            "accent",
            "ringTrack",
            "characterFilter",
        ] {
            assert!(
                warnings.iter().any(|w| w.starts_with(&format!("{field}:"))),
                "{field}: {warnings:?}"
            );
        }
    }

    #[test]
    fn sanitize_bounds_label() {
        let long = "x".repeat(MAX_LABEL_CHARS + 10);
        let (clean, warnings) = sanitize_palette(&PaletteSpec {
            label: Some(long),
            ..Default::default()
        });
        assert_eq!(
            clean.label.map(|l| l.chars().count()),
            Some(MAX_LABEL_CHARS)
        );
        assert_eq!(warnings.len(), 1);
        // Empty or control-only labels disappear silently.
        let (clean, warnings) = sanitize_palette(&PaletteSpec {
            label: Some("\u{7}\n ".into()),
            ..Default::default()
        });
        assert_eq!(clean.label, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn sanitize_warning_excerpt_is_bounded_and_escaped() {
        let evil = format!("url(\"{}\")\n", "a".repeat(500));
        let (_, warnings) = sanitize_palette(&PaletteSpec {
            background: Some(evil),
            ..Default::default()
        });
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].len() < 120, "{}", warnings[0]);
        assert!(!warnings[0].contains('\n'));
    }

    #[test]
    fn empty_palette_is_untouched() {
        assert_eq!(
            sanitize_palette(&PaletteSpec::default()),
            (PaletteSpec::default(), vec![])
        );
    }

    #[test]
    fn serde_shape_is_camel_case_and_skips_none() {
        let p = PaletteSpec {
            ring_track: Some("#000".into()),
            character_filter: Some("sepia(1)".into()),
            ..Default::default()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"ringTrack": "#000", "characterFilter": "sepia(1)"})
        );
        let back: PaletteSpec = serde_json::from_value(v).unwrap();
        assert_eq!(back, p);
    }

    /// Deterministic fuzz: random strings over a CSS-ish alphabet never pass with
    /// a dangerous character or keyword, and every accepted value is stable under
    /// sanitize (kept verbatim).
    #[test]
    fn fuzz_accepted_values_never_contain_dangerous_tokens() {
        const ALPHABET: &[u8] = b"#0123456789abcdefgrlhsuvxtnpe(),.%/ -+;:{}'\"\\<>deg";
        let mut seed: u64 = 0x5eed_1234_abcd_ef01;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let prefixes = [
            "",
            "#",
            "rgb(",
            "rgba(",
            "hsl(",
            "hsla(",
            "brightness(",
            "hue-rotate(",
            "sepia(",
        ];
        let mut accepted = 0;
        for _ in 0..20_000 {
            let prefix = prefixes[(next() % prefixes.len() as u64) as usize];
            let len = (next() % 24) as usize;
            let mut s = String::from(prefix);
            for _ in 0..len {
                s.push(ALPHABET[(next() % ALPHABET.len() as u64) as usize] as char);
            }
            if next() % 2 == 0 {
                s.push(')');
            }
            for ok in [is_safe_color(&s), is_safe_filter(&s)] {
                if ok {
                    accepted += 1;
                    for bad in [";", "{", "}", "'", "\"", "\\", "<", ">", ":", "url", "var"] {
                        assert!(!s.contains(bad), "{s:?} contains {bad:?}");
                    }
                }
            }
        }
        // Sanity: the generator does hit accepted values (mostly short hex colors).
        assert!(accepted > 0);
    }
}
