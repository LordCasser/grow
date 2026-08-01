//! Local changelog loading.
//! Consumers pick the format they need:
//! - `/release-notes` uses `changelog.markdown` for rich scrollback display
//! - Welcome screen uses `changelog.entries` for bullet rendering

use std::path::PathBuf;

/// A single structured changelog entry from the published JSON changelog.
///
/// Shape must match the output of `render_external_json` in `changelog.sh`:
///   `{category, description, breaking_change}`
/// If you change fields here, update `changelog.sh:render_external_json` too.
///
/// All fields use `#[serde(default)]` so a single malformed entry doesn't
/// kill the entire array parse. Entries with an empty description are
/// filtered out by `bullets_from_entries`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChangelogEntry {
    /// Category label (e.g. "features", "fixes", "breaking", "performance").
    #[serde(default)]
    pub category: String,
    /// Human-readable description (may contain `**bold**` or backticks).
    #[serde(default)]
    pub description: String,
    /// Whether this entry represents a breaking change.
    #[serde(default)]
    pub breaking_change: bool,
}

/// Both formats of a version's changelog, fetched together.
pub struct Changelog {
    /// Rendered markdown (for `/release-notes` display).
    pub markdown: Option<String>,
    /// Structured entries (for welcome screen bullets).
    pub entries: Option<Vec<ChangelogEntry>>,
}

/// Loads changelog files from the Grow home directory.
///
/// Single entry point: `fetch()` returns both markdown and JSON in one
/// `Changelog` struct. Each format is fetched independently with its own
/// cache file, so a failure in one doesn't block the other.
pub struct ChangelogManager {
    md_cache: PathBuf,
    json_cache: PathBuf,
}

impl Default for ChangelogManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangelogManager {
    pub fn new() -> Self {
        // Prefer live `$GROW_HOME` so harness-injected homes (PTY e2e) always
        // win over a OnceLock that may have been initialised earlier with a
        // different path in the same process graph.
        Self::from_env_home()
    }

    /// Resolve cache paths from the live process environment (not the
    /// `grow_home()` OnceLock). A seeded `$GROW_HOME` set on the pager
    /// process is always honoured even if some earlier init path cached a
    /// different home.
    fn from_env_home() -> Self {
        let home = std::env::var_os("GROW_HOME")
            .map(std::path::PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(crate::util::grow_home::grow_home);
        Self {
            md_cache: home.join("CHANGELOG.md"),
            json_cache: home.join("CHANGELOG.json"),
        }
    }

    /// Load both markdown and JSON changelogs without network access.
    pub fn fetch(&self) -> Changelog {
        let current = Self::from_env_home();
        Changelog {
            markdown: read_cache(&current.md_cache),
            entries: current.read_json_cache(),
        }
    }

    fn read_json_cache(&self) -> Option<Vec<ChangelogEntry>> {
        let cached = read_cache(&self.json_cache)?;
        match serde_json::from_str(&cached) {
            Ok(entries) => Some(entries),
            Err(e) => {
                tracing::debug!(error = %e, "failed to parse cached JSON changelog");
                None
            }
        }
    }
}

fn read_cache(path: &std::path::Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .filter(|c| !c.trim().is_empty())
}

/// Strip `**bold**` markers and backticks from a description string.
fn strip_markdown_inline(s: &str) -> String {
    s.replace("**", "").replace('`', "")
}

/// Convert changelog entries to plain-text bullet strings.
///
/// Strips `**bold**` and backtick formatting from each description,
/// skips entries with empty descriptions (from tolerant deserialization),
/// and returns at most `max` entries.
pub fn bullets_from_entries(entries: &[ChangelogEntry], max: usize) -> Vec<String> {
    entries
        .iter()
        .filter(|e| !e.description.is_empty())
        .take(max)
        .map(|e| strip_markdown_inline(&e.description))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a manager pointing at `home` directly, bypassing the global
    /// `$GROW_HOME` env so tests never race the parallel harness.
    fn manager_for(home: &std::path::Path) -> ChangelogManager {
        ChangelogManager {
            md_cache: home.join("CHANGELOG.md"),
            json_cache: home.join("CHANGELOG.json"),
        }
    }

    #[test]
    fn offline_mode_reads_seeded_disk_cache_only() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("grow-home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("CHANGELOG.md"), "# seeded offline md\n").unwrap();
        std::fs::write(
            home.join("CHANGELOG.json"),
            r#"[{"category":"features","description":"seeded entry","breaking_change":false}]"#,
        )
        .unwrap();

        // Offline path: read only the seeded disk cache, no network.
        let manager = manager_for(&home);
        let changelog = Changelog {
            markdown: read_cache(&manager.md_cache),
            entries: manager.read_json_cache(),
        };
        assert_eq!(
            changelog.markdown.as_deref(),
            Some("# seeded offline md\n"),
            "offline mode must return seeded markdown"
        );
        let entries = changelog.entries.expect("seeded json entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].description, "seeded entry");
    }

    #[test]
    fn bullets_strips_markdown_and_respects_max() {
        let entries = vec![
            ChangelogEntry {
                category: "features".into(),
                description: "Added **dark mode** support".into(),
                breaking_change: false,
            },
            ChangelogEntry {
                category: "fixes".into(),
                description: "Fixed `crash` on startup".into(),
                breaking_change: false,
            },
            ChangelogEntry {
                category: "performance".into(),
                description: "Faster **rendering** of `code` blocks".into(),
                breaking_change: false,
            },
        ];

        let bullets = bullets_from_entries(&entries, 2);
        assert_eq!(bullets.len(), 2);
        assert_eq!(bullets[0], "Added dark mode support");
        assert_eq!(bullets[1], "Fixed crash on startup");
    }

    #[test]
    fn bullets_skips_empty_descriptions() {
        let entries = vec![
            ChangelogEntry {
                category: "features".into(),
                description: "Good entry".into(),
                breaking_change: false,
            },
            ChangelogEntry {
                category: String::new(),
                description: String::new(), // bad entry from tolerant deser
                breaking_change: false,
            },
            ChangelogEntry {
                category: "fixes".into(),
                description: "Another good one".into(),
                breaking_change: false,
            },
        ];
        let bullets = bullets_from_entries(&entries, 10);
        assert_eq!(bullets, vec!["Good entry", "Another good one"]);
    }

    #[test]
    fn tolerant_deserialization_partial_entry() {
        // Missing description field → defaults to empty string, not a parse error
        let json = r#"[{"category":"features"},{"description":"ok"}]"#;
        let entries: Vec<ChangelogEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].description, "");
        assert_eq!(entries[1].category, "");
        assert_eq!(entries[1].description, "ok");
    }
}
