//! Palette overrides shared by unlock defaults (`data/unlocks.json`) and packs.
//!
//! Palette values end up as CSS custom properties and a CSS `filter` in the
//! renderer, so they are checked against strict allow-lists here: no `url()`,
//! no `var()`, no `expression()`, no semicolons or braces, bounded lengths.
//! Anything outside the grammar is dropped (never "repaired").

use serde::{Deserialize, Serialize};

/// Max length of a color value, in bytes.
pub const MAX_COLOR_LEN: usize = 64;
/// Max length of a `characterFilter` value, in bytes.
pub const MAX_FILTER_LEN: usize = 160;
/// Max number of functions in a `characterFilter` list.
pub const MAX_FILTER_FUNCTIONS: usize = 8;
/// Max length of a palette `label`, in Unicode scalar values (longer labels are truncated).
pub const MAX_LABEL_CHARS: usize = 48;
/// Max length of one numeric token (sign, digits and decimal point) in a color or filter.
const MAX_NUMBER_LEN: usize = 12;

/// Colors must be `#rgb`, `#rrggbb`, `#rrggbbaa`, or `rgb()/rgba()/hsl()/hsla()`
/// with numeric arguments. `characterFilter` may only use
/// `brightness|contrast|saturate|sepia|hue-rotate|grayscale` with numeric
/// (`deg`/`%`/plain) arguments. Invalid values are dropped with a warning.
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PaletteSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub muted: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub accent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ring: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ring_track: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub character_filter: Option<String>,
}

/// True if `v` is an allowed CSS color (see `PaletteSpec`).
///
/// Accepted, with no surrounding whitespace and at most [`MAX_COLOR_LEN`] bytes:
/// - `#rgb`, `#rrggbb`, `#rrggbbaa` (hex digits, any case);
/// - `rgb()`, `rgba()`, `hsl()`, `hsla()` (function name in any ASCII case) with
///   either the comma form `a, b, c[, alpha]` or the space form `a b c[ / alpha]`.
///   Arguments are plain decimal numbers (optional sign, no exponent) with an
///   optional `%`; the hue of `hsl()/hsla()` may instead carry `deg`.
pub fn is_safe_color(v: &str) -> bool {
    if v.is_empty() || v.len() > MAX_COLOR_LEN || !v.is_ascii() {
        return false;
    }
    if let Some(hex) = v.strip_prefix('#') {
        return matches!(hex.len(), 3 | 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    let Some(open) = v.find('(') else {
        return false;
    };
    let name = v[..open].to_ascii_lowercase();
    let Some(inner) = v[open + 1..].strip_suffix(')') else {
        return false;
    };
    let hue_first = match name.as_str() {
        "rgb" | "rgba" => false,
        "hsl" | "hsla" => true,
        _ => return false,
    };
    let Some(args) = split_color_args(inner) else {
        return false;
    };
    args.iter().enumerate().all(|(i, arg)| {
        let (num, unit) = split_unit(arg);
        let unit_ok = match unit {
            "" | "%" => true,
            "deg" => hue_first && i == 0,
            _ => false,
        };
        unit_ok && is_number(num, true)
    })
}

/// Split color function arguments into 3 or 4 tokens, accepting either the comma
/// form (`a, b, c[, d]`) or the space form (`a b c[ / d]`), never a mix.
fn split_color_args(inner: &str) -> Option<Vec<&str>> {
    if inner.contains(',') {
        if inner.contains('/') {
            return None;
        }
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if !(3..=4).contains(&parts.len())
            || parts
                .iter()
                .any(|p| p.is_empty() || p.contains(char::is_whitespace))
        {
            return None;
        }
        return Some(parts);
    }
    let mut halves = inner.split('/');
    let main = halves.next()?;
    let alpha = halves.next();
    if halves.next().is_some() {
        return None;
    }
    let mut parts: Vec<&str> = main.split_whitespace().collect();
    if parts.len() != 3 {
        return None;
    }
    if let Some(alpha) = alpha {
        let a: Vec<&str> = alpha.split_whitespace().collect();
        if a.len() != 1 {
            return None;
        }
        parts.push(a[0]);
    }
    Some(parts)
}

/// Split a token into its numeric part and a trailing `%` or `deg` unit.
fn split_unit(token: &str) -> (&str, &str) {
    if let Some(n) = token.strip_suffix('%') {
        (n, "%")
    } else if token.len() > 3 && token[token.len() - 3..].eq_ignore_ascii_case("deg") {
        (&token[..token.len() - 3], "deg")
    } else {
        // Anything else that is not a plain number fails `is_number` later.
        let unit_start = token
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
            .unwrap_or(token.len());
        (&token[..unit_start], &token[unit_start..])
    }
}

/// A plain decimal number: optional sign (only if `signed`), digits with an
/// optional fraction, or a bare fraction (`.5`). No exponent, bounded length.
fn is_number(s: &str, signed: bool) -> bool {
    if s.is_empty() || s.len() > MAX_NUMBER_LEN {
        return false;
    }
    let body = match s.as_bytes()[0] {
        b'-' | b'+' if signed => &s[1..],
        b'-' | b'+' => return false,
        _ => s,
    };
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    match frac {
        None => !int.is_empty() && digits(int),
        Some(f) => !f.is_empty() && digits(f) && digits(int),
    }
}

/// True if `v` is an allowed CSS filter list (see `PaletteSpec`).
///
/// Accepted: 1 to [`MAX_FILTER_FUNCTIONS`] functions, at most [`MAX_FILTER_LEN`]
/// bytes, separated by optional whitespace. Each is one of `brightness`,
/// `contrast`, `saturate`, `sepia`, `grayscale` (a non-negative number with an
/// optional `%`) or `hue-rotate` (a signed number with `deg`, or a plain `0`).
/// Arguments may not be empty. Use `brightness(1)` for "no filter".
pub fn is_safe_filter(v: &str) -> bool {
    if v.is_empty() || v.len() > MAX_FILTER_LEN || !v.is_ascii() {
        return false;
    }
    let mut rest = v.trim_start();
    if rest.len() != v.len() {
        return false; // no leading whitespace
    }
    let mut count = 0;
    while !rest.is_empty() {
        let Some(open) = rest.find('(') else {
            return false;
        };
        let name = &rest[..open];
        let Some(close_rel) = rest[open + 1..].find(')') else {
            return false;
        };
        let arg = rest[open + 1..open + 1 + close_rel].trim();
        let ok = match name {
            "brightness" | "contrast" | "saturate" | "sepia" | "grayscale" => {
                let n = arg.strip_suffix('%').unwrap_or(arg);
                is_number(n, false)
            }
            "hue-rotate" => match arg.strip_suffix("deg") {
                Some(n) => is_number(n, true),
                None => is_number(arg, false) && arg.bytes().all(|b| b == b'0' || b == b'.'),
            },
            _ => false,
        };
        if !ok {
            return false;
        }
        count += 1;
        if count > MAX_FILTER_FUNCTIONS {
            return false;
        }
        rest = &rest[open + 1 + close_rel + 1..];
        let trimmed = rest.trim_start();
        if trimmed.is_empty() && !rest.is_empty() {
            return false; // no trailing whitespace
        }
        rest = trimmed;
    }
    count > 0
}

/// Normalize free display text: control characters become spaces, whitespace
/// runs collapse to one space, the result is trimmed and cut to `max` scalar
/// values. Returns `(text, truncated)`; empty text is `None`.
pub(crate) fn clean_text(s: &str, max: usize) -> (Option<String>, bool) {
    let mut out = String::with_capacity(s.len().min(max * 4));
    let mut pending_space = false;
    let mut n = 0;
    let mut truncated = false;
    for c in s.chars() {
        if c.is_control() || c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        let needed = if pending_space { 2 } else { 1 };
        if n + needed > max {
            truncated = true;
            break;
        }
        if pending_space {
            out.push(' ');
            n += 1;
            pending_space = false;
        }
        out.push(c);
        n += 1;
    }
    ((!out.is_empty()).then_some(out), truncated)
}

/// Short, escaped excerpt of a rejected value for warnings.
fn excerpt(v: &str) -> String {
    const MAX: usize = 40;
    let mut s: String = v.chars().take(MAX).collect();
    if v.chars().count() > MAX {
        s.push('…');
    }
    format!("{s:?}")
}

/// Drop unsafe values; returns the cleaned palette and one warning per dropped field.
///
/// Colors and the filter are trimmed and then checked with [`is_safe_color`] /
/// [`is_safe_filter`]. The label is display text: control characters and
/// whitespace runs are normalized, it is cut to [`MAX_LABEL_CHARS`] (with a
/// warning), and an empty label is dropped silently.
pub fn sanitize_palette(p: &PaletteSpec) -> (PaletteSpec, Vec<String>) {
    let mut warnings = Vec::new();
    let mut color = |field: &str, v: &Option<String>| -> Option<String> {
        let v = v.as_deref()?;
        let t = v.trim();
        if is_safe_color(t) {
            Some(t.to_string())
        } else {
            warnings.push(format!("{field}: dropped unsupported color {}", excerpt(v)));
            None
        }
    };
    let mut out = PaletteSpec {
        label: None,
        background: color("background", &p.background),
        surface: color("surface", &p.surface),
        text: color("text", &p.text),
        muted: color("muted", &p.muted),
        accent: color("accent", &p.accent),
        ring: color("ring", &p.ring),
        ring_track: color("ringTrack", &p.ring_track),
        character_filter: None,
    };
    if let Some(label) = p.label.as_deref() {
        let (clean, truncated) = clean_text(label, MAX_LABEL_CHARS);
        if truncated {
            warnings.push(format!("label: truncated to {MAX_LABEL_CHARS} characters"));
        }
        out.label = clean;
    }
    if let Some(f) = p.character_filter.as_deref() {
        let t = f.trim();
        if is_safe_filter(t) {
            out.character_filter = Some(t.to_string());
        } else {
            warnings.push(format!(
                "characterFilter: dropped unsupported filter {}",
                excerpt(f)
            ));
        }
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        for ok in ["0", "1", "255", "0.5", ".5", "10.25"] {
            assert!(is_number(ok, false), "{ok}");
        }
        for ok in ["-1", "+1", "-0.5", "-.5"] {
            assert!(is_number(ok, true), "{ok}");
            assert!(!is_number(ok, false), "{ok} unsigned");
        }
        for bad in [
            "",
            ".",
            "-",
            "1.",
            "1e3",
            "1..2",
            "0x10",
            "1,0",
            " 1",
            "1234567890123",
        ] {
            assert!(!is_number(bad, true), "{bad}");
        }
    }

    #[test]
    fn clean_text_normalizes_and_truncates() {
        assert_eq!(
            clean_text("  a\u{0}\tb \n c  ", 10),
            (Some("a b c".into()), false)
        );
        assert_eq!(clean_text("   ", 10), (None, false));
        assert_eq!(clean_text("abcdef", 3), (Some("abc".into()), true));
        // A space that would only precede the cut is not kept.
        assert_eq!(clean_text("abc def", 4), (Some("abc".into()), true));
        assert_eq!(clean_text("ééé", 2), (Some("éé".into()), true));
    }

    #[test]
    fn split_unit_cases() {
        assert_eq!(split_unit("50%"), ("50", "%"));
        assert_eq!(split_unit("120deg"), ("120", "deg"));
        assert_eq!(split_unit("120DEG"), ("120", "deg"));
        assert_eq!(split_unit("1px"), ("1", "px"));
        assert_eq!(split_unit("12"), ("12", ""));
    }
}
