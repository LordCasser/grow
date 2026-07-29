mod cursor;
mod envelope;
mod facets;
mod row;

use std::collections::BTreeMap;
use std::sync::LazyLock;

use cursor::{Cursor, paginate};
pub use envelope::{FacetMap, FacetValue, SessionKind, SessionMetaEnvelope};
pub use facets::{
    BRANCH_FACET_KEY, BranchFacet, CWD_FACET_KEY, CwdFacet, FacetProvider, FacetRegistry,
    FacetSummary, FacetSummaryKey, FacetSummaryValue, GIT_ROOT_FACET_KEY, GitRootFacet,
    KIND_FACET_KEY, KindFacet, NormalizedItem, REPO_FACET_KEY, RepoFacet,
    SOURCE_WORKSPACE_FACET_KEY, SourceWorkspaceFacet, WORKTREE_FACET_KEY, WorktreeFacet,
    build_facet_registry,
};
pub use row::{ExtSupersetRow, RowMeta, SessionInfo, UnifiedRow, merged_session_to_row};
use serde::{Deserialize, Serialize};

pub const DEFAULT_LIMIT: usize = 30;

static FACET_REGISTRY: LazyLock<FacetRegistry> = LazyLock::new(build_facet_registry);

pub fn facet_registry() -> &'static FacetRegistry {
    &FACET_REGISTRY
}

pub fn parse_list_req(raw: &str) -> Result<ListReq, serde_json::Error> {
    serde_json::from_str(raw)
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListReq {
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub allow_relax: bool,
    #[serde(default, rename = "_meta")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListScope {
    #[default]
    Cwd,
    Repo,
    All,
}

impl ListScope {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cwd => "cwd",
            Self::Repo => "repo",
            Self::All => "all",
        }
    }

    pub const fn is_relaxed(self) -> bool {
        !matches!(self, Self::Cwd)
    }
}

pub struct UnifiedListResult {
    pub rows: Vec<UnifiedRow>,
    pub next_cursor: Option<String>,
    pub facets: FacetSummary,
    pub scope: ListScope,
}

#[derive(Debug, Default)]
struct ParsedMeta {
    facet_filters: BTreeMap<String, Vec<serde_json::Value>>,
    query: Option<String>,
    limit: Option<usize>,
}

impl ParsedMeta {
    fn parse(meta: Option<&serde_json::Value>) -> Self {
        let Some(meta) = meta else {
            return Self::default();
        };
        let facet_filters = meta
            .get("grow/facetFilters")
            .and_then(serde_json::Value::as_object)
            .map(|filters| {
                filters
                    .iter()
                    .map(|(key, value)| (key.clone(), value_list(value)))
                    .collect()
            })
            .unwrap_or_default();
        let query = meta
            .get("grow/query")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let limit = meta
            .get("grow/limit")
            .and_then(serde_json::Value::as_u64)
            .map(|limit| limit as usize);
        Self {
            facet_filters,
            query,
            limit,
        }
    }
}

fn value_list(value: &serde_json::Value) -> Vec<serde_json::Value> {
    match value {
        serde_json::Value::Array(values) => values.clone(),
        value => vec![value.clone()],
    }
}

pub async fn build_unified_list(req: ListReq) -> UnifiedListResult {
    let registry = facet_registry();
    let ParsedMeta {
        facet_filters,
        query: meta_query,
        limit: meta_limit,
    } = ParsedMeta::parse(req.meta.as_ref());
    let limit = req.limit.or(meta_limit).unwrap_or(DEFAULT_LIMIT);
    let query = req.query.clone().or(meta_query);
    let cursor = Cursor::decode(req.cursor.as_deref());
    let over_fetch = crate::session::merge::over_fetch(limit);

    let mut rows = if excludes_build(&facet_filters) {
        Vec::new()
    } else {
        crate::session::merge::fetch_merged(req.cwd.as_deref(), query.as_deref(), over_fetch)
            .await
            .into_iter()
            .map(|session| merged_session_to_row(session, registry))
            .collect()
    };
    let mut scope = ListScope::Cwd;

    if relax_eligible(&req, &facet_filters, query.as_deref()) && lane_has_no_messages(&rows) {
        let repo_urls = req
            .cwd
            .as_deref()
            .map(|cwd| {
                grow_workspace::session::git::resolve_normalized_remote_urls(std::path::Path::new(
                    cwd,
                ))
            })
            .unwrap_or_default();
        let all_local = crate::session::persistence::list_summaries(None)
            .await
            .unwrap_or_default();
        let scoped = crate::session::merge::filter_summaries_by_repo(all_local, &repo_urls);
        let relaxed: Vec<_> = crate::session::merge::merge(scoped, None, over_fetch)
            .into_iter()
            .map(|session| merged_session_to_row(session, registry))
            .collect();
        if !lane_has_no_messages(&relaxed) {
            rows = relaxed;
            scope = if repo_urls.is_empty() {
                ListScope::All
            } else {
                ListScope::Repo
            };
        }
    }

    let rows = registry.apply_in_memory_filters(&facet_filters, rows);
    let (rows, next_cursor) = paginate(rows, &cursor, limit);
    let facets = registry.summarize_window(&rows);
    UnifiedListResult {
        rows,
        next_cursor: next_cursor.map(|cursor| cursor.encode()),
        facets,
        scope,
    }
}

fn relax_eligible(
    req: &ListReq,
    facet_filters: &BTreeMap<String, Vec<serde_json::Value>>,
    query: Option<&str>,
) -> bool {
    req.allow_relax && facet_filters.is_empty() && req.cwd.is_some() && query.is_none()
}

fn lane_has_no_messages(rows: &[UnifiedRow]) -> bool {
    rows.iter().all(|row| row.legacy.num_messages == 0)
}

fn excludes_build(filters: &BTreeMap<String, Vec<serde_json::Value>>) -> bool {
    match filters.get(KIND_FACET_KEY) {
        Some(allowed) if !allowed.is_empty() => !allowed
            .iter()
            .any(|value| value.as_str() == Some(SessionKind::Build.as_str())),
        _ => false,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtListResponse {
    pub sessions: Vec<ExtSupersetRow>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta")]
    pub meta: ExtListResponseMeta,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtListResponseMeta {
    #[serde(rename = "grow/facets")]
    pub facets: FacetSummary,
    #[serde(rename = "grow/listScope", skip_serializing_if = "Option::is_none")]
    pub list_scope: Option<&'static str>,
}

pub fn ext_list_response(result: UnifiedListResult) -> ExtListResponse {
    ExtListResponse {
        sessions: result
            .rows
            .into_iter()
            .map(UnifiedRow::into_ext_superset)
            .collect(),
        next_cursor: result.next_cursor,
        meta: ExtListResponseMeta {
            facets: result.facets,
            list_scope: result.scope.is_relaxed().then_some(result.scope.as_str()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_list_request_preserves_local_filters() {
        let request =
            parse_list_req(r#"{"cwd":"/repo","query":"rust","limit":5,"allowRelax":true}"#)
                .unwrap();
        assert_eq!(request.cwd.as_deref(), Some("/repo"));
        assert_eq!(request.query.as_deref(), Some("rust"));
        assert_eq!(request.limit, Some(5));
        assert!(request.allow_relax);
    }
}
