//! Parse marketplace sources from `~/.grow/config.toml`.
//!
//! Expected format:
//! ```toml
//! [marketplace.bootstrap]
//! name = "Community"
//! git = "https://github.com/example/plugin-marketplace.git"
//!
//! [[marketplace.sources]]
//! name = "Local Dev"
//! path = "~/dev/my-plugins"
//! ```

use std::path::PathBuf;

use crate::types::{MarketplaceSource, SourceKind};

/// Raw TOML source entry.
#[derive(Debug, serde::Deserialize)]
struct RawSource {
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    git: Option<String>,
    #[serde(default)]
    branch: Option<String>,
}

/// Whether remote plugin installs/updates must pin a full commit sha.
///
/// `[marketplace] require_sha = true` in config.toml, or
/// `GROW_MARKETPLACE_REQUIRE_SHA=1`. Tighten-only: either source can enable,
/// neither can override the other off. Defaults off so existing unpinned
/// catalogs keep installing.
pub fn load_require_sha(config: &toml::Value) -> bool {
    env_require_sha()
        || config
            .get("marketplace")
            .and_then(|m| m.get("require_sha"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

pub fn env_require_sha() -> bool {
    config::env_bool("GROW_MARKETPLACE_REQUIRE_SHA").unwrap_or(false)
}

/// Reads `[marketplace].sources` array. Returns empty vec if not configured.
pub fn load_sources(config: &toml::Value) -> Vec<MarketplaceSource> {
    let Some(marketplace) = config.get("marketplace") else {
        return Vec::new();
    };
    let mut sources = Vec::new();

    if let Some(bootstrap) = marketplace.get("bootstrap") {
        match bootstrap.clone().try_into::<RawSource>() {
            Ok(raw) => {
                if let Some(source) = into_source(raw, true) {
                    sources.push(source);
                }
            }
            Err(error) => tracing::warn!("failed to parse marketplace.bootstrap: {error}"),
        }
    }

    let Some(sources_val) = marketplace.get("sources") else {
        return sources;
    };

    let raw_sources: Vec<RawSource> = match serde_json::to_value(sources_val)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(s) => s,
        None => {
            // Try direct toml deserialization.
            match sources_val.clone().try_into::<Vec<RawSource>>() {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("failed to parse marketplace.sources: {e}");
                    return sources;
                }
            }
        }
    };

    sources.extend(
        raw_sources
            .into_iter()
            .filter_map(|raw| into_source(raw, false)),
    );
    sources
}

fn into_source(raw: RawSource, featured: bool) -> Option<MarketplaceSource> {
    let kind = if let Some(git_url) = raw.git {
        SourceKind::Git {
            url: git_url,
            branch: raw.branch,
        }
    } else if let Some(path_str) = raw.path {
        // Expand ~ to home directory.
        let expanded = if let Some(rest) = path_str.strip_prefix('~') {
            dirs::home_dir()
                .map(|h| {
                    h.join(rest.strip_prefix('/').unwrap_or(rest))
                        .to_string_lossy()
                        .to_string()
                })
                .unwrap_or(path_str.clone())
        } else {
            path_str
        };
        SourceKind::Local {
            path: PathBuf::from(expanded),
        }
    } else {
        tracing::warn!(
            "marketplace source '{}' has neither 'path' nor 'git'",
            raw.name
        );
        return None;
    };
    Some(MarketplaceSource {
        name: raw.name,
        featured,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_source() {
        let config: toml::Value = toml::from_str(
            r#"
            [[marketplace.sources]]
            name = "Local Dev"
            path = "/home/user/plugins"
            "#,
        )
        .unwrap();
        let sources = load_sources(&config);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "Local Dev");
        assert!(
            matches!(&sources[0].kind, SourceKind::Local { path } if path == &PathBuf::from("/home/user/plugins"))
        );
    }

    #[test]
    fn parse_git_source() {
        let config: toml::Value = toml::from_str(
            r#"
            [[marketplace.sources]]
            name = "Community"
            git = "https://github.com/example/plugin-marketplace.git"
            branch = "main"
            "#,
        )
        .unwrap();
        let sources = load_sources(&config);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "Community");
        assert!(!sources[0].featured);
        assert!(
            matches!(&sources[0].kind, SourceKind::Git { url, branch } if url.contains("example") && branch.as_deref() == Some("main"))
        );
    }

    #[test]
    fn bootstrap_source_is_loaded_first_and_featured() {
        let config: toml::Value = toml::from_str(
            r#"
            [marketplace.bootstrap]
            name = "Preferred"
            git = "https://github.com/example/preferred.git"

            [[marketplace.sources]]
            name = "Extra"
            git = "https://github.com/example/extra.git"
            "#,
        )
        .unwrap();
        let sources = load_sources(&config);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "Preferred");
        assert!(sources[0].featured);
        assert!(!sources[1].featured);
    }

    #[test]
    fn parse_mixed_sources() {
        let config: toml::Value = toml::from_str(
            r#"
            [[marketplace.sources]]
            name = "Local"
            path = "/tmp/plugins"

            [[marketplace.sources]]
            name = "Remote"
            git = "https://example.com/plugins.git"
            "#,
        )
        .unwrap();
        let sources = load_sources(&config);
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn empty_config_returns_empty() {
        let config: toml::Value = toml::from_str("").unwrap();
        assert!(load_sources(&config).is_empty());
    }

    /// Drives the shipped composition: config alone, env alone, and the
    /// tighten-only rule (falsy env cannot relax config-set true).
    #[test]
    fn require_sha_policy_composition() {
        // Process-global env: serialize against any other env-touching test.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = ENV_LOCK.lock().unwrap();

        let empty: toml::Value = toml::from_str("").unwrap();
        let enabled: toml::Value = toml::from_str("[marketplace]\nrequire_sha = true\n").unwrap();

        // SAFETY: single-threaded within the lock; restored before release.
        unsafe { std::env::remove_var("GROW_MARKETPLACE_REQUIRE_SHA") };
        assert!(!load_require_sha(&empty), "absent everywhere → off");
        assert!(load_require_sha(&enabled), "config alone can enable");

        unsafe { std::env::set_var("GROW_MARKETPLACE_REQUIRE_SHA", "1") };
        assert!(load_require_sha(&empty), "env alone can enable");

        unsafe { std::env::set_var("GROW_MARKETPLACE_REQUIRE_SHA", "0") };
        assert!(
            load_require_sha(&enabled),
            "a falsy env must not relax config-set policy (tighten-only)"
        );

        unsafe { std::env::remove_var("GROW_MARKETPLACE_REQUIRE_SHA") };
    }

    #[test]
    fn missing_sources_key_returns_empty() {
        let config: toml::Value = toml::from_str("[marketplace]\n").unwrap();
        assert!(load_sources(&config).is_empty());
    }

    #[test]
    fn source_without_path_or_git_skipped() {
        let config: toml::Value = toml::from_str(
            r#"
            [[marketplace.sources]]
            name = "Bad"
            "#,
        )
        .unwrap();
        assert!(load_sources(&config).is_empty());
    }
}
