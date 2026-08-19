//! Parse the CI-generated `plugin-index.json` component catalog.
//!
//! The optional catalog has one location:
//! `.grow-plugin/plugin-index.json`. It is presentation-layer enrichment only;
//! failures degrade to `None` and never alter marketplace identity.

use std::collections::HashMap;
use std::path::Path;

use extension_types::PluginComponents;
use serde::Deserialize;

/// Catalog format version this client understands.
const SUPPORTED_VERSION: u64 = 1;

/// Top-level `plugin-index.json` catalog, keyed by index plugin name.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCatalog {
    pub version: u64,
    #[serde(default)]
    pub plugins: HashMap<String, CatalogEntry>,
}

/// Per-plugin catalog entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogEntry {
    /// Commit the components were extracted from (required for URL-sourced
    /// entries; optional for in-repo plugins).
    #[serde(default)]
    pub sha: Option<String>,
    pub components: PluginComponents,
}

impl PluginCatalog {
    /// Components for an index entry, gated on the pinned SHA for
    /// URL-sourced entries: when `index_sha` is `Some`, the catalog entry
    /// must carry an equal `sha` or the components are treated as absent.
    pub fn components_for(
        &self,
        index_name: &str,
        index_sha: Option<&str>,
    ) -> Option<&PluginComponents> {
        let entry = self.plugins.get(index_name)?;
        if let Some(expected) = index_sha
            && entry.sha.as_deref() != Some(expected)
        {
            tracing::debug!(
                plugin = index_name,
                catalog_sha = entry.sha.as_deref().unwrap_or(""),
                index_sha = expected,
                "marketplace catalog sha mismatch; hiding components"
            );
            return None;
        }
        Some(&entry.components)
    }
}

/// Load `plugin-index.json` from a marketplace root, or `None` when absent,
/// malformed, or of an unsupported version.
pub fn load_catalog(marketplace_root: &Path) -> Option<PluginCatalog> {
    let path = marketplace_root
        .join(".grow-plugin")
        .join("plugin-index.json");
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!("failed to read {}: {error}", path.display());
            return None;
        }
    };
    let mut catalog: PluginCatalog = match serde_json::from_str(&content) {
        Ok(catalog) => catalog,
        Err(error) => {
            tracing::warn!("failed to parse {}: {error}", path.display());
            return None;
        }
    };
    if catalog.version != SUPPORTED_VERSION {
        tracing::warn!(
            "unsupported plugin catalog version {} in {}",
            catalog.version,
            path.display()
        );
        return None;
    }
    for entry in catalog.plugins.values_mut() {
        entry.components.sanitize();
    }
    Some(catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_catalog(root: &Path, content: &str) {
        let directory = root.join(".grow-plugin");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("plugin-index.json"), content).unwrap();
    }

    #[test]
    fn loads_canonical_catalog() {
        let root = tempfile::tempdir().unwrap();
        write_catalog(
            root.path(),
            r#"{"version":1,"plugins":{"p":{"sha":"abc","components":{"skills":[{"name":"review"}]}}}}"#,
        );
        let catalog = load_catalog(root.path()).unwrap();
        assert_eq!(
            catalog.components_for("p", Some("abc")).unwrap().skills[0].name,
            "review"
        );
        assert!(catalog.components_for("p", Some("other")).is_none());
    }

    #[test]
    fn rejects_alternate_location_and_unknown_fields() {
        let claude = tempfile::tempdir().unwrap();
        let directory = claude.path().join(".claude-plugin");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("plugin-index.json"),
            r#"{"version":1,"plugins":{}}"#,
        )
        .unwrap();
        assert!(load_catalog(claude.path()).is_none());

        let unknown = tempfile::tempdir().unwrap();
        write_catalog(
            unknown.path(),
            r#"{"version":1,"generatedAt":"now","plugins":{}}"#,
        );
        assert!(load_catalog(unknown.path()).is_none());
    }
}
