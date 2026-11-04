//! The bundled placeholder pack and the `pack://` protocol (SPEC §7).
//!
//! Files are served as `pack://localhost/<packId>/<path>` (Windows:
//! `http://pack.localhost/…`). Path segments arrive percent-encoded and are
//! decoded here; the registry then refuses unsafe paths, unknown or invalid
//! packs, disallowed types, oversized files and anything resolving outside the
//! pack directory. Every refusal is a 404. The handler runs on the main thread
//! and only takes the registry's own read lock, never the engine lock.

use std::borrow::Cow;

use focus_core::packs::BundledPack;
use include_dir::{include_dir, Dir, DirEntry};
use percent_encoding::percent_decode_str;
use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Manager};

use crate::shell::Shell;

pub const SCHEME: &str = "pack";

static PLACEHOLDER: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../packs/placeholder");

/// The placeholder pack embedded in the binary, every file with its
/// `/`-separated path relative to the pack root.
pub fn bundled_pack() -> BundledPack {
    let mut files = Vec::new();
    collect(&PLACEHOLDER, &mut files);
    files.sort_by(|a, b| a.0.cmp(&b.0));
    BundledPack { files }
}

fn collect(dir: &'static Dir<'static>, out: &mut Vec<(String, &'static [u8])>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(d) => collect(d, out),
            DirEntry::File(f) => {
                out.push((f.path().to_string_lossy().replace('\\', "/"), f.contents()))
            }
        }
    }
}

/// URL prefix used in `ResolvedAnimation.url`.
pub fn url_prefix() -> &'static str {
    if cfg!(windows) {
        "http://pack.localhost/"
    } else {
        "pack://localhost/"
    }
}

/// Split a request path (`/<packId>/<percent-encoded path>`) into the pack id
/// and the decoded pack-relative path. Each segment is decoded on its own; a
/// segment that decodes to a separator, NUL, `.`/`..` or a hidden name, an
/// empty segment, or invalid UTF-8 is refused. The registry validates again.
pub fn split_request_path(path: &str) -> Option<(String, String)> {
    let path = path.strip_prefix('/').unwrap_or(path);
    let mut segments = Vec::new();
    for raw in path.split('/') {
        let seg = percent_decode_str(raw).decode_utf8().ok()?;
        let unsafe_segment = seg.is_empty()
            || seg.starts_with('.')
            || seg.contains(['/', '\\', '\0'])
            || seg.chars().any(char::is_control);
        if unsafe_segment {
            return None;
        }
        segments.push(seg.into_owned());
    }
    if segments.len() < 2 {
        return None;
    }
    let pack_id = segments.remove(0);
    Some((pack_id, segments.join("/")))
}

pub fn ok_response(bytes: Vec<u8>, mime: &str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CACHE_CONTROL, "no-cache")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(Cow::Owned(bytes))
        .unwrap_or_else(|_| not_found())
}

pub fn not_found() -> Response<Cow<'static, [u8]>> {
    let mut response = Response::new(Cow::Borrowed(&b""[..]));
    *response.status_mut() = StatusCode::NOT_FOUND;
    let headers = response.headers_mut();
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header::HeaderValue::from_static("*"),
    );
    response
}

/// The protocol handler.
pub fn handle(app: &AppHandle, path: &str) -> Response<Cow<'static, [u8]>> {
    let Some((pack_id, rel)) = split_request_path(path) else {
        log::debug!("pack:// refused path {path:?}");
        return not_found();
    };
    let Some(shell) = app.try_state::<Shell>() else {
        return not_found();
    };
    let file = shell.packs().read_file(&pack_id, &rel);
    match file {
        Some((bytes, mime)) => ok_response(bytes, mime),
        None => {
            log::debug!("pack:// refused {pack_id}/{rel}");
            not_found()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(p: &str) -> Option<(String, String)> {
        split_request_path(p)
    }

    #[test]
    fn splits_pack_id_and_relative_path() {
        assert_eq!(
            split("/placeholder/idle.svg"),
            Some(("placeholder".into(), "idle.svg".into()))
        );
        assert_eq!(
            split("/mypack/sheets/focus.png"),
            Some(("mypack".into(), "sheets/focus.png".into()))
        );
    }

    #[test]
    fn decodes_percent_encoded_segments() {
        assert_eq!(
            split("/fixture-sprout/react%20sheet.svg"),
            Some(("fixture-sprout".into(), "react sheet.svg".into()))
        );
        assert_eq!(
            split("/p/%E6%A3%AE.png"),
            Some(("p".into(), "森.png".into()))
        );
    }

    #[test]
    fn refuses_traversal_hidden_and_odd_segments() {
        for bad in [
            "",
            "/",
            "/placeholder",
            "/placeholder/",
            "//idle.svg",
            "/placeholder//idle.svg",
            "/placeholder/../secret.png",
            "/placeholder/%2E%2E/secret.png",
            "/placeholder/a%2Fb.png",
            "/placeholder/a%5Cb.png",
            "/placeholder/a\\b.png",
            "/placeholder/.hidden.png",
            "/placeholder/%2Ehidden.png",
            "/placeholder/./idle.svg",
            "/placeholder/a%00.png",
            "/placeholder/a%0A.png",
            "/placeholder/%FF.png",
            "/../etc/passwd",
        ] {
            assert_eq!(split(bad), None, "{bad:?} must be refused");
        }
    }

    #[test]
    fn ok_responses_carry_type_cache_and_cors_headers() {
        let r = ok_response(b"<svg/>".to_vec(), "image/svg+xml");
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(r.headers()[header::CACHE_CONTROL], "no-cache");
        assert_eq!(r.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(r.body().as_ref(), b"<svg/>");
    }

    #[test]
    fn refusals_are_empty_404s() {
        let r = not_found();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        assert!(r.body().is_empty());
        assert_eq!(r.headers()[header::CACHE_CONTROL], "no-cache");
    }

    #[test]
    fn url_prefix_matches_the_platform_form() {
        if cfg!(windows) {
            assert_eq!(url_prefix(), "http://pack.localhost/");
        } else {
            assert_eq!(url_prefix(), "pack://localhost/");
        }
    }

    #[test]
    fn bundled_pack_embeds_the_placeholder_folder_recursively() {
        let pack = bundled_pack();
        let names: Vec<&str> = pack.files.iter().map(|(p, _)| p.as_str()).collect();
        assert!(names.contains(&"LICENSE.md"), "{names:?}");
        for (path, bytes) in &pack.files {
            assert!(!path.contains('\\') && !path.starts_with('/'), "{path}");
            let on_disk = std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../packs/placeholder")
                    .join(path),
            )
            .expect("embedded file exists on disk");
            assert_eq!(&on_disk[..], *bytes, "{path} embedded verbatim");
        }
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "stable order");
    }

    #[test]
    fn bundled_pack_includes_the_manifest_once_it_lands() {
        // packs/placeholder/pack.json is written by the packs agent; once it exists
        // it must be embedded.
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packs/placeholder/pack.json");
        if manifest.exists() {
            assert!(bundled_pack().files.iter().any(|(p, _)| p == "pack.json"));
        }
    }
}
