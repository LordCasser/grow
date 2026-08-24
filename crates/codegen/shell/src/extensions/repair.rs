//! `grow/session/repair` — out-of-band recovery for sessions bricked by
//! corrupted tool-pairing history.
//!
//! A `ToolResult` whose owning assistant `tool_call` is missing (e.g. a
//! malformed historical import) makes every request
//! 400 with "unexpected `tool_use_id` found in `tool_result` blocks". No
//! in-band path can recover — compaction's sanitizer needs a model call that
//! itself 400s — so the client invokes this method against the session.
//!
//! Repairs via [`chat_state::compaction_utils::repair_history`]. Resident
//! sessions go through `SessionCommand::RepairHistory` (serialized with
//! session activity, rejected mid-turn); non-resident sessions append the same
//! recovery intent and Surface replacement directly to Timeline.

use agent_client_protocol as acp;
use chat_state::compaction_utils::HistoryRepairReport;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use super::{ExtResult, parse_params, to_raw_response};
use crate::agent::MvpAgent;
use crate::session::SessionCommand;
use crate::session::storage::StorageAdapter;
use crate::session::storage::jsonl::JsonlStorageAdapter;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepairSessionRequest {
    session_id: String,
    /// Report what would change without mutating memory or disk.
    #[serde(default)]
    dry_run: bool,
}

/// Response payload for `grow/session/repair`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepairSessionResponse {
    /// Whether the repair modified (or, for `dryRun`, would modify) the history.
    pub repaired: bool,
    /// Echo of the request's `dryRun` flag.
    pub dry_run: bool,
    /// Whether the session was resident (repaired via the live actor) or
    /// repaired directly on disk.
    pub resident: bool,
    /// Duplicate `ToolResult` entries removed.
    pub duplicates_removed: usize,
    /// `tool_call_id`s of orphaned/displaced `ToolResult`s stripped.
    pub stripped_tool_result_ids: Vec<String>,
    /// Synthetic `ToolResult`s inserted for unanswered tool calls.
    pub synthetic_results_inserted: usize,
}

impl RepairSessionResponse {
    fn new(report: HistoryRepairReport, dry_run: bool, resident: bool) -> Self {
        Self {
            repaired: report.changed(),
            dry_run,
            resident,
            duplicates_removed: report.duplicates_removed,
            stripped_tool_result_ids: report.stripped_tool_result_ids,
            synthetic_results_inserted: report.synthetic_results_inserted,
        }
    }
}

#[tracing::instrument(skip_all, fields(method = %args.method))]
pub async fn handle(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    match args.method.as_ref() {
        "grow/session/repair" => handle_session_repair(agent, args).await,
        _ => Err(acp::Error::method_not_found()),
    }
}

async fn handle_session_repair(agent: &MvpAgent, args: &acp::ExtRequest) -> ExtResult {
    let req: RepairSessionRequest = parse_params(args)?;
    let session_id = acp::SessionId::new(req.session_id.as_str());

    // Resident rail. The load-waiting lookup keeps a repair racing a
    // reconnect replay from falling through to the disk rail.
    if let Some(handle) = agent.session_handle_waiting_for_load(&session_id).await {
        let (tx, rx) = oneshot::channel();
        handle
            .cmd_tx
            .send(SessionCommand::RepairHistory {
                dry_run: req.dry_run,
                respond_to: tx,
            })
            .map_err(|_| acp::Error::internal_error().data("failed to send repair command"))?;
        let report = rx
            .await
            .map_err(|_| acp::Error::internal_error().data("session failed to respond"))?
            .map_err(|e| acp::Error::internal_error().data(format!("repair failed: {e}")))?;
        return to_raw_response(&RepairSessionResponse::new(report, req.dry_run, true));
    }

    // Disk rail: session not resident — append a canonical Timeline repair.
    repair_on_disk(
        &crate::util::grow_home::grow_home(),
        &req.session_id,
        req.dry_run,
    )
    .await
}

/// Repair a non-resident session by folding Timeline, then appending recovery
/// intent and replacement events. `grow_root` is injectable for tests.
async fn repair_on_disk(grow_root: &std::path::Path, session_id: &str, dry_run: bool) -> ExtResult {
    let summary = crate::session::persistence::find_summary_by_session_id_in_root(
        session_id,
        &grow_root.join("sessions"),
    )
    .ok_or_else(|| {
        acp::Error::resource_not_found(Some(format!("session not found: {session_id}")))
    })?;
    let info = summary.info.clone();

    let storage = JsonlStorageAdapter::with_root(grow_root.to_path_buf());
    let persisted = storage
        .load_session_without_updates(&info)
        .await
        .map_err(|e| {
            acp::Error::internal_error().data(format!("failed to load session history: {e}"))
        })?;
    let mut timeline =
        chat_state::Timeline::from_events(persisted.timeline_events).map_err(|error| {
            acp::Error::internal_error().data(format!("invalid session Timeline: {error}"))
        })?;
    let (report, repair_events) = timeline.repair_surface_history().map_err(|error| {
        acp::Error::internal_error().data(format!("failed to build Timeline repair: {error}"))
    })?;

    if report.changed() && !dry_run {
        for event in repair_events {
            storage
                .append_timeline_event_durable(&info, &event)
                .await
                .map_err(|error| {
                    acp::Error::internal_error()
                        .data(format!("failed to commit Timeline repair: {error}"))
                })?;
        }
        tracing::warn!(
            session_id,
            duplicates_removed = report.duplicates_removed,
            stripped_tool_result_ids = ?report.stripped_tool_result_ids,
            synthetic_results_inserted = report.synthetic_results_inserted,
            "session Timeline repaired on disk"
        );
    }

    to_raw_response(&RepairSessionResponse::new(report, dry_run, false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sampling::ConversationItem;
    use crate::session::info::Info;
    use crate::session::persistence::default_model_id;
    use sampling_types::ToolCall;
    use tempfile::TempDir;

    const SESSION_ID: &str = "019f3df7-3d70-7f60-8ca0-a38d2d005670";

    /// Seed `{root}/sessions/{cwd}/{id}/` with a summary and canonical Timeline.
    async fn seed_session(
        root: &std::path::Path,
        items: &[ConversationItem],
    ) -> (JsonlStorageAdapter, Info) {
        let adapter = JsonlStorageAdapter::with_root(root.to_path_buf());
        let info = Info {
            id: acp::SessionId::new(SESSION_ID),
            cwd: "/work".to_string(),
        };
        adapter
            .init_session(&info, default_model_id())
            .await
            .expect("init session");
        let timeline = chat_state::Timeline::from_seed(items.to_vec()).unwrap();
        for event in timeline.events() {
            adapter
                .append_timeline_event(&info, event)
                .await
                .expect("append Timeline event");
        }
        (adapter, info)
    }

    /// The bricked-session shape: the assistant line owning `call_LOST` is
    /// gone (torn/merged JSONL line skipped on load), leaving an orphaned
    /// tool result that 400s on every request.
    fn corrupted_history() -> Vec<ConversationItem> {
        vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            ConversationItem::tool_result("call_LOST", "orphaned result"),
            ConversationItem::assistant_tool_calls(vec![ToolCall {
                id: "call_OK".into(),
                name: "read_file".to_string(),
                arguments: "{}".into(),
            }]),
            ConversationItem::tool_result("call_OK", "fine"),
        ]
    }

    fn parse(resp: &acp::ExtResponse) -> serde_json::Value {
        serde_json::from_str(resp.0.get()).expect("repair response json")
    }

    #[tokio::test]
    async fn disk_repair_strips_orphaned_result_and_rewrites_file() {
        let tmp = TempDir::new().unwrap();
        let (adapter, info) = seed_session(tmp.path(), &corrupted_history()).await;
        drop(adapter);

        let resp = repair_on_disk(tmp.path(), SESSION_ID, false)
            .await
            .expect("repair ok");
        let v = parse(&resp);
        assert_eq!(v["repaired"], true);
        assert_eq!(v["resident"], false);
        assert_eq!(v["dryRun"], false);
        assert_eq!(v["strippedToolResultIds"], serde_json::json!(["call_LOST"]));
        assert_eq!(v["duplicatesRemoved"], 0);
        assert_eq!(v["syntheticResultsInserted"], 0);

        // The rewritten file must reload as a valid conversation with the
        // orphan gone and the intact pair preserved.
        let reloaded = JsonlStorageAdapter::with_root(tmp.path().to_path_buf())
            .load_session_without_updates(&info)
            .await
            .expect("reload");
        let reloaded = chat_state::Timeline::from_events(reloaded.timeline_events)
            .expect("valid repaired Timeline")
            .surface()
            .to_vec();
        assert_eq!(reloaded.len(), 4);
        assert!(!reloaded.iter().any(|i| matches!(
            i,
            ConversationItem::ToolResult(tr) if tr.tool_call_id == "call_LOST"
        )));

        // A second repair is a no-op: the corruption is really gone.
        let v2 = parse(
            &repair_on_disk(tmp.path(), SESSION_ID, false)
                .await
                .expect("second repair ok"),
        );
        assert_eq!(v2["repaired"], false);
    }

    #[tokio::test]
    async fn disk_repair_dry_run_reports_without_writing() {
        let tmp = TempDir::new().unwrap();
        let (adapter, info) = seed_session(tmp.path(), &corrupted_history()).await;

        let v = parse(
            &repair_on_disk(tmp.path(), SESSION_ID, true)
                .await
                .expect("dry run ok"),
        );
        assert_eq!(v["repaired"], true);
        assert_eq!(v["dryRun"], true);
        assert_eq!(v["strippedToolResultIds"], serde_json::json!(["call_LOST"]));

        // Disk untouched: the orphan is still there.
        let reloaded = adapter
            .load_session_without_updates(&info)
            .await
            .expect("reload");
        let reloaded = chat_state::Timeline::from_events(reloaded.timeline_events)
            .expect("valid original Timeline")
            .surface()
            .to_vec();
        assert_eq!(reloaded.len(), 5);
    }

    #[tokio::test]
    async fn disk_repair_noop_on_valid_history() {
        let tmp = TempDir::new().unwrap();
        let valid = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prompt"),
            ConversationItem::assistant("done"),
        ];
        seed_session(tmp.path(), &valid).await;

        let v = parse(
            &repair_on_disk(tmp.path(), SESSION_ID, false)
                .await
                .expect("repair ok"),
        );
        assert_eq!(v["repaired"], false);
    }

    #[tokio::test]
    async fn disk_repair_unknown_session_is_resource_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = repair_on_disk(tmp.path(), "no-such-session", false)
            .await
            .expect_err("must fail");
        assert_eq!(
            err.code,
            acp::Error::resource_not_found(None::<String>).code
        );
    }
}
