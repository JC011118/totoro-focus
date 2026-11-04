//! Pack-relative path safety, served MIME types and URL encoding.

use std::path::{Path, PathBuf};

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use super::{MAX_PATH_DEPTH, MAX_PATH_LEN};

/// Everything except RFC 3986 unreserved characters (`A-Z a-z 0-9 - . _ ~`) is
/// percent-encoded in a URL path segment.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Normalize a pack-relative path. Rejects empty, absolute, drive-prefixed,
/// backslash, `..`, NUL, and hidden (`.`-prefixed) components.
///
/// Also rejected: empty components (`a//b`, trailing `/`), any control
/// character, `:` anywhere (drive prefixes, alternate data streams), paths longer
/// than `MAX_PATH_LEN` bytes or deeper than `MAX_PATH_DEPTH` components. A path
/// that passes is returned unchanged as a `PathBuf` of plain components.
pub fn safe_relative_path(p: &str) -> Option<PathBuf> {
    if p.is_empty() || p.len() > MAX_PATH_LEN {
        return None;
    }
    if p.starts_with('/') || p.contains('\\') || p.contains(':') || p.chars().any(char::is_control)
    {
        return None;
    }
    let mut out = PathBuf::new();
    let mut depth = 0;
    for comp in p.split('/') {
        if comp.is_empty() || comp.starts_with('.') {
            return None;
        }
        depth += 1;
        if depth > MAX_PATH_DEPTH {
            return None;
        }
        out.push(comp);
    }
    // Defense in depth: every component must be a plain name on this platform.
    if !out
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    Some(out)
}

/// `safe_relative_path` rendered with `/` separators (the form used as a key for
/// bundled files and passed to `validate_manifest`'s `file_exists`).
pub(crate) fn normalized_key(p: &str) -> Option<String> {
    let path = safe_relative_path(p)?;
    let parts: Vec<&str> = path
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    Some(parts.join("/"))
}

/// MIME type for a served asset by extension (png, webp, svg, json, gif, jpg);
/// `None` for anything else (the protocol refuses it). Extensions match in any
/// ASCII case; `jpeg` is accepted as `jpg`.
pub fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        _ => return None,
    })
}

/// True if `path` has an image type allowed for sprite sheets (every
/// `mime_for` type except JSON).
pub fn is_sheet_type(path: &Path) -> bool {
    mime_for(path).is_some_and(|m| m.starts_with("image/"))
}

/// Percent-encode each `/`-separated segment of a pack-relative path for use in
/// a `pack://` URL (the protocol handler decodes it again).
pub fn encode_url_path(rel: &str) -> String {
    rel.split('/')
        .map(|seg| utf8_percent_encode(seg, SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_key_joins_with_slashes() {
        assert_eq!(normalized_key("a/b/c.png").as_deref(), Some("a/b/c.png"));
        assert_eq!(normalized_key("../x"), None);
    }

    #[test]
    fn segment_set_keeps_unreserved() {
        assert_eq!(encode_url_path("A-z_0.9~"), "A-z_0.9~");
        assert_eq!(encode_url_path("a b/c%d"), "a%20b/c%25d");
    }
}
