//! Filesystem locations for grow config files and binaries.

use std::io::Read;
use std::path::PathBuf;
use std::sync::OnceLock;

static GROW_HOME: OnceLock<PathBuf> = OnceLock::new();

/// The default user grow directory (`~/.grow`, canonicalized) used when
/// `GROW_HOME` is unset. Exposed so callers (e.g. display helpers) can detect
/// whether [`grow_home()`] is the default without duplicating the computation.
///
/// Uses [`dunce::canonicalize`] instead of [`std::fs::canonicalize`]: on
/// Windows, std returns a verbatim path (`\\?\C:\Users\...`) which external
/// tools choke on — e.g. `git clone` rejects `\\?\` destinations with
/// "Invalid argument", breaking marketplace cache clones under
/// `~/.grow/marketplace-cache`. `dunce` strips the prefix whenever the path
/// is safely representable in legacy form; on non-Windows it is identical to
/// `std::fs::canonicalize`.
///
/// Keep the dunce canonicalization in sync with the hand-rolled duplicate in
/// `fast_worktree::db::resolve_grow_home` (deliberately standalone crate).
pub fn default_grow_home() -> PathBuf {
    #[allow(deprecated)]
    let home = std::env::home_dir().unwrap_or_else(|| PathBuf::from("."));
    dunce::canonicalize(&home).unwrap_or(home).join(".grow")
}

/// Per-user config directory: `$GROW_HOME` or `~/.grow`. Created if needed.
pub fn grow_home() -> PathBuf {
    GROW_HOME
        .get_or_init(|| {
            let grow_home = if let Ok(v) = std::env::var("GROW_HOME") {
                PathBuf::from(v)
            } else {
                default_grow_home()
            };
            let _ = std::fs::create_dir_all(&grow_home);
            grow_home
        })
        .clone()
}

/// The user-global grow home, but only when one genuinely resolves: `Some` when
/// `$GROW_HOME` is set or a home directory is found, `None` otherwise. Unlike
/// [`grow_home()`], this never falls back to a cwd-relative `.grow`, so callers
/// that *scan* user-global grow resources (hooks, marketplace sources, ...) don't
/// mistake a project's `.grow` tree for the user-global one when no home resolves.
pub fn user_grow_home() -> Option<PathBuf> {
    #[allow(deprecated)]
    let resolvable = std::env::var_os("GROW_HOME").is_some() || std::env::home_dir().is_some();
    resolvable.then(grow_home)
}

/// Canonical grow application path: `$GROW_HOME/bin/grow` (Unix) or `grow.exe` (Windows).
pub fn grow_application() -> PathBuf {
    grow_application_in(&grow_home())
}

/// [`grow_application`] under an explicit home instead of `$GROW_HOME`.
pub fn grow_application_in(home: &std::path::Path) -> PathBuf {
    let name = if cfg!(windows) { "grow.exe" } else { "grow" };
    home.join("bin").join(name)
}

/// Max bytes for a single directory name component (macOS APFS, Linux ext4,
/// NTFS all enforce 255 bytes).
const MAX_DIRNAME_BYTES: usize = 255;

/// Encode a CWD string into a filesystem-safe directory name component.
///
/// Short CWDs (URL-encoded form <= 255 bytes) use a reversible, readable
/// URL-encoded representation.
///
/// Long CWDs (> 255 bytes encoded) use a compact `{slug}-{blake3_hex16}`
/// form that is always <= 57 bytes. The session storage adapter writes the
/// marker required by [`decode_cwd_from_dirname`].
pub fn encode_cwd_dirname(cwd: &str) -> String {
    let url_encoded = urlencoding::encode(cwd);
    if url_encoded.len() <= MAX_DIRNAME_BYTES {
        return url_encoded.into_owned();
    }
    let hash = blake3::hash(cwd.as_bytes());
    let hash16 = &hash.to_hex()[..16];
    let leaf = std::path::Path::new(cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("workspace");
    let slug = slugify(leaf, 40);
    let slug = if slug.is_empty() { "workspace" } else { &slug };
    format!("{slug}-{hash16}")
}

/// Recover the original CWD from a sessions CWD directory.
///
/// Tries URL-decoding the directory name first (works for short/legacy dirs).
/// Falls back to reading the metadata marker inside hash-based directories.
pub fn decode_cwd_from_dirname(dir: &std::path::Path) -> Option<String> {
    let directory_metadata = std::fs::symlink_metadata(dir).ok()?;
    if directory_metadata.file_type().is_symlink() || !directory_metadata.is_dir() {
        return None;
    }
    let name = dir.file_name()?.to_str()?;
    if let Ok(decoded) = urlencoding::decode(name) {
        let s = decoded.into_owned();
        // URL-decoded absolute CWDs always start with `/` (Unix) or a drive
        // letter (Windows).  The slug-hash form never does, so this
        // distinguishes the two encodings unambiguously.
        if s.starts_with('/') || (cfg!(windows) && s.chars().nth(1) == Some(':')) {
            return Some(s);
        }
    }
    const MAX_CWD_MARKER_BYTES: u64 = 1024 * 1024;
    let marker = dir.join(".cwd");
    let metadata = std::fs::symlink_metadata(&marker).ok()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_CWD_MARKER_BYTES
    {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let marker_file = options.open(marker).ok()?;
    let mut content = String::new();
    marker_file
        .take(MAX_CWD_MARKER_BYTES.saturating_add(1))
        .read_to_string(&mut content)
        .ok()?;
    if content.len() as u64 > MAX_CWD_MARKER_BYTES {
        return None;
    }
    Some(content.trim().to_string())
}

/// Build the CWD-level session directory path:
/// The storage adapter owns directory creation and marker publication so all
/// session writes share one contained, no-symlink mechanism.
pub fn sessions_cwd_dir(cwd: &str) -> PathBuf {
    grow_home().join("sessions").join(encode_cwd_dirname(cwd))
}

/// Generate a URL-safe slug from a string.
///
/// Lowercases, replaces non-alphanumeric chars with `-`, collapses
/// consecutive dashes, and truncates to `max_len` characters.
fn slugify(input: &str, max_len: usize) -> String {
    let mut result = String::with_capacity(input.len());
    let mut prev_dash = false;
    for c in input.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c);
            prev_dash = false;
        } else if !prev_dash {
            result.push('-');
            prev_dash = true;
        }
    }
    let trimmed = result.trim_matches('-');
    trimmed.chars().take(max_len).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Realistic CWDs that trigger the bug (URL-encoded > 255 bytes).
    const LONG_CWDS: &[&str] = &[
        "/Users/dev/Documents/開発プロジェクト/機能追加/テスト環境/ソースコード/main-branch",
        "/Users/user/Library/Mobile Documents/com~apple~CloudDocs/项目文件/深层嵌套目录/更深层次的/工作区域/project",
        "/Users/user/Library/CloudStorage/OneDrive-대한민국회사/프로젝트/개발환경/소스코드/백엔드/서비스/my-app",
        "/Users/user/Documents/工作文件夹/二零二六年项目/子目录一/子目录二/子目录三/源代码/code",
    ];

    #[test]
    fn long_cwd_uses_hash_fallback_within_name_max() {
        let long_cwd = format!("/Users/test/{}", "中".repeat(30));
        let encoded = encode_cwd_dirname(&long_cwd);
        assert!(encoded.len() <= MAX_DIRNAME_BYTES);
        assert!(!encoded.starts_with("%2F"));
    }

    #[test]
    fn different_long_paths_produce_different_hashes() {
        let a = format!("/Users/test/{}", "中".repeat(30));
        let b = format!("/Users/test/{}", "日".repeat(30));
        assert_ne!(encode_cwd_dirname(&a), encode_cwd_dirname(&b));
    }

    #[test]
    fn decode_reads_cwd_file_for_hash_dirs() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("some-slug-abcdef0123456789");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".cwd"), "/original/long/path").unwrap();
        assert_eq!(
            decode_cwd_from_dirname(&dir),
            Some("/original/long/path".to_string())
        );
    }

    #[test]
    fn decode_returns_none_without_cwd_file() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("some-slug-abcdef0123456789");
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(decode_cwd_from_dirname(&dir), None);
    }

    #[cfg(unix)]
    #[test]
    fn decode_rejects_symlinked_cwd_marker() {
        use std::os::unix::fs::symlink;

        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("some-slug-abcdef0123456789");
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(&outside, "/outside/workspace").unwrap();
        symlink(outside, dir.join(".cwd")).unwrap();
        assert_eq!(decode_cwd_from_dirname(&dir), None);
    }

    #[test]
    fn url_encoded_long_cwd_fails_on_real_filesystem() {
        let tmp = TempDir::new().unwrap();
        let url_encoded = urlencoding::encode(LONG_CWDS[0]).into_owned();
        let result = std::fs::create_dir_all(tmp.path().join(&url_encoded));
        assert!(result.is_err());
    }

    #[test]
    fn full_roundtrip_on_real_filesystem_for_long_cwds() {
        let tmp = TempDir::new().unwrap();
        for cwd in LONG_CWDS {
            let encoded = encode_cwd_dirname(cwd);
            let dir = tmp.path().join(&encoded);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(".cwd"), cwd).unwrap();
            assert_eq!(decode_cwd_from_dirname(&dir).as_deref(), Some(*cwd));
        }
    }

    #[test]
    fn short_cwds_use_url_encoding_and_roundtrip_on_real_filesystem() {
        let tmp = TempDir::new().unwrap();
        for cwd in [
            "/Users/foo/project",
            "/tmp",
            "/Users/user/Documents/project-名前",
        ] {
            let encoded = encode_cwd_dirname(cwd);
            assert_eq!(encoded, urlencoding::encode(cwd).into_owned());
            let dir = tmp.path().join(&encoded);
            std::fs::create_dir_all(&dir).unwrap();
            assert_eq!(decode_cwd_from_dirname(&dir).as_deref(), Some(cwd));
        }
    }

    #[test]
    fn default_grow_home_has_no_verbatim_prefix() {
        // On Windows, std::fs::canonicalize returns `\\?\C:\...` verbatim
        // paths that external tools (notably `git clone`) reject. The dunce
        // canonicalization must yield a plain path. No-op assertion on Unix.
        let home = default_grow_home();
        assert!(!home.to_string_lossy().starts_with(r"\\?\"));
        assert!(home.ends_with(".grow"));
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World!", 40), "hello-world");
    }

    #[test]
    fn slugify_cjk_produces_empty() {
        assert_eq!(slugify("深层目录", 40), "");
    }

    #[test]
    fn slugify_truncates() {
        assert_eq!(slugify(&"a".repeat(100), 10).len(), 10);
    }
}
