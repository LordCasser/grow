//! Local session listing shared by the ACP session picker and `grow sessions`.

use std::cmp::Reverse;
use std::collections::HashSet;

use serde::Serialize;

use crate::session::persistence::{Summary, list_summaries};
use workspace::session::git::normalize_repo_url;

/// Over-fetch before in-memory facet filtering and pagination.
pub(crate) fn over_fetch(limit: usize) -> usize {
    (limit * 3).max(100)
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionListing {
    pub session_id: String,
    pub title: String,
    pub updated_at: String,
    pub created_at: String,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default)]
    pub num_messages: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_root_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_remotes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_workspace_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
}

pub async fn fetch_sessions(
    cwd: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Vec<SessionListing> {
    let summaries = fetch_local_summaries(cwd).await;
    list_from_summaries(summaries, query, limit)
}

pub(crate) async fn fetch_local_summaries(cwd: Option<&str>) -> Vec<Summary> {
    let cwds = cwd
        .map(|cwd| {
            crate::session::worktree::candidate_worktree_cwds_for_same_repo(std::path::Path::new(
                cwd,
            ))
            .unwrap_or_else(|_| vec![cwd.to_owned()])
        })
        .unwrap_or_default();

    if cwds.is_empty() {
        return list_summaries(cwd).await.unwrap_or_default();
    }

    let mut summaries = Vec::new();
    for cwd in cwds {
        if let Ok(found) = list_summaries(Some(&cwd)).await {
            summaries.extend(found);
        }
    }
    summaries
}

pub(crate) fn filter_summaries_by_repo(
    summaries: Vec<Summary>,
    repo_urls: &[String],
) -> Vec<Summary> {
    if repo_urls.is_empty() {
        return summaries;
    }
    summaries
        .into_iter()
        .filter(|summary| {
            summary
                .git_remotes
                .iter()
                .any(|url| normalize_repo_url(url).is_some_and(|url| repo_urls.contains(&url)))
        })
        .collect()
}

pub(crate) fn list_from_summaries(
    local: Vec<Summary>,
    query: Option<&str>,
    limit: usize,
) -> Vec<SessionListing> {
    let query = query.map(str::to_lowercase);
    let mut sessions: Vec<SessionListing> = local
        .into_iter()
        .filter(|summary| {
            query.as_ref().is_none_or(|query| {
                summary.info.id.to_string().to_lowercase().contains(query)
                    || summary.display_title().to_lowercase().contains(query)
            })
        })
        .map(summary_to_session)
        .collect();

    sessions.sort_by_cached_key(|session| {
        (
            Reverse(effective_sort_time(session)),
            session.session_id.clone(),
        )
    });
    dedup_empty_sessions(&mut sessions);
    sessions.truncate(limit);
    sessions
}

fn summary_to_session(summary: Summary) -> SessionListing {
    let repo_name = summary
        .git_root_dir
        .as_deref()
        .and_then(|path| std::path::Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned);
    let worktree_label = summary
        .worktree_label
        .clone()
        .or_else(|| crate::session::worktree::lookup_worktree_label(&summary.info.cwd));

    SessionListing {
        session_id: summary.info.id.to_string(),
        title: summary.display_title().to_owned(),
        updated_at: summary.updated_at.to_rfc3339(),
        created_at: summary.created_at.to_rfc3339(),
        cwd: summary.info.cwd,
        hostname: None,
        model_id: Some(summary.current_model_id.to_string()),
        num_messages: summary.num_messages,
        last_active_at: summary.last_active_at.map(|time| time.to_rfc3339()),
        branch: summary.head_branch,
        repo_name,
        worktree_label,
        git_root_dir: summary.git_root_dir,
        git_remotes: summary.git_remotes,
        source_workspace_dir: summary.source_workspace_dir,
        session_kind: summary.session_kind,
    }
}

fn effective_sort_time(session: &SessionListing) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    session
        .last_active_at
        .as_deref()
        .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
        .or_else(|| chrono::DateTime::parse_from_rfc3339(&session.updated_at).ok())
}

fn dedup_empty_sessions(sessions: &mut Vec<SessionListing>) {
    let mut seen_empty_cwds = HashSet::new();
    sessions.retain(|session| {
        session.num_messages != 0 || seen_empty_cwds.insert(normalize_cwd(&session.cwd))
    });
}

fn normalize_cwd(cwd: &str) -> String {
    let trimmed = cwd.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else {
        trimmed.replace("/./", "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::info::Info;
    use agent_client_protocol as acp;
    use chrono::{TimeZone, Utc};

    fn summary(id: &str, cwd: &str, title: &str, messages: usize) -> Summary {
        Summary {
            info: Info {
                id: acp::SessionId::new(id),
                cwd: cwd.to_owned(),
            },
            cwd_generation: 0,
            previous_cwd: None,
            pending_cwd_switch_reminder: None,
            cwd_switch_bookkeeping_generation: 0,
            title: Some(title.to_owned()),
            title_source: Some(chat_state::SessionTitleSource::User),
            title_event_seq: Some(1),
            created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            updated_at: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            num_messages: messages,
            current_model_id: acp::ModelId::new("test/model"),
            parent_session_id: None,
            forked_at: None,
            session_format_version: crate::session::persistence::SESSION_FORMAT_VERSION,
            prompt_display_cwd: None,
            session_kind: None,
            fork_context_source: None,
            fork_parent_prompt_id: None,
            hidden: None,
            source_workspace_dir: None,
            git_root_dir: None,
            git_remotes: Vec::new(),
            head_commit: None,
            head_branch: None,
            grow_home: None,
            last_active_at: None,
            worktree_label: None,
            agent_name: None,
            sandbox_profile: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn listing_filters_and_keeps_only_newest_empty_session_per_cwd() {
        let sessions = list_from_summaries(
            vec![
                summary("match", "/repo", "needle", 1),
                summary("empty-a", "/empty", "needle", 0),
                summary("empty-b", "/empty/", "needle", 0),
                summary("other", "/repo", "haystack", 1),
            ],
            Some("needle"),
            10,
        );
        assert_eq!(sessions.len(), 2);
    }
}
