use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use tokio::fs;

use shell::util::grow_home::grow_home;

const TTL_SECONDS_BEFORE_AUTO_UPDATE: Duration = Duration::from_secs(60 * 30);
pub const GH_RELEASE_REPO: &str = "LordCasser/grow";

/// Minimal configuration threaded through the update call chain.
#[derive(Debug, Clone)]
pub struct UpdateConfig {
    /// Release channel: "stable" or "alpha". Loaded from config.
    pub channel: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            channel: "stable".to_string(),
        }
    }
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct GrowVersion {
    version: String,
    #[serde(default)]
    stable_version: Option<String>,
    checked_at: String,
}

impl GrowVersion {
    fn is_fresh(&self, now: time::OffsetDateTime, ttl: Duration) -> bool {
        if let Ok(dt) = time::OffsetDateTime::parse(
            &self.checked_at,
            &time::format_description::well_known::Rfc3339,
        ) {
            // Clock-skew guard: future timestamps are never fresh.
            if dt > now {
                return false;
            }
            now - dt < ttl
        } else {
            false
        }
    }

    fn new(version: String, stable_version: Option<String>, now: time::OffsetDateTime) -> Self {
        let checked_at = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| now.to_string());
        Self {
            version,
            stable_version,
            checked_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

/// Fetch the latest version from the public GitHub Releases API.
#[doc(hidden)]
pub async fn fetch_gh_release_version(channel: &str) -> Result<String> {
    let url = format!("https://api.github.com/repos/{GH_RELEASE_REPO}/releases?per_page=100");
    fetch_gh_release_version_from_url(channel, &url).await
}

#[doc(hidden)]
pub async fn fetch_gh_release_version_from_url(channel: &str, url: &str) -> Result<String> {
    let releases = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("grow-updater")
        .build()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json::<Vec<GitHubRelease>>()
        .await?;
    select_gh_release_version(&releases, channel)
        .ok_or_else(|| anyhow::anyhow!("No matching releases found in {GH_RELEASE_REPO}"))
}

fn select_gh_release_version(releases: &[GitHubRelease], channel: &str) -> Option<String> {
    releases
        .iter()
        .filter(|release| !release.draft)
        .filter(|release| channel == "alpha" || !release.prerelease)
        .filter_map(|release| {
            semver::Version::parse(
                release
                    .tag_name
                    .strip_prefix('v')
                    .unwrap_or(&release.tag_name),
            )
            .ok()
        })
        .max()
        .map(|version| version.to_string())
}

/// Fetch the latest version for the given installer type without writing the
/// version cache. Use this when the caller needs to control when the cache is
/// written (e.g. auto-update should only cache after a successful install or
/// when no update is needed).
pub async fn fetch_latest_version(config: &UpdateConfig) -> Result<String> {
    fetch_gh_release_version(&config.channel).await
}

/// Write the version cache to disk, recording that `version` was seen at the
/// current time. Call after confirming the version is current (no update
/// needed) or after a successful install.
///
/// `stable_version` records the current stable channel pointer so that
/// `channel_label()` can derive `[alpha]` vs `[stable]` without network I/O.
pub async fn write_version_cache(version: &str, stable_version: Option<&str>) {
    let version_path = grow_home().join("version.json");
    let now = time::OffsetDateTime::now_utc();
    let json = GrowVersion::new(
        version.to_string(),
        stable_version.map(|s| s.to_string()),
        now,
    );
    if let Some(dir) = version_path.parent()
        && let Err(e) = fs::create_dir_all(dir).await
    {
        tracing::warn!("failed to create version cache directory: {}", e);
        return;
    }
    let tmp = version_path.with_extension("json.tmp");
    let data = match serde_json::to_vec_pretty(&json) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("failed to serialize version cache: {}", e);
            return;
        }
    };
    if let Err(e) = fs::write(&tmp, data).await {
        tracing::warn!("failed to write version cache tmp file: {}", e);
        return;
    }
    if let Err(e) = fs::rename(&tmp, &version_path).await {
        tracing::warn!("failed to rename version cache file: {}", e);
    }
}

/// Fetch the latest GitHub Release version and cache it.
pub async fn get_latest_version(config: &UpdateConfig) -> Result<String> {
    let version = fetch_latest_version(config).await?;
    let stable_ptr = try_fetch_stable_pointer().await;
    write_version_cache(&version, stable_ptr.as_deref()).await;
    Ok(version)
}

/// True if `version.json` exists and is within TTL.
pub async fn is_version_cache_fresh() -> bool {
    let version_path = grow_home().join("version.json");
    let now = time::OffsetDateTime::now_utc();
    if let Ok(version_str) = fs::read_to_string(&version_path).await
        && let Ok(version) = serde_json::from_str::<GrowVersion>(&version_str)
        && version.is_fresh(now, TTL_SECONDS_BEFORE_AUTO_UPDATE)
    {
        return true;
    }
    false
}

pub use version::installed as get_installed_version;

/// Version of the managed grow binary currently on disk, read from the
/// `~/.grow/bin/grow` symlink target (`../downloads/grow-<version>-<platform>`)
/// without exec'ing anything.
///
/// Concurrent updaters (TUI background download, leader hourly checker,
/// explicit `grow update`) decide staleness from this instead of their own
/// compiled-in version, so a binary another process already installed is
/// never downloaded a second time.
///
/// Returns `None` when there is no parseable managed symlink (Windows
/// copy-based installs, dev builds) or when the symlink is DANGLING — a
/// link whose target binary was deleted (e.g. manual `~/.grow/downloads`
/// cleanup) must not report an installed version, or every updater would
/// claim "already up to date" forever while no runnable binary exists.
pub fn installed_on_disk_version() -> Option<String> {
    #[cfg(unix)]
    {
        let app = shell::util::grow_home::grow_application();
        let target = std::fs::read_link(&app).ok()?;
        // metadata() follows the symlink: Err means the target is gone
        // (dangling link) and the version it names is not actually on disk.
        std::fs::metadata(&app).ok()?;
        version_from_versioned_binary_name(target.file_name()?.to_str()?, "grow")
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Extract the `<version>` portion of a versioned binary file name.
///
/// Handles the GitHub Release layout (`grow-0.1.150-macos-aarch64`, including
/// pre-releases: `grow-0.1.150-alpha.1-linux-x86_64` → `0.1.150-alpha.1`):
/// everything between the `{bin_prefix}-` prefix
/// and the first platform-OS component is the version, validated as semver
/// so unknown layouts (`grow-latest`, `grow-pager-*` when `bin_prefix` is
/// `grow`) return `None` instead of garbage.
///
/// Shared by the disk-version probe above and `cleanup_old_downloads` in
/// `auto_update` — keep it the single place that understands this naming.
pub(crate) fn version_from_versioned_binary_name(name: &str, bin_prefix: &str) -> Option<String> {
    const PLATFORM_OS: &[&str] = &["macos", "linux", "windows"];
    let suffix = name.strip_prefix(bin_prefix)?.strip_prefix('-')?;
    let parts: Vec<&str> = suffix.split('-').collect();
    let platform_start = parts
        .iter()
        .position(|p| PLATFORM_OS.contains(p))
        .unwrap_or(parts.len());
    let ver_str = parts[..platform_start].join("-");
    semver::Version::parse(&ver_str).ok()?;
    Some(ver_str)
}

/// Fetch the stable channel pointer for caching alongside the version.
///
/// Best-effort GitHub release lookup. The stable version is only used
/// to derive the `[alpha]`/`[stable]` channel label — it is never required
/// for correctness.
pub(crate) async fn try_fetch_stable_pointer() -> Option<String> {
    tokio::time::timeout(Duration::from_secs(3), fetch_gh_release_version("stable"))
        .await
        .ok()
        .and_then(Result::ok)
}

/// Read the cached stable version from `~/.grow/version.json` (sync, for display).
///
/// Returns `None` if the file doesn't exist, can't be parsed, or has no
/// `stable_version` field (e.g. written by an older binary).
pub fn cached_stable_version() -> Option<String> {
    let version_path = grow_home().join("version.json");
    let content = std::fs::read_to_string(&version_path).ok()?;
    let gv: GrowVersion = serde_json::from_str(&content).ok()?;
    gv.stable_version
}

/// Pure comparison: derive the channel name from current vs stable pointer.
///
/// Returns `Some("alpha")` when `current > stable`, `Some("stable")` when
/// `current <= stable`, or `None` when either version fails to parse.
fn derive_channel<'a>(current: &str, stable: &str) -> Option<&'a str> {
    let current_v = semver::Version::parse(current).ok()?;
    let stable_v = semver::Version::parse(stable).ok()?;
    if current_v > stable_v {
        Some("alpha")
    } else {
        Some("stable")
    }
}

/// Machine-readable channel name derived from the cached stable pointer.
///
/// Returns `Some("alpha")` when the current version is ahead of the cached
/// stable pointer, `Some("stable")` when at or behind, or `None` when no
/// cached pointer is available (first launch, old cache format, parse error).
///
/// The result is computed once and cached for the process lifetime.
pub fn channel_name() -> Option<&'static str> {
    use std::sync::OnceLock;
    static NAME: OnceLock<Option<&'static str>> = OnceLock::new();
    *NAME.get_or_init(|| {
        let stable = cached_stable_version()?;
        derive_channel(version::VERSION, &stable)
    })
}

/// Channel label derived from the cached stable pointer.
///
/// Compares the compiled-in `VERSION` against the stable pointer stored in
/// `~/.grow/version.json` (written by the auto-updater):
/// - `" [alpha]"` when the current version is ahead of stable,
/// - `" [stable]"` when at or behind stable,
/// - `""` when no cached pointer is available (first launch, old cache format).
///
/// The result is computed once and cached for the process lifetime.
pub fn channel_label() -> &'static str {
    use std::sync::OnceLock;
    static LABEL: OnceLock<&'static str> = OnceLock::new();
    LABEL.get_or_init(|| {
        let stable = match cached_stable_version() {
            Some(s) => s,
            None => return "",
        };
        match derive_channel(version::VERSION, &stable) {
            Some("alpha") => " [alpha]",
            Some(_) => " [stable]",
            None => "",
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that a future `checked_at` timestamp (e.g. from clock skew or
    /// NTP time-warp) is never considered fresh. Without the clock-skew guard
    /// this would return true indefinitely, silently disabling auto-update.
    #[test]
    fn test_is_fresh_rejects_future_timestamp() {
        let now = time::OffsetDateTime::now_utc();
        let future = now + Duration::from_secs(600);
        let v = GrowVersion::new("0.1.200".to_string(), None, future);
        assert!(
            !v.is_fresh(now, Duration::from_secs(30)),
            "Future timestamp must not be considered fresh (clock-skew guard)."
        );
    }

    /// Disk-version probe: parsing the version out of the managed install's
    /// symlink-target file name (`grow-<version>-<platform>`).
    #[test]
    fn test_version_from_versioned_binary_name() {
        let cases: &[(&str, Option<&str>)] = &[
            ("grow-0.2.46-macos-aarch64", Some("0.2.46")),
            ("grow-0.1.220-linux-x86_64", Some("0.1.220")),
            ("grow-0.1.220-linux-aarch64-musl", Some("0.1.220")),
            ("grow-0.1.220-windows-x86_64.exe", Some("0.1.220")),
            // Pre-releases must round-trip whole — truncating to "0.1.220"
            // would make an alpha install masquerade as the release and
            // mask alpha → stable updates.
            ("grow-0.1.220-alpha.4-linux-x86_64", Some("0.1.220-alpha.4")),
            ("grow-0.1.220-alpha.4", Some("0.1.220-alpha.4")), // GitHub Release layout
            ("grow-pager-0.1.5-macos-aarch64", None),          // "grow-pager" is not a version
            ("grow-garbage-macos-aarch64", None),              // unparseable version
            ("grow-0.2.46", Some("0.2.46")),                   // no platform suffix
            ("other-0.2.46-macos-aarch64", None),              // wrong prefix
            ("grow-latest", None),                             // symlink alias, not a version
            ("grow", None),                                    // bare name
            ("", None),
        ];
        for (name, expected) in cases {
            assert_eq!(
                version_from_versioned_binary_name(name, "grow").as_deref(),
                *expected,
                "version_from_versioned_binary_name({name:?})"
            );
        }

        // bin_prefix discrimination: the pager binary parses under its own
        // prefix but not under "grow".
        assert_eq!(
            version_from_versioned_binary_name("grow-pager-0.1.5-macos-aarch64", "grow-pager",)
                .as_deref(),
            Some("0.1.5")
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // derive_channel — invariant matrix
    //
    // Tests the pure comparison logic that determines [alpha] vs [stable].
    // Covers current 0.1.X-alpha.N, future 0.2.X, edge cases, and errors.
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn test_derive_channel_matrix() {
        // (current, stable_pointer, expected_channel)
        let cases: &[(&str, &str, Option<&str>)] = &[
            // ── Current 0.1.X workflow ──
            ("0.1.220-alpha.2", "0.1.219", Some("alpha")), // alpha ahead of stable
            ("0.1.219", "0.1.219", Some("stable")),        // stable user on latest
            ("0.1.218", "0.1.219", Some("stable")),        // stable user behind latest
            ("0.1.220-alpha.2", "0.1.220-alpha.2", Some("stable")), // pointer matches exactly
            ("0.1.220-alpha.2", "0.1.220", Some("stable")), // semver: release > pre-release
            // ── Future 0.2.X workflow ──
            ("0.2.5", "0.2.3", Some("alpha")), // alpha ahead of stable
            ("0.2.5", "0.2.5", Some("stable")), // promoted to stable
            ("0.2.3", "0.2.5", Some("stable")), // behind stable
            ("0.2.0", "0.2.0", Some("stable")), // first release, both 0.2.0
            // ── Cross-regime upgrade ──
            ("0.2.0", "0.1.219", Some("alpha")), // new regime ahead of old stable
            ("0.1.220-alpha.2", "0.2.0", Some("stable")), // old pre-release < new stable
            // ── Error cases ──
            ("garbage", "0.1.219", None), // unparseable current
            ("0.1.219", "garbage", None), // unparseable stable
            ("", "0.1.219", None),        // empty current
            ("0.1.219", "", None),        // empty stable
        ];

        for (current, stable, expected) in cases {
            let result = derive_channel(current, stable);
            assert_eq!(
                result, *expected,
                "derive_channel({:?}, {:?}) = {:?}, expected {:?}",
                current, stable, result, expected,
            );
        }
    }

    #[test]
    fn github_release_selection_is_semver_ordered_and_channel_aware() {
        let releases = vec![
            GitHubRelease {
                tag_name: "v0.2.112".into(),
                draft: false,
                prerelease: false,
            },
            GitHubRelease {
                tag_name: "v0.2.113-alpha.2".into(),
                draft: false,
                prerelease: true,
            },
            GitHubRelease {
                tag_name: "v9.0.0".into(),
                draft: true,
                prerelease: false,
            },
            GitHubRelease {
                tag_name: "not-semver".into(),
                draft: false,
                prerelease: false,
            },
        ];
        assert_eq!(
            select_gh_release_version(&releases, "stable").as_deref(),
            Some("0.2.112")
        );
        assert_eq!(
            select_gh_release_version(&releases, "alpha").as_deref(),
            Some("0.2.113-alpha.2")
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // GrowVersion JSON shape — backward compatibility invariants
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn test_version_json_backward_compat() {
        // Old format (no stable_version) must parse — serde(default) fills None.
        let old = r#"{"version":"0.1.180","checked_at":"2026-04-22T10:30:00Z"}"#;
        let v: GrowVersion = serde_json::from_str(old).unwrap();
        assert_eq!(v.version, "0.1.180");
        assert!(v.stable_version.is_none());

        // New format with stable_version round-trips correctly.
        let now = time::OffsetDateTime::now_utc();
        let new = GrowVersion::new("0.2.5".to_string(), Some("0.2.3".to_string()), now);
        let json = serde_json::to_string(&new).unwrap();
        let parsed: GrowVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, "0.2.5");
        assert_eq!(parsed.stable_version.as_deref(), Some("0.2.3"));

        // checked_at must be valid RFC3339.
        assert!(
            time::OffsetDateTime::parse(
                &parsed.checked_at,
                &time::format_description::well_known::Rfc3339,
            )
            .is_ok()
        );

        // Unknown fields are ignored (forward-compat).
        let future = r#"{"version":"0.1.180","checked_at":"2026-04-22T10:30:00Z","future":"ok"}"#;
        assert!(serde_json::from_str::<GrowVersion>(future).is_ok());

        // Missing required field (checked_at) is rejected.
        let missing = r#"{"version":"0.1.180"}"#;
        assert!(serde_json::from_str::<GrowVersion>(missing).is_err());
    }

    // ──────────────────────────────────────────────────────────────────────
    // is_fresh — TTL boundary invariants
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn test_is_fresh_ttl_boundaries() {
        let now = time::OffsetDateTime::now_utc();
        let v = GrowVersion::new("0.1.200".to_string(), None, now);

        // Within TTL → fresh
        assert!(v.is_fresh(now, Duration::from_secs(60)));
        assert!(v.is_fresh(now + Duration::from_secs(29), Duration::from_secs(30)));

        // At TTL boundary → NOT fresh (strict <)
        assert!(!v.is_fresh(now + Duration::from_secs(30), Duration::from_secs(30)));

        // Past TTL → not fresh
        assert!(!v.is_fresh(now + Duration::from_secs(31), Duration::from_secs(30)));

        // Zero TTL → never fresh
        assert!(!v.is_fresh(now, Duration::ZERO));

        // Malformed timestamp → not fresh
        let bad = GrowVersion {
            version: "0.1.200".to_string(),
            stable_version: None,
            checked_at: "not-rfc3339".to_string(),
        };
        assert!(!bad.is_fresh(now, Duration::from_secs(60)));
    }

    // ──────────────────────────────────────────────────────────────────────
    // UpdateConfig defaults
    // ──────────────────────────────────────────────────────────────────────

    #[test]
    fn test_update_config_default_channel_is_stable() {
        let cfg = UpdateConfig::default();
        assert_eq!(cfg.channel, "stable");
    }
}
