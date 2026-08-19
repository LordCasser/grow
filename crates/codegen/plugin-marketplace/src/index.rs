//! Canonical marketplace index parsing.
//!
//! A marketplace is defined by exactly one required
//! `.grow-plugin/marketplace.json`. Directory scanning and alternate index
//! locations are intentionally not part of the contract.

use std::path::Path;

use serde::Deserialize;

use crate::types::MarketplaceRelativePath;

const MARKETPLACE_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarketplaceIndex {
    pub version: u64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub owner: Option<IndexOwner>,
    pub plugins: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexOwner {
    pub name: String,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexEntry {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub author: Option<IndexAuthor>,
    pub source: IndexSource,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexAuthor {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum IndexSource {
    Local {
        path: String,
    },
    Git {
        url: String,
        #[serde(default, rename = "ref")]
        git_ref: Option<String>,
        #[serde(default)]
        sha: Option<String>,
        #[serde(default)]
        path: Option<String>,
    },
}

impl IndexEntry {
    pub fn resolved_marketplace_path(&self) -> Result<MarketplaceRelativePath, String> {
        let IndexSource::Local { path } = &self.source else {
            return Err("git source has no marketplace-local path".into());
        };
        MarketplaceRelativePath::parse(path).map_err(|error| error.to_string())
    }

    pub fn remote_url(&self) -> Option<(&str, Option<&str>)> {
        match &self.source {
            IndexSource::Git { url, git_ref, .. } => Some((url, git_ref.as_deref())),
            IndexSource::Local { .. } => None,
        }
    }

    pub fn remote_sha(&self) -> Option<&str> {
        match &self.source {
            IndexSource::Git { sha, .. } => sha.as_deref(),
            IndexSource::Local { .. } => None,
        }
    }

    pub fn remote_subdir(&self) -> Option<&str> {
        match &self.source {
            IndexSource::Git { path, .. } => path.as_deref(),
            IndexSource::Local { .. } => None,
        }
    }
}

pub fn load_index(marketplace_root: &Path) -> Result<MarketplaceIndex, String> {
    let index_path = marketplace_root
        .join(".grow-plugin")
        .join("marketplace.json");
    let content = std::fs::read_to_string(&index_path)
        .map_err(|error| format!("failed to read {}: {error}", index_path.display()))?;
    let index: MarketplaceIndex = serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {error}", index_path.display()))?;
    if index.version != MARKETPLACE_SCHEMA_VERSION {
        return Err(format!(
            "unsupported marketplace schema version {} in {}; expected {}",
            index.version,
            index_path.display(),
            MARKETPLACE_SCHEMA_VERSION
        ));
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_index(root: &Path, content: &str) {
        let directory = root.join(".grow-plugin");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("marketplace.json"), content).unwrap();
    }

    #[test]
    fn loads_canonical_local_and_git_sources() {
        let root = tempfile::tempdir().unwrap();
        write_index(
            root.path(),
            r#"{
                "version": 1,
                "name": "test",
                "plugins": [
                    {"name":"local","source":{"type":"local","path":"plugins/local"}},
                    {"name":"remote","source":{"type":"git","url":"https://example.com/p.git","ref":"main","sha":"abc","path":"plugin"}}
                ]
            }"#,
        );

        let index = load_index(root.path()).unwrap();
        assert_eq!(
            index.plugins[0]
                .resolved_marketplace_path()
                .unwrap()
                .as_str(),
            "plugins/local"
        );
        assert_eq!(
            index.plugins[1].remote_url(),
            Some(("https://example.com/p.git", Some("main")))
        );
        assert_eq!(index.plugins[1].remote_sha(), Some("abc"));
        assert_eq!(index.plugins[1].remote_subdir(), Some("plugin"));
    }

    #[test]
    fn rejects_missing_wrong_version_and_alternate_shapes() {
        let missing = tempfile::tempdir().unwrap();
        assert!(load_index(missing.path()).is_err());

        let wrong_version = tempfile::tempdir().unwrap();
        write_index(
            wrong_version.path(),
            r#"{"version":2,"name":"test","plugins":[]}"#,
        );
        assert!(load_index(wrong_version.path()).is_err());

        let string_source = tempfile::tempdir().unwrap();
        write_index(
            string_source.path(),
            r#"{"version":1,"name":"test","plugins":[{"name":"p","source":"plugins/p"}]}"#,
        );
        assert!(load_index(string_source.path()).is_err());

        let claude_only = tempfile::tempdir().unwrap();
        let directory = claude_only.path().join(".claude-plugin");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("marketplace.json"),
            r#"{"version":1,"name":"test","plugins":[]}"#,
        )
        .unwrap();
        assert!(load_index(claude_only.path()).is_err());
    }
}
