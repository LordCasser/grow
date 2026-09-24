//! Numbered image anchors and local image attachment URI helpers.
//!
//! `[Image #N: <path>]` is display text. The path is stripped before
//! model admission; image bytes arrive only through explicit attachments.
//! Placeholder text must never be used as authority to open a file.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

/// Compiled regex matching the TUI placeholder format
/// `[Image #<digits>: <path>]`.
///
/// * Producer emits exactly `": "` (colon, single space) as the
///   separator — see [`pager::prompt_images::display_text`].
///   The regex requires the same; a path token like `[Image #5:foo]`
///   does **not** match.
/// * The path capture excludes `]`, `\n`, and `\r` so the match
///   terminates cleanly at the placeholder boundary even on
///   Windows-style line endings or path strings containing other
///   bracket forms.
/// * Path captures may contain spaces (typical macOS paths in
///   `~/My Pictures`).
static IMAGE_PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[Image #(\d+): ([^\]\r\n]+?)\]").expect("placeholder regex is valid")
});

/// Rewrites every `[Image #N: <path>]` placeholder in `text` to the
/// shorter `[Image #N]` form, dropping the path component.
///
/// The path is harmful: the model may treat it as a hint and call the
/// `Read` tool. The bracketed anchor `[Image #N]` remains in the prose.
///
/// Takes `String` by value so the common no-placeholder case returns
/// the input unchanged with zero allocations.
///
/// Every matching placeholder is stripped, including those beyond the
/// former orphan-recovery count cap.
pub fn strip_paths_from_image_placeholders(text: String) -> String {
    use std::fmt::Write as _;
    // Fast path: probe with `is_match` (no `Captures` allocation) and
    // return the owned input unchanged when there is nothing to do.
    if !IMAGE_PLACEHOLDER_RE.is_match(&text) {
        return text;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    for cap in IMAGE_PLACEHOLDER_RE.captures_iter(&text) {
        // Group 0 is the full match and group 1 is `(\d+)` — both are
        // structurally guaranteed by the regex.
        let whole = cap.get(0).expect("regex match always has group 0");
        let n = cap.get(1).expect("regex always has group 1").as_str();
        out.push_str(&text[last..whole.start()]);
        // `write!` to a String is infallible.
        let _ = write!(out, "[Image #{n}]");
        last = whole.end();
    }
    out.push_str(&text[last..]);
    out
}

/// Encode an absolute filesystem path as a file URI without lossy string conversion.
pub fn file_uri_from_path(path: &Path) -> Option<String> {
    url::Url::from_file_path(path).ok().map(String::from)
}

/// Resolve a plain file URI, decoding path bytes once. If the file is absent,
/// retain the parsed path so callers can still compare attachment identities.
pub fn canonical_from_file_uri(uri: &str) -> Option<PathBuf> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "file" || url.query().is_some() || url.fragment().is_some() {
        return None;
    }
    let path = url.to_file_path().ok()?;
    Some(dunce::canonicalize(&path).unwrap_or(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal valid 1x1 PNG (real format, decodable by `image`).
    const PNG_BYTES: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    fn write_png(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(PNG_BYTES).unwrap();
        path
    }

    // ----- strip_paths_from_image_placeholders ---------------------------

    #[test]
    fn strip_paths_drops_path_keeps_anchor() {
        // The whole point: the model should see the bracketed anchor
        // `[Image #N]` but not the path that would tempt a `Read`.
        assert_eq!(
            strip_paths_from_image_placeholders(
                "what is that?[Image #1: /Users/me/Desktop/x.png] thanks".to_owned()
            ),
            "what is that?[Image #1] thanks"
        );
    }

    #[test]
    fn strip_paths_handles_multiple_placeholders_and_spaces_in_paths() {
        assert_eq!(
            strip_paths_from_image_placeholders(
                "[Image #1: /tmp/a.png] mid [Image #2: /home/u/My Pictures/b.jpg] tail".to_owned()
            ),
            "[Image #1] mid [Image #2] tail"
        );
    }

    #[test]
    fn strip_paths_returns_input_unchanged_when_no_placeholders() {
        // Fast-path: the helper takes `String` by value and the
        // no-match branch returns it verbatim — no allocation. The
        // identity here pins the contract (input string ⇔ output
        // string) byte-for-byte.
        let text = "no placeholder here, just prose";
        assert_eq!(strip_paths_from_image_placeholders(text.to_owned()), text);
    }

    #[test]
    fn strip_paths_preserves_surrounding_whitespace_and_unicode() {
        assert_eq!(
            strip_paths_from_image_placeholders(
                "café \u{202f}[Image #4: /Users/me/Desktop/Screenshot 2026-05-22 at 16.01.21.png] ok"
                    .to_owned()
            ),
            "café \u{202f}[Image #4] ok"
        );
    }

    #[test]
    fn strip_paths_ignores_malformed_placeholders() {
        // None of these match the regex (the last is unterminated, the
        // middle two have empty paths). The leading `[Image #1]` is
        // already in the short form, so the output is bit-identical
        // to the input.
        let text = "[Image #1] [Image #2:] [Image #3: ] [Image #4: /ok.png";
        assert_eq!(strip_paths_from_image_placeholders(text.to_owned()), text);
    }

    #[test]
    fn strip_paths_removes_every_path_after_former_recovery_limit() {
        let text = (1..=20)
            .map(|n| format!("[Image #{n}: /tmp/{n}.png]"))
            .collect::<Vec<_>>()
            .join(" ");
        let expected = (1..=20)
            .map(|n| format!("[Image #{n}]"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(strip_paths_from_image_placeholders(text), expected);
    }

    // ----- canonical_from_file_uri ----------------------------------------

    #[test]
    fn canonical_from_file_uri_rejects_non_file_scheme() {
        assert!(canonical_from_file_uri("https://example.com/x.png").is_none());
        assert!(canonical_from_file_uri("/raw/path").is_none());
    }

    #[test]
    fn canonical_from_file_uri_handles_percent_encoded_path() {
        let dir = tempfile::tempdir().unwrap();
        let with_space = dir.path().join("My Pictures");
        std::fs::create_dir(&with_space).unwrap();
        let png = write_png(&with_space, "cat.png");
        let canon = dunce::canonicalize(&png).unwrap();
        // RFC 3986 form: spaces percent-encoded.
        let raw = format!("file://{}", png.display());
        let encoded = raw.replace(' ', "%20");
        let parsed = canonical_from_file_uri(&encoded).unwrap();
        assert_eq!(parsed, canon);
    }

    #[test]
    fn image_file_uri_roundtrips_reserved_and_literal_percent_names() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "space name.png",
            "literal%20name.png",
            "literal%2Fname.png",
            "hash#query?图片.png",
        ] {
            let path = write_png(dir.path(), name);
            let canonical = dunce::canonicalize(&path).unwrap();
            let uri = file_uri_from_path(&path).unwrap();
            assert_eq!(canonical_from_file_uri(&uri), Some(canonical));
        }
        assert!(file_uri_from_path(Path::new("relative.png")).is_none());
        assert!(canonical_from_file_uri("file:///tmp/image.png?other=1").is_none());
        assert!(canonical_from_file_uri("file:///tmp/image.png#other").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn image_file_uri_roundtrips_non_utf8_path_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        // Some Unix filesystems (including this host's APFS) reject creation
        // of invalid UTF-8 names. Test path-byte conversion without file I/O.
        let path = dunce::canonicalize(dir.path())
            .unwrap()
            .join(std::ffi::OsString::from_vec(b"image-\xff.png".to_vec()));
        let uri = file_uri_from_path(&path).unwrap();
        assert!(uri.contains("%FF"));
        assert_eq!(canonical_from_file_uri(&uri), Some(path));
    }
}
