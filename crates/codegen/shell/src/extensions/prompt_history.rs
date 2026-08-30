//! `grow/prompt_history` extension handler.
//!
//! Timeline is the only durable source. This module performs bounded,
//! read-only projections for the pager and command suggestion provider; it
//! never writes or repairs a separate history index.

use std::collections::HashSet;
use std::io;

use acp_transport::protocol as acp;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::persistence::{Summary, list_summaries};
use crate::session::storage::StorageAdapter;
use crate::session::storage::jsonl::JsonlStorageAdapter;
use crate::timed;

const MAX_CONCURRENT_READS: usize = 32;
const MAX_HISTORY_SESSIONS: usize = 256;
const MAX_HISTORY_ENTRIES: usize = 10_000;

#[derive(Deserialize)]
struct PromptHistoryRequest {
    cwd: String,
    /// Restricts the canonical projection to one session when present.
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Serialize)]
struct PromptHistoryResponse {
    prompts: Vec<String>,
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(_agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/prompt_history" => handle_prompt_history(args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_prompt_history(args: &acp::ExtRequest) -> ExtResult {
    let request: PromptHistoryRequest = parse_params(args)?;
    let prompts = timed!(try: "prompt_history: project timeline", async {
        tracing::debug!(
            cwd = request.cwd,
            session_id = ?request.session_id,
            "projecting prompt history from Timeline"
        );
        load_prompts(&request.cwd, request.session_id.as_deref())
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("failed to project prompt history: {error}"))
            })
    })?;

    tracing::debug!(
        count = prompts.len(),
        cwd = request.cwd,
        "projected prompt history"
    );
    to_raw_response(&PromptHistoryResponse { prompts })
}

/// Project user-authored inputs in most-recent-first order.
pub(crate) async fn load_prompts(cwd: &str, session_id: Option<&str>) -> io::Result<Vec<String>> {
    let records = load_records(
        Some(cwd),
        session_id,
        MAX_HISTORY_SESSIONS,
        MAX_HISTORY_ENTRIES,
    )
    .await?;
    Ok(deduplicate(
        records.into_iter().map(|record| record.text),
        MAX_HISTORY_ENTRIES,
    ))
}

/// Project direct Bash inputs in most-recent-first order.
pub(crate) async fn load_bash_prompts(
    cwd: Option<&str>,
    max_sessions: usize,
    max_entries: usize,
) -> io::Result<Vec<String>> {
    let records = load_records(cwd, None, max_sessions, max_entries).await?;
    Ok(deduplicate(
        records
            .into_iter()
            .filter(|record| record.input_kind == chat_state::TurnInputKind::Bash)
            .map(|record| record.text),
        max_entries,
    ))
}

async fn load_records(
    cwd: Option<&str>,
    session_id: Option<&str>,
    max_sessions: usize,
    max_entries: usize,
) -> io::Result<Vec<chat_state::PromptRecord>> {
    let root_dir = crate::util::grow_home::grow_home();
    let storage = JsonlStorageAdapter::with_root(root_dir);
    let mut summaries = load_summaries(&storage, cwd, max_sessions).await?;
    if let Some(target) = session_id {
        summaries.retain(|summary| summary.info.id.0.as_ref() == target);
    }
    summaries.truncate(max_sessions);

    let batches = stream::iter(summaries)
        .map(|summary| {
            let storage = storage.clone();
            async move { storage.load_prompt_records(&summary.info).await }
        })
        .buffered(MAX_CONCURRENT_READS)
        .collect::<Vec<_>>()
        .await;

    let mut records = Vec::new();
    for batch in batches {
        let mut batch = batch?;
        batch.reverse();
        let remaining = max_entries.saturating_sub(records.len());
        records.extend(batch.into_iter().take(remaining));
        if records.len() >= max_entries {
            break;
        }
    }
    Ok(records)
}

async fn load_summaries(
    storage: &JsonlStorageAdapter,
    cwd: Option<&str>,
    max_sessions: usize,
) -> io::Result<Vec<Summary>> {
    match cwd {
        Some(cwd) => list_summaries(Some(cwd)).await,
        None => storage.list_sessions_recent(max_sessions).await,
    }
}

fn deduplicate(prompts: impl IntoIterator<Item = String>, max_entries: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    prompts
        .into_iter()
        .filter(|prompt| seen.insert(prompt.clone()))
        .take(max_entries)
        .collect()
}
