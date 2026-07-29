use std::collections::BTreeMap;

use serde::Serialize;

use super::envelope::{FacetMap, FacetValue, SessionKind};
use super::row::UnifiedRow;
use crate::session::merge::MergedSession;

pub const KIND_FACET_KEY: &str = "kind";
pub const CWD_FACET_KEY: &str = "cwd";
pub const REPO_FACET_KEY: &str = "repo";
pub const BRANCH_FACET_KEY: &str = "branch";
pub const WORKTREE_FACET_KEY: &str = "worktree";
pub const GIT_ROOT_FACET_KEY: &str = "gitRoot";
pub const SOURCE_WORKSPACE_FACET_KEY: &str = "sourceWorkspace";

#[derive(Debug, Clone)]
pub struct NormalizedItem {
    pub kind: SessionKind,
    pub cwd: String,
    pub repo_name: Option<String>,
    pub branch: Option<String>,
    pub worktree_label: Option<String>,
    pub git_root_dir: Option<String>,
    pub source_workspace_dir: Option<String>,
}

impl NormalizedItem {
    pub fn from_merged(session: &MergedSession) -> Self {
        Self {
            kind: SessionKind::Build,
            cwd: session.cwd.clone(),
            repo_name: session.repo_name.clone(),
            branch: session.branch.clone(),
            worktree_label: session.worktree_label.clone(),
            git_root_dir: session.git_root_dir.clone(),
            source_workspace_dir: session.source_workspace_dir.clone(),
        }
    }
}

pub trait FacetProvider: Send + Sync {
    fn key(&self) -> &'static str;
    fn extract(&self, item: &NormalizedItem) -> Option<FacetValue>;
}

pub struct KindFacet;

impl FacetProvider for KindFacet {
    fn key(&self) -> &'static str {
        KIND_FACET_KEY
    }

    fn extract(&self, item: &NormalizedItem) -> Option<FacetValue> {
        Some(FacetValue::One(serde_json::Value::String(
            item.kind.as_str().to_owned(),
        )))
    }
}

macro_rules! string_facet_provider {
    ($name:ident, $key:ident, $field:ident) => {
        pub struct $name;
        impl FacetProvider for $name {
            fn key(&self) -> &'static str {
                $key
            }
            fn extract(&self, item: &NormalizedItem) -> Option<FacetValue> {
                string_facet(item.$field.as_deref())
            }
        }
    };
}

pub struct CwdFacet;

impl FacetProvider for CwdFacet {
    fn key(&self) -> &'static str {
        CWD_FACET_KEY
    }

    fn extract(&self, item: &NormalizedItem) -> Option<FacetValue> {
        string_facet(Some(&item.cwd))
    }
}

string_facet_provider!(RepoFacet, REPO_FACET_KEY, repo_name);
string_facet_provider!(BranchFacet, BRANCH_FACET_KEY, branch);
string_facet_provider!(WorktreeFacet, WORKTREE_FACET_KEY, worktree_label);
string_facet_provider!(GitRootFacet, GIT_ROOT_FACET_KEY, git_root_dir);
string_facet_provider!(
    SourceWorkspaceFacet,
    SOURCE_WORKSPACE_FACET_KEY,
    source_workspace_dir
);

fn string_facet(value: Option<&str>) -> Option<FacetValue> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| FacetValue::One(serde_json::Value::String(value.to_owned())))
}

#[derive(Default)]
pub struct FacetRegistry {
    providers: Vec<Box<dyn FacetProvider>>,
}

impl FacetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, provider: impl FacetProvider + 'static) -> Self {
        self.providers.push(Box::new(provider));
        self
    }

    pub fn extract_all(&self, item: &NormalizedItem) -> FacetMap {
        self.providers
            .iter()
            .filter_map(|provider| {
                provider
                    .extract(item)
                    .map(|value| (provider.key().to_owned(), value))
            })
            .collect()
    }

    pub fn apply_in_memory_filters(
        &self,
        filters: &BTreeMap<String, Vec<serde_json::Value>>,
        rows: Vec<UnifiedRow>,
    ) -> Vec<UnifiedRow> {
        let active: Vec<_> = filters
            .iter()
            .filter(|(key, allowed)| key.as_str() != CWD_FACET_KEY && !allowed.is_empty())
            .filter_map(|(key, allowed)| {
                self.providers
                    .iter()
                    .find(|provider| provider.key() == key)
                    .map(|provider| (provider.key(), allowed))
            })
            .collect();
        rows.into_iter()
            .filter(|row| {
                active.iter().all(|(key, allowed)| {
                    row.facets
                        .get(*key)
                        .is_some_and(|value| value.intersects(allowed))
                })
            })
            .collect()
    }

    pub fn summarize_window(&self, rows: &[UnifiedRow]) -> FacetSummary {
        let mut counts: BTreeMap<String, BTreeMap<String, (serde_json::Value, usize)>> =
            BTreeMap::new();
        for row in rows {
            for (key, value) in &row.facets {
                let values = counts.entry(key.clone()).or_default();
                for value in value.values() {
                    values
                        .entry(value.to_string())
                        .or_insert_with(|| (value.clone(), 0))
                        .1 += 1;
                }
            }
        }
        FacetSummary {
            scope: "window",
            keys: counts
                .into_iter()
                .map(|(key, values)| FacetSummaryKey {
                    key,
                    values: values
                        .into_values()
                        .map(|(value, count)| FacetSummaryValue {
                            value,
                            label: None,
                            count,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

pub fn build_facet_registry() -> FacetRegistry {
    FacetRegistry::new()
        .with(KindFacet)
        .with(CwdFacet)
        .with(RepoFacet)
        .with(BranchFacet)
        .with(WorktreeFacet)
        .with(GitRootFacet)
        .with(SourceWorkspaceFacet)
}

#[derive(Debug, Clone, Serialize)]
pub struct FacetSummary {
    pub scope: &'static str,
    pub keys: Vec<FacetSummaryKey>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FacetSummaryKey {
    pub key: String,
    pub values: Vec<FacetSummaryValue>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FacetSummaryValue {
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_registry_contains_only_local_facets() {
        let registry = build_facet_registry();
        let facets = registry.extract_all(&NormalizedItem {
            kind: SessionKind::Build,
            cwd: "/repo".to_owned(),
            repo_name: Some("grow".to_owned()),
            branch: None,
            worktree_label: None,
            git_root_dir: Some("/repo".to_owned()),
            source_workspace_dir: None,
        });
        assert_eq!(facets[KIND_FACET_KEY].values()[0], "build");
        assert_eq!(facets[CWD_FACET_KEY].values()[0], "/repo");
        assert!(!facets.contains_key("workspace"));
        assert!(!facets.contains_key("starred"));
    }
}
