//! Local session listing shared by the ACP session picker and `grow sessions`.

use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::Datelike;
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
    let mut summaries: Vec<Summary> = local
        .into_iter()
        .filter(|summary| {
            query.as_ref().is_none_or(|query| {
                summary.info.id.to_string().to_lowercase().contains(query)
                    || summary.display_title().to_lowercase().contains(query)
            })
        })
        .collect();

    // Cache each typed key once. RFC3339 parsing in the previous algorithm
    // rejects signed or expanded years, so those timestamps remain None here.
    summaries.sort_by_cached_key(|summary| {
        (
            Reverse(effective_sort_time(summary)),
            summary.info.id.0.clone(),
        )
    });

    let mut seen_empty_cwds = HashSet::new();
    summaries.retain(|summary| {
        summary.num_messages != 0 || seen_empty_cwds.insert(normalize_cwd(&summary.info.cwd))
    });
    summaries.truncate(limit);
    summaries.into_iter().map(summary_to_session).collect()
}

fn effective_sort_time(summary: &Summary) -> Option<chrono::DateTime<chrono::Utc>> {
    rfc3339_sort_time(summary.last_active_at.as_ref())
        .or_else(|| rfc3339_sort_time(Some(&summary.updated_at)))
}

fn rfc3339_sort_time(
    time: Option<&chrono::DateTime<chrono::Utc>>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    time.filter(|time| (0..=9999).contains(&time.year()))
        .cloned()
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
    use agent_client_protocol::schema::v1 as acp;
    use chrono::{Duration, TimeZone, Utc};
    use serde_json::Value;

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
            current_model_id: crate::agent::models::ModelId::new("test/model"),
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

    fn old_listing_reference(
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
            let effective_time = session
                .last_active_at
                .as_deref()
                .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                .or_else(|| chrono::DateTime::parse_from_rfc3339(&session.updated_at).ok());
            (Reverse(effective_time), session.session_id.clone())
        });
        let mut seen_empty_cwds = HashSet::new();
        sessions.retain(|session| {
            session.num_messages != 0 || {
                let trimmed = session.cwd.trim_end_matches('/');
                let cwd = if trimmed.is_empty() {
                    "/".to_owned()
                } else {
                    trimmed.replace("/./", "/")
                };
                seen_empty_cwds.insert(cwd)
            }
        });
        sessions.truncate(limit);
        sessions
    }

    fn serialized(sessions: &[SessionListing]) -> Vec<Value> {
        sessions
            .iter()
            .map(|session| serde_json::to_value(session).unwrap())
            .collect()
    }

    fn differential_fixture() -> Vec<Summary> {
        let tie = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap() + Duration::nanoseconds(123);
        let at_year = |year| Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0).unwrap();
        let mut summaries = Vec::new();

        let mut newest = summary("newest", "/repo", "会话 newest", 2);
        newest.last_active_at = Some(tie + Duration::nanoseconds(2));
        summaries.push(newest);

        // Two rows intentionally share the complete old sort key. Stable sort
        // must preserve this input order even though the projected rows differ.
        for title in ["会话 duplicate-first", "会话 duplicate-second"] {
            let mut duplicate = summary("same-key", "/repo", title, 1);
            duplicate.last_active_at = Some(tie);
            duplicate.updated_at = tie;
            summaries.push(duplicate);
        }

        let mut fallback = summary("fallback", "/repo", "needle fallback", 1);
        fallback.updated_at = tie + Duration::nanoseconds(1);
        summaries.push(fallback);

        let mut empty_old = summary("empty-old", "/empty", "needle empty old", 0);
        empty_old.last_active_at = Some(tie - Duration::seconds(2));
        summaries.push(empty_old);
        let mut empty_new = summary("empty-new", "/empty/", "needle empty new", 0);
        empty_new.last_active_at = Some(tie - Duration::seconds(1));
        summaries.push(empty_new);
        let mut empty_dot = summary("empty-dot", "/empty/./", "needle empty dot", 0);
        empty_dot.last_active_at = Some(tie);
        summaries.push(empty_dot);
        summaries.push(summary("empty-root-a", "", "needle root a", 0));
        summaries.push(summary("empty-root-b", "///", "needle root b", 0));

        // Non-empty sessions sharing a cwd are not deduplicated.
        summaries.push(summary("active-a", "/same-cwd/", "needle active a", 1));
        summaries.push(summary("active-b", "/same-cwd", "needle active b", 2));
        summaries.push(summary("id-query-target", "/repo", "unrelated title", 1));

        let mut min_utc = summary("min-utc", "/repo", "minimum UTC", 1);
        min_utc.updated_at = chrono::DateTime::<Utc>::MIN_UTC;
        min_utc.last_active_at = Some(chrono::DateTime::<Utc>::MIN_UTC);
        summaries.push(min_utc);
        let mut max_utc = summary("max-utc", "/repo", "maximum UTC", 1);
        max_utc.updated_at = chrono::DateTime::<Utc>::MAX_UTC;
        max_utc.last_active_at = Some(chrono::DateTime::<Utc>::MAX_UTC);
        summaries.push(max_utc);

        let mut year_minus_one = summary("year-minus-one", "/repo", "invalid last active", 1);
        year_minus_one.last_active_at = Some(at_year(-1));
        year_minus_one.updated_at = tie + Duration::seconds(1);
        summaries.push(year_minus_one);
        let mut year_zero = summary("year-zero", "/repo", "year zero", 1);
        year_zero.last_active_at = Some(at_year(0));
        summaries.push(year_zero);
        let mut year_9999 = summary("year-9999", "/repo", "year 9999", 1);
        year_9999.last_active_at = Some(at_year(9999));
        summaries.push(year_9999);
        let mut leap_second = summary("leap-second", "/repo", "leap second", 1);
        leap_second.last_active_at = Some(Utc.timestamp_opt(1_483_228_799, 1_500_000_000).unwrap());
        summaries.push(leap_second);
        let mut year_10000 = summary("year-10000", "/repo", "year 10000", 1);
        year_10000.updated_at = at_year(10000);
        year_10000.last_active_at = Some(at_year(10000));
        summaries.push(year_10000);
        summaries
    }

    #[test]
    fn listing_matches_old_algorithm_for_sort_query_dedup_and_limit_boundaries() {
        let summaries = differential_fixture();
        for query in [None, Some("NEEDLE"), Some("same-key"), Some("会话")] {
            for limit in [0, 1, 2, 5, 12, usize::MAX] {
                let expected = old_listing_reference(summaries.clone(), query, limit);
                let actual = list_from_summaries(summaries.clone(), query, limit);
                assert_eq!(
                    serialized(&actual),
                    serialized(&expected),
                    "query={query:?}, limit={limit}"
                );
            }
        }

        let all = list_from_summaries(summaries, None, usize::MAX);
        let same_key_titles: Vec<_> = all
            .iter()
            .filter(|session| session.session_id == "same-key")
            .map(|session| session.title.as_str())
            .collect();
        assert_eq!(
            same_key_titles,
            ["会话 duplicate-first", "会话 duplicate-second"],
            "stable ordering preserves input order for exact sort-key ties"
        );
        assert_eq!(
            all.iter()
                .filter(|session| session.cwd.trim_end_matches('/') == "/empty")
                .count(),
            1,
            "only the newest empty session for normalized /empty is retained"
        );
        assert_eq!(
            all.iter()
                .filter(|session| session.cwd.trim_end_matches('/') == "/same-cwd")
                .count(),
            2,
            "non-empty sessions sharing a cwd remain visible"
        );
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
