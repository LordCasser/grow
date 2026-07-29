use chrono::{DateTime, Utc};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::sampling::Client as OaiCompatClient;
use crate::sampling::ConversationItem;
use grow_workspace::session::file_state::RewindPoint;

use crate::session::signals::SessionSignals;
use crate::session::storage::relocation::{RelocationError, RelocationView};
use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};
use crate::tools::todo::TodoState;
use crate::util::grow_home::grow_home;
use agent_client_protocol as acp;
use grow_sampling_types::ReasoningEffort;
use xai_acp_lib::AcpAgentGatewaySender as GatewaySender;

use crate::session::info::Info;
use tokio::sync::mpsc;

/// Current chat history format version.
/// - Version 0: Legacy ChatRequestMessage format (default for old sessions)
/// - Version 1: ConversationItem format (used for new sessions)
pub const CHAT_FORMAT_VERSION: u8 = 1;

#[derive(Debug, Clone)]
pub struct PersistenceContentChunk {
    content_chunks: Vec<acp::ContentBlock>,
}

impl PersistenceContentChunk {
    pub fn new(content_chunks: Vec<acp::ContentBlock>) -> Self {
        Self { content_chunks }
    }
}

use crate::session::storage::SessionUpdate;
use serde::{Deserialize, Serialize};

// /btw side question persistence types

/// A single /btw side question entry persisted to `btw_history.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtwEntry {
    /// Unique ID for this side question.
    pub btw_session_id: String,
    /// The parent session ID.
    pub parent_session_id: String,
    /// When the question was asked.
    pub asked_at: DateTime<Utc>,
    /// The user's question.
    pub question: String,
    /// The model's response (empty if failed).
    pub answer: String,
    /// Model used.
    pub model: String,
    /// Whether the request succeeded.
    pub success: bool,
    /// Error message if failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum PersistenceMsg {
    /// A session update (ACP update or Grow extension update)
    Update(SessionUpdate),
    AppendUpdateDurablyAndAck {
        update: SessionUpdate,
        respond_to:
            tokio::sync::oneshot::Sender<Result<(), crate::session::storage::AppendUpdateError>>,
    },
    ContentChunk(PersistenceContentChunk),
    Chat(ConversationItem),
    AppendCwdSwitchAndAck {
        item: ConversationItem,
        respond_to: tokio::sync::oneshot::Sender<
            Result<xai_chat_state::StrictAppendAck, xai_chat_state::StrictAppendError>,
        >,
    },
    /// Replace the entire chat history (used for compaction)
    ReplaceChatHistory(Vec<ConversationItem>),
    CurrentModel {
        model_id: acp::ModelId,
        /// The active agent definition name (e.g. `"grow-build"`).
        /// Persisted in `summary.agent_name` so session resume doesn't depend
        /// on the mutable model catalog.
        agent_name: Option<String>,
        reasoning_effort: Option<Option<ReasoningEffort>>,
    },
    PlanState(TodoState),
    /// Plan mode lifecycle state to persist
    PlanModeState(crate::session::plan_mode::PlanModeSnapshot),
    /// A rewind point to persist
    RewindPoint(RewindPoint),
    /// Truncate rewind points from a specific prompt index (inclusive).
    /// Syncs the persisted file with the in-memory FileStateTracker after rewind.
    TruncateRewindPoints {
        from_index: usize,
    },
    /// Merge rewind points at indices >= `target_index` into the previous point
    /// (read-modify-write on disk, after a ConversationOnly rewind). Disk is
    /// authoritative, so a partial in-memory tracker can't truncate history.
    MergeRewindPointsFrom {
        target_index: usize,
    },
    /// Persist a snapshot of the session signals.
    Signals(SessionSignals),
    /// Persist announcement tracking state (MCP + skill announcement dedup).
    AnnouncementState(crate::session::announcement_state::AnnouncementState),
    /// Persist goal mode orchestration state.
    GoalModeState(crate::session::goal_tracker::GoalOrchestration),
    DeleteGoalModeState {
        respond_to: tokio::sync::oneshot::Sender<io::Result<()>>,
    },
    WorkflowRunState(crate::session::workflow::store::WorkflowRunManifest),
    WorkflowRunStateAndAck {
        manifest: crate::session::workflow::store::WorkflowRunManifest,
        respond_to: tokio::sync::oneshot::Sender<io::Result<()>>,
    },
    DeleteWorkflowRunState(String),
    /// Persist a /btw side question entry
    Btw(BtwEntry),
    /// Persist updated HEAD commit and branch to summary.
    GitHead {
        commit: Option<String>,
        branch: Option<String>,
    },
    /// Persist a compaction checkpoint file to `compaction_checkpoints/{id}.json`.
    CompactionCheckpoint(crate::extensions::notification::CompactionCheckpointFile),
    /// Persist a compaction request+response artifact to
    /// `compaction_requests/{request_id}.json`. Used for offline prompt
    /// iteration — captures the exact ConversationItem list sent to the
    /// compaction model plus the summary it returned (or the final error).
    /// Stored under the local session directory for offline diagnostics.
    CompactionRequest(crate::extensions::notification::CompactionRequestFile),
    /// Persist a recap request+response artifact to
    /// `recap_requests/{request_id}.json` for offline recap prompt / garble replay.
    RecapRequest(crate::extensions::notification::RecapRequestFile),
    /// Persist a compaction segment (`Segments` mode).
    CompactionSegment(crate::extensions::notification::CompactionSegmentFile),
    /// Generated session title from background LLM task.
    /// Routed back through the persistence channel so the storage write
    /// stays sequential with other summary.json mutations.
    GeneratedTitle(String),
    Flush,
    /// Flush all pending writes, then signal the caller once the flush is complete.
    /// Unlike `Flush` (fire-and-forget), this is a **sync barrier**: the caller's
    /// oneshot only resolves after `flush_pending()` finishes writing to disk.
    FlushAndAck {
        respond_to: tokio::sync::oneshot::Sender<()>,
    },
}

pub use grow_shared::session::session_dir;

type RelocationResult<T> = crate::session::storage::relocation::Result<T>;
type SummaryReader = fn(&Path) -> RelocationResult<Summary>;

fn storage_view(sessions_root: &Path) -> RelocationResult<RelocationView> {
    RelocationView::load_for_sessions_root(sessions_root)
}

/// Check if a session exists locally under the given cwd.
///
/// A session is considered present here only when it lives under the exact cwd.
pub fn session_exists_for_cwd(session_id: &str, cwd: &str) -> bool {
    let sessions_root = crate::util::grow_home::grow_home().join("sessions");
    session_exists_for_cwd_in_root(session_id, cwd, &sessions_root)
}

/// A directory is a resumable session only if it has a `summary.json`; this
/// skips `images/`-only stubs that would otherwise hijack `--resume`. Used by
/// the resume/restore resolution path; `find_session_dir_by_id` intentionally
/// stays dir-only for non-resume compatibility.
fn is_persisted_session_dir(session_path: &Path) -> bool {
    session_path.join("summary.json").is_file()
}

/// Inner implementation of `session_exists_for_cwd` with an injectable root.
/// Separated for deterministic tempdir-based tests.
fn session_exists_for_cwd_in_root(session_id: &str, cwd: &str, sessions_root: &Path) -> bool {
    let encoded = crate::util::grow_home::encode_cwd_dirname(cwd);
    let session_path = sessions_root.join(&encoded).join(session_id);
    is_persisted_session_dir(&session_path)
}

/// Resolve a session only when it exists in the requested local cwd.
pub fn resolve_local_session(session_id: &str, cwd: &str) -> Option<String> {
    session_exists_for_cwd(session_id, cwd).then(|| session_id.to_owned())
}

// Repo-wide session resolution (for worktree resume)

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalSessionResolutionKind {
    ExactCwd,
    SameRepoDifferentCwd,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLocalSession {
    pub session_id: String,
    pub cwd: String,
    pub resolution_kind: LocalSessionResolutionKind,
}

/// Resolve a session across multiple candidate cwds for worktree resume.
///
/// The first cwd in `candidate_cwds` should be the exact current cwd so it
/// gets priority. For each candidate, checks both direct session existence
/// and previously-restored children.
///
/// Returns `None` when no local match exists in any candidate.
pub fn resolve_local_session_for_repo(
    session_id: &str,
    candidate_cwds: &[&str],
) -> Option<ResolvedLocalSession> {
    let sessions_root = crate::util::grow_home::grow_home().join("sessions");
    resolve_local_session_for_repo_in_root(session_id, candidate_cwds, &sessions_root)
}

pub fn resolve_local_session_for_repo_in_root(
    session_id: &str,
    candidate_cwds: &[&str],
    sessions_root: &Path,
) -> Option<ResolvedLocalSession> {
    for (i, &cwd) in candidate_cwds.iter().enumerate() {
        let is_exact = i == 0;

        if session_exists_for_cwd_in_root(session_id, cwd, sessions_root) {
            return Some(ResolvedLocalSession {
                session_id: session_id.to_owned(),
                cwd: cwd.to_owned(),
                resolution_kind: if is_exact {
                    LocalSessionResolutionKind::ExactCwd
                } else {
                    LocalSessionResolutionKind::SameRepoDifferentCwd
                },
            });
        }
    }
    None
}

/// Check if a session exists locally by session ID.
/// Searches across ALL cwd directories under `~/.grow/sessions/`.
///
/// Use `session_exists_for_cwd` instead when the target cwd is known
/// (e.g., the `-r` resume path) to avoid false-positive matches.
/// Find a session by ID across **all** CWD directories under `~/.grow/sessions/`.
///
/// Unlike [`resolve_local_session`] which only checks a single CWD,
/// this scans every encoded-CWD subdirectory. Returns the decoded CWD path
/// that contains the session, or `None` if not found anywhere.
///
/// This is used by the pager's `--resume` to find sessions that were created
/// in a different CWD (e.g., a worktree) than the one the user is currently in.
pub fn resolve_local_session_any_cwd(session_id: &str) -> Option<String> {
    resolve_local_session_any_cwd_result(session_id)
        .ok()
        .flatten()
}

pub fn resolve_local_session_any_cwd_result(session_id: &str) -> io::Result<Option<String>> {
    resolve_local_session_any_cwd_in_root(session_id, &grow_home().join("sessions"))
        .map_err(io::Error::other)
}

fn resolve_local_session_any_cwd_in_root(
    session_id: &str,
    sessions_root: &Path,
) -> Result<Option<String>, crate::session::storage::relocation::RelocationError> {
    let Some(session_path) = storage_view(sessions_root)?.find_persisted_session_dir(session_id)?
    else {
        return Ok(None);
    };
    Ok(session_path
        .parent()
        .and_then(crate::util::grow_home::decode_cwd_from_dirname))
}

/// Scan all CWD directories for a session and return its directory path.
pub fn find_session_dir_by_id(session_id: &str) -> Option<PathBuf> {
    find_any_session_dir_by_id_result(session_id).ok().flatten()
}

pub(crate) fn find_persisted_session_dir_by_id_result(
    session_id: &str,
) -> io::Result<Option<PathBuf>> {
    find_persisted_session_dir_by_id_in_root_result(session_id, &grow_home().join("sessions"))
}

pub(crate) fn find_persisted_session_dir_by_id_in_root_result(
    session_id: &str,
    sessions_root: &Path,
) -> io::Result<Option<PathBuf>> {
    storage_view(sessions_root)
        .and_then(|view| view.find_persisted_session_dir(session_id))
        .map_err(io::Error::other)
}

pub(crate) fn find_any_session_dir_by_id_result(session_id: &str) -> io::Result<Option<PathBuf>> {
    storage_view(&grow_home().join("sessions"))
        .and_then(|view| view.find_any_session_dir(session_id))
        .map_err(io::Error::other)
}

#[cfg(test)]
fn session_exists_in_root(session_id: &str, sessions_root: &Path) -> bool {
    find_persisted_session_dir_by_id_in_root_result(session_id, sessions_root)
        .is_ok_and(|path| path.is_some())
}

/// Find and read a session summary given only its ID (scans all CWD directories).
pub fn find_summary_by_session_id(session_id: &str) -> Option<Summary> {
    find_summary_by_session_id_in_root(session_id, &grow_home().join("sessions"))
}

/// Inner implementation with injectable root for testing.
pub(crate) fn find_summary_by_session_id_in_root(
    session_id: &str,
    sessions_root: &Path,
) -> Option<Summary> {
    let path = storage_view(sessions_root)
        .ok()?
        .find_persisted_session_dir(session_id)
        .ok()
        .flatten()?;
    read_summary_from_dir(&path).ok()
}

fn read_summary_from_dir(session_dir: &Path) -> RelocationResult<Summary> {
    let path = session_dir.join("summary.json");
    let bytes = std::fs::read(&path).map_err(|error| RelocationError::Io {
        operation: "read",
        path: path.clone(),
        source: error,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| RelocationError::Json { path, source })
}

/// The most recently updated local session summary for `cwd` (by
/// `last_active_at` else `updated_at`), or `None` if there are no local sessions
/// for that cwd. Sync and local-only — suitable for the startup path that must
/// resolve the sandbox profile before the (irreversible) OS sandbox is applied.
fn most_recent_local_summary_for_cwd_in_root(cwd: &str, sessions_root: &Path) -> Option<Summary> {
    most_recent_local_summary_for_cwd_in_view(
        cwd,
        &storage_view(sessions_root).ok()?,
        read_summary_from_dir,
    )
    .ok()
    .flatten()
}

fn most_recent_local_summary_for_cwd_in_view(
    cwd: &str,
    view: &RelocationView,
    read_summary: SummaryReader,
) -> RelocationResult<Option<Summary>> {
    let mut best: Option<Summary> = None;
    for session_dir in view.session_dirs(Some(cwd))? {
        let summary = match read_summary(&session_dir) {
            Ok(summary) => summary,
            Err(RelocationError::Json { .. }) => continue,
            Err(RelocationError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                continue;
            }
            Err(error) => return Err(error),
        };
        if summary.is_hidden() {
            continue;
        }
        if best.as_ref().is_none_or(|current| {
            let time = summary.last_active_at.unwrap_or(summary.updated_at);
            let current_time = current.last_active_at.unwrap_or(current.updated_at);
            time > current_time
                || (time == current_time && summary.info.id.0.as_ref() < current.info.id.0.as_ref())
        }) {
            best = Some(summary);
        }
    }
    Ok(best)
}

/// Sync, local-only session summaries for `cwd` (hidden sessions filtered).
/// For startup paths that must resolve a resume target before the
/// irreversible OS sandbox is applied; async callers use [`list_summaries`].
///
/// Listing failures propagate so pre-sandbox callers can fail closed;
/// individual unreadable summaries are skipped, matching the async path's
/// tolerance for a single corrupt file.
pub fn local_summaries_for_cwd_sync(cwd: &str) -> io::Result<Vec<Summary>> {
    local_summaries_for_cwd_sync_in_root(cwd, &grow_home().join("sessions"))
}

fn local_summaries_for_cwd_sync_in_root(
    cwd: &str,
    sessions_root: &Path,
) -> io::Result<Vec<Summary>> {
    let view = storage_view(sessions_root).map_err(io::Error::other)?;
    let dirs = view.session_dirs(Some(cwd)).map_err(io::Error::other)?;
    Ok(dirs
        .iter()
        .filter_map(|dir| read_summary_from_dir(dir).ok())
        .filter(|s| !s.is_hidden())
        .collect())
}

/// Best-effort lookup of the sandbox profile persisted with a session that is
/// about to be resumed, used at startup to restore the session's profile before
/// the (irreversible) OS sandbox is applied.
///
/// - `session_id`: the explicit id from `--resume <id>` / `--load <id>` /
///   `-s <id>`, resolved directly across all local cwd directories.
/// - `cwd`: the lookup key for `-c` / `--continue` and bare `--resume`.
///
/// Returns `None` when not resuming, the session isn't found locally, or it has
/// no persisted profile (sessions created before this was tracked) — callers
/// then fall back to the normal config/CLI resolution.
pub fn resumed_session_sandbox_profile(
    session_id: Option<&str>,
    cwd: Option<&str>,
) -> Option<String> {
    resumed_session_sandbox_profile_in_root(session_id, cwd, &grow_home().join("sessions"))
}

fn resumed_session_sandbox_profile_in_root(
    session_id: Option<&str>,
    cwd: Option<&str>,
    sessions_root: &Path,
) -> Option<String> {
    if let Some(id) = session_id.filter(|s| !s.is_empty()) {
        // Direct match by id (across all cwds).
        if let Some(summary) = find_summary_by_session_id_in_root(id, sessions_root) {
            return summary.sandbox_profile;
        }
        return None;
    }
    if let Some(cwd) = cwd {
        return most_recent_local_summary_for_cwd_in_root(cwd, sessions_root)
            .and_then(|s| s.sandbox_profile);
    }
    None
}

/// Get file path for storing a large prompt.
/// Creates the prompts subdirectory if it doesn't exist.
/// Path format: `{session_dir}/prompts/prompt_{prompt_index}.txt`
pub fn get_prompt_file_path(info: &Info, prompt_index: usize) -> PathBuf {
    let prompts_dir = session_dir(info).join("prompts");
    std::fs::create_dir_all(&prompts_dir).ok();
    prompts_dir.join(format!("prompt_{}.txt", prompt_index))
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingCwdSwitchReminder {
    pub cwd_generation: u64,
    pub previous_cwd: String,
    #[serde(alias = "cwd")]
    pub destination_cwd: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_project_instructions: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub info: Info,
    /// Monotonic generation of the authoritative cwd in `info.cwd`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cwd_generation: u64,
    /// Cwd immediately preceding the current generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_cwd: Option<String>,
    /// Reminder staged for exactly-once append during relocation completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_cwd_switch_reminder: Option<PendingCwdSwitchReminder>,
    /// Latest switch generation reflected in `num_chat_messages` bookkeeping.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cwd_switch_bookkeeping_generation: u64,
    pub session_summary: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub num_messages: usize,
    #[serde(default)]
    pub num_chat_messages: usize,
    pub current_model_id: acp::ModelId,
    /// Parent session ID if this session was forked from another session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// Timestamp when this session was forked (only set for forked sessions)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_at: Option<DateTime<Utc>>,
    /// Chat history format version:
    /// - 0 (default): Legacy ChatRequestMessage format
    /// - 1: ConversationItem format
    #[serde(default)]
    pub chat_format_version: u8,
    /// Stable display path for forked sessions.
    ///
    /// When set, the system prompt's `Workspace Path` and prompt metadata
    /// paths show this value instead of the real worktree/overlay path
    /// (`info.cwd`). Persisted so the override survives session
    /// restore/reload without the caller needing to resend it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_display_cwd: Option<String>,
    /// What created this session: `"fork"`, `"subagent"`, `"subagent_fork"`, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_kind: Option<String>,
    /// How the session's initial context was bootstrapped: `"new"` or `"forked"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_context_source: Option<String>,
    /// The parent prompt/turn ID that triggered this fork.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fork_parent_prompt_id: Option<String>,
    /// Number of conversation items inherited from the parent session.
    /// During compaction, items below this index are preserved as-is
    /// (the "inherited prefix"). Only items after this boundary are
    /// summarized. `None` means no inherited prefix (non-forked session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inherited_prefix_len: Option<usize>,
    /// Visibility override. None = default for `session_kind`, Some = explicit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// The original workspace directory this worktree session was spawned from.
    /// Used by clients to group worktree sessions under their source workspace
    /// regardless of the worktree's actual `cwd`. Only set when
    /// `session_kind == "worktree"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_workspace_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_root_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_remotes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_branch: Option<String>,
    /// Absolute path to the `.grow` directory, used by reconstruction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grow_home: Option<String>,
    /// When the session last had content added (user or model messages).
    /// Only advanced locally by `append_update` / `append_chat_message`;
    /// never touched by remote registry operations or metadata-only writes.
    /// `None` for sessions created before this field was added.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<DateTime<Utc>>,
    /// LLM-generated session title persisted separately from `session_summary`.
    /// When present, this is preferred for display over `session_summary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_title: Option<String>,
    /// True when `generated_title` was set by a manual `/rename` (vs auto LLM
    /// title). Manual titles render inline in the prompt's top border on
    /// resume.
    #[serde(default, skip_serializing_if = "is_false")]
    pub title_is_manual: bool,
    /// Human-readable label for the worktree directory (e.g. "nuke-v-tables").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_label: Option<String>,
    /// The agent definition name that was active when the session was last saved.
    /// Used during session resume to avoid re-deriving from the (mutable) model
    /// catalog — if the model is removed or its `agent_type` changes between
    /// sessions, this persisted value ensures the correct harness is restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    /// The OS sandbox profile this session ran under (e.g. "workspace",
    /// "strict", "off", or a custom name). Persisted so a resumed session is
    /// restored to the same profile instead of silently falling back to the
    /// config default — which would otherwise break commands that worked before
    /// (a stricter profile denies filesystem/network the session relied on).
    /// `None` for sessions created before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// Current `grow_home` as a UTF-8 string, or `None` if the path isn't valid UTF-8.
pub fn grow_home_string() -> Option<String> {
    crate::util::grow_home::grow_home()
        .to_str()
        .map(String::from)
}

pub fn default_model_id() -> acp::ModelId {
    acp::ModelId::new(String::new())
}

impl Summary {
    pub fn new(info: &Info, model_id: acp::ModelId) -> std::io::Result<Self> {
        let git_metadata =
            grow_workspace::session::git::resolve_persisted_session_git_metadata_sync(
                std::path::Path::new(&info.cwd),
            );
        Ok(Self {
            info: info.clone(),
            cwd_generation: 0,
            previous_cwd: None,
            pending_cwd_switch_reminder: None,
            cwd_switch_bookkeeping_generation: 0,
            session_summary: String::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            num_messages: 0,
            num_chat_messages: 0,
            current_model_id: model_id,
            parent_session_id: None,
            forked_at: None,
            chat_format_version: CHAT_FORMAT_VERSION,
            prompt_display_cwd: None,
            session_kind: None,
            fork_context_source: None,
            fork_parent_prompt_id: None,
            inherited_prefix_len: None,
            hidden: None,
            source_workspace_dir: None,
            git_root_dir: git_metadata.git_root_dir,
            git_remotes: git_metadata.git_remotes,
            head_commit: git_metadata.head_commit,
            head_branch: git_metadata.head_branch,
            grow_home: grow_home_string(),
            last_active_at: None,
            generated_title: None,
            title_is_manual: false,
            worktree_label: crate::session::worktree::lookup_worktree_label(&info.cwd),
            agent_name: None,
            sandbox_profile: None,
            reasoning_effort: None,
        })
    }

    /// Whether this session should be excluded from history listings.
    pub fn is_hidden(&self) -> bool {
        self.hidden.unwrap_or(
            self.session_kind
                .as_deref()
                .is_some_and(|k| k.starts_with("subagent")),
        )
    }

    /// Preferred display title: `generated_title` if non-empty, else `session_summary`.
    pub fn display_title(&self) -> &str {
        self.generated_title
            .as_deref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or(&self.session_summary)
    }

    /// [`Self::display_title`] as an `Option`, `None` when blank.
    pub fn display_title_opt(&self) -> Option<String> {
        let title = self.display_title().trim();
        (!title.is_empty()).then(|| title.to_string())
    }

    /// The manually-`/rename`d title (trimmed), `None` for auto-generated or
    /// blank titles. Binds to `generated_title` — the field `title_is_manual`
    /// describes — never the `session_summary` display fallback, so a stale
    /// flag over a blank manual title can't relabel an auto summary as
    /// manual. When `Some`, it equals [`Self::display_title_opt`] (a
    /// non-blank `generated_title` wins the display chain).
    pub fn manual_title_opt(&self) -> Option<String> {
        self.title_is_manual
            .then_some(self.generated_title.as_deref())
            .flatten()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    }

    /// Last-change time (unix millis): `last_active_at`, else `updated_at`.
    pub fn last_change_unix_ms(&self) -> i64 {
        self.last_active_at
            .unwrap_or(self.updated_at)
            .timestamp_millis()
    }
}

#[cfg(test)]
mod is_hidden_tests {
    use super::*;

    fn summary_with_kind(kind: Option<&str>) -> Summary {
        Summary {
            session_kind: kind.map(String::from),
            hidden: None,
            ..Summary::new(
                &Info {
                    id: acp::SessionId::new("test"),
                    cwd: "/tmp".into(),
                },
                default_model_id(),
            )
            .unwrap()
        }
    }

    #[test]
    fn summary_round_trips_and_defaults_reasoning_effort() {
        let mut s = summary_with_kind(None);
        s.reasoning_effort = None;
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("reasoning_effort"),
            "a None effort must not be serialized"
        );
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reasoning_effort, None);

        s.reasoning_effort = Some(ReasoningEffort::Xhigh);
        let json = serde_json::to_string(&s).unwrap();
        let back: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.reasoning_effort, Some(ReasoningEffort::Xhigh));
    }

    #[test]
    fn hidden_for_all_subagent_kinds() {
        for kind in ["subagent", "subagent_fork", "subagent_resume"] {
            assert!(
                summary_with_kind(Some(kind)).is_hidden(),
                "{kind} should be hidden"
            );
        }
    }

    #[test]
    fn not_hidden_for_regular_sessions() {
        assert!(!summary_with_kind(None).is_hidden());
        assert!(!summary_with_kind(Some("fork")).is_hidden());
        assert!(!summary_with_kind(Some("worktree")).is_hidden());
    }

    #[test]
    fn explicit_hidden_overrides_session_kind() {
        let mut s = summary_with_kind(Some("subagent"));
        s.hidden = Some(false);
        assert!(!s.is_hidden(), "explicit hidden=false overrides kind");

        let mut s = summary_with_kind(None);
        s.hidden = Some(true);
        assert!(s.is_hidden(), "explicit hidden=true overrides kind");
    }
}

#[cfg(test)]
mod head_fields_tests {
    use super::*;

    #[test]
    fn summary_round_trips_head_fields_through_json() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.head_commit = Some("abc123def456".into());
        summary.head_branch = Some("main".into());

        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: Summary = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.head_commit.as_deref(), Some("abc123def456"));
        assert_eq!(deserialized.head_branch.as_deref(), Some("main"));
    }

    #[test]
    fn summary_deserializes_without_head_fields_backward_compat() {
        // Simulate an old summary.json that lacks head_commit/head_branch.
        let json = r#"{
            "info": { "id": "old-session", "cwd": "/tmp" },
            "session_summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 0,
            "num_chat_messages": 0,
            "current_model_id": "test-model"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert!(summary.head_commit.is_none());
        assert!(summary.head_branch.is_none());
    }

    #[test]
    fn summary_relocation_metadata_is_backward_compatible() {
        let json = r#"{
            "info": { "id": "old-session", "cwd": "/tmp" },
            "session_summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 0,
            "num_chat_messages": 0,
            "current_model_id": "test-model"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.cwd_generation, 0);
        assert!(summary.previous_cwd.is_none());
        assert!(summary.pending_cwd_switch_reminder.is_none());
        assert_eq!(summary.cwd_switch_bookkeeping_generation, 0);

        let serialized = serde_json::to_value(summary).unwrap();
        for field in [
            "cwd_generation",
            "previous_cwd",
            "pending_cwd_switch_reminder",
            "cwd_switch_bookkeeping_generation",
        ] {
            assert!(serialized.get(field).is_none());
        }
    }

    #[test]
    fn summary_relocation_metadata_round_trips() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/new".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.cwd_generation = 2;
        summary.previous_cwd = Some("/old".into());
        summary.pending_cwd_switch_reminder = Some(PendingCwdSwitchReminder {
            cwd_generation: 2,
            previous_cwd: "/old".into(),
            destination_cwd: "/new".into(),
            content: "moved".into(),
            destination_project_instructions: Some("target rules".into()),
        });

        let serialized = serde_json::to_value(&summary).unwrap();
        assert_eq!(
            serialized["pending_cwd_switch_reminder"]["destination_cwd"],
            "/new"
        );
        assert!(
            serialized["pending_cwd_switch_reminder"]
                .get("cwd")
                .is_none()
        );
        let back: Summary = serde_json::from_value(serialized).unwrap();
        assert_eq!(back.cwd_generation, 2);
        assert_eq!(back.previous_cwd.as_deref(), Some("/old"));
        assert_eq!(
            back.pending_cwd_switch_reminder,
            summary.pending_cwd_switch_reminder
        );
        assert_eq!(back.info.cwd, "/new");

        let pending: PendingCwdSwitchReminder = serde_json::from_value(serde_json::json!({
            "cwd_generation": 2,
            "previous_cwd": "/old",
            "cwd": "/new",
            "content": "moved"
        }))
        .unwrap();
        assert_eq!(pending.destination_cwd, "/new");
    }

    #[test]
    fn summary_skips_none_head_fields_in_serialized_json() {
        let summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        // In a non-git directory the fields will be None.
        // Verify they are omitted from the JSON output.
        let json = serde_json::to_string(&summary).unwrap();
        // head_commit should not appear if the cwd has a repo (it might),
        // but verify the skip_serializing_if attribute works for None.
        if summary.head_commit.is_none() {
            assert!(!json.contains("head_commit"));
        }
        if summary.head_branch.is_none() {
            assert!(!json.contains("head_branch"));
        }
    }
}

#[cfg(test)]
mod generated_title_tests {
    use super::*;

    #[test]
    fn summary_round_trips_generated_title_through_json() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.generated_title = Some("Refactor auth middleware".into());
        summary.worktree_label = Some("auth-refactor".into());

        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: Summary = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.generated_title.as_deref(),
            Some("Refactor auth middleware")
        );
        assert_eq!(
            deserialized.worktree_label.as_deref(),
            Some("auth-refactor")
        );
    }

    #[test]
    fn summary_deserializes_without_new_fields_backward_compat() {
        let json = r#"{
            "info": { "id": "old-session", "cwd": "/tmp" },
            "session_summary": "first prompt text",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 5,
            "num_chat_messages": 3,
            "current_model_id": "test-model"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert!(summary.generated_title.is_none());
        assert!(summary.worktree_label.is_none());
        assert_eq!(summary.session_summary, "first prompt text");
    }

    #[test]
    fn summary_skips_none_generated_title_in_json() {
        let summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("generated_title"));
        assert!(!json.contains("worktree_label"));
    }

    #[test]
    fn summary_includes_generated_title_when_set() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.generated_title = Some("Fix K8s deployment".into());
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("generated_title"));
        assert!(json.contains("Fix K8s deployment"));
    }

    #[test]
    fn summary_deserializes_with_all_fields_present() {
        let json = r#"{
            "info": { "id": "full-session", "cwd": "/tmp" },
            "session_summary": "first prompt",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 10,
            "num_chat_messages": 5,
            "current_model_id": "test-model",
            "head_branch": "feature/xyz",
            "git_root_dir": "/home/user/myrepo",
            "generated_title": "Implement XYZ feature",
            "worktree_label": "xyz-feature"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert_eq!(
            summary.generated_title.as_deref(),
            Some("Implement XYZ feature")
        );
        assert_eq!(summary.worktree_label.as_deref(), Some("xyz-feature"));
        assert_eq!(summary.head_branch.as_deref(), Some("feature/xyz"));
        assert_eq!(summary.git_root_dir.as_deref(), Some("/home/user/myrepo"));
    }

    // ── display_title direct tests ──────────────────────────────────────

    #[test]
    fn display_title_returns_generated_title_when_set() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.generated_title = Some("Refactor auth layer".into());
        assert_eq!(summary.display_title(), "Refactor auth layer");
    }

    #[test]
    fn display_title_falls_back_on_empty_generated_title() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.session_summary = "first prompt fallback".into();
        summary.generated_title = Some(String::new());
        assert_eq!(summary.display_title(), "first prompt fallback");
    }

    #[test]
    fn display_title_falls_back_on_none_generated_title() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.session_summary = "session summary fallback".into();
        summary.generated_title = None;
        assert_eq!(summary.display_title(), "session summary fallback");
    }

    // ── title_is_manual / manual_title_opt ──────────────────────────────

    #[test]
    fn title_is_manual_round_trips_through_json() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.generated_title = Some("Manual Title".into());
        summary.title_is_manual = true;

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("title_is_manual"));
        let deserialized: Summary = serde_json::from_str(&json).unwrap();

        assert!(deserialized.title_is_manual);
        assert_eq!(
            deserialized.manual_title_opt().as_deref(),
            Some("Manual Title")
        );
    }

    #[test]
    fn title_is_manual_defaults_false_and_skips_when_unset() {
        // Old summary.json without the field: default false, so pre-existing
        // renames show no border title until renamed again.
        let json = r#"{
            "info": { "id": "old-session", "cwd": "/tmp" },
            "session_summary": "first prompt text",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 5,
            "num_chat_messages": 3,
            "current_model_id": "test-model",
            "generated_title": "Old Rename"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert!(!summary.title_is_manual);
        assert!(summary.manual_title_opt().is_none());
        assert_eq!(summary.display_title_opt().as_deref(), Some("Old Rename"));

        // And false is omitted on write, keeping old files byte-stable.
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains("title_is_manual"));
    }

    #[test]
    fn manual_title_opt_none_for_auto_generated_title() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.generated_title = Some("Auto Title".into());

        assert!(summary.manual_title_opt().is_none());
        assert_eq!(summary.display_title_opt().as_deref(), Some("Auto Title"));
    }

    /// A stale `title_is_manual` over a blank `generated_title` (e.g. written
    /// by an old client before the ext boundary rejected blank renames) must
    /// not relabel the `session_summary` display fallback as manual.
    #[test]
    fn manual_title_opt_ignores_stale_flag_over_blank_generated_title() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.session_summary = "auto first-prompt summary".into();
        summary.generated_title = Some("   ".into());
        summary.title_is_manual = true;

        assert!(summary.manual_title_opt().is_none());
        assert_eq!(
            summary.display_title_opt().as_deref(),
            Some("auto first-prompt summary")
        );
    }
}

#[derive(Clone)]
pub struct PersistenceHandle {
    pub tx: mpsc::UnboundedSender<PersistenceMsg>,
    noop: bool,
}

#[derive(Debug)]
pub enum DurableAppendError {
    NotCommitted(io::Error),
    Committed(io::Error),
    AcknowledgementLost(io::Error),
}

impl std::fmt::Display for DurableAppendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotCommitted(error)
            | Self::Committed(error)
            | Self::AcknowledgementLost(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DurableAppendError {}

impl From<crate::session::storage::AppendUpdateError> for DurableAppendError {
    fn from(error: crate::session::storage::AppendUpdateError) -> Self {
        use crate::session::storage::AppendUpdateError;
        match error {
            AppendUpdateError::NotCommitted(error) => Self::NotCommitted(error),
            AppendUpdateError::Committed(error) => Self::Committed(error),
        }
    }
}

impl PersistenceHandle {
    #[cfg(test)]
    pub(crate) fn from_sender_for_test(tx: mpsc::UnboundedSender<PersistenceMsg>) -> Self {
        Self { tx, noop: false }
    }

    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::unbounded_channel();
        Self { tx, noop: true }
    }

    pub fn is_noop(&self) -> bool {
        self.noop
    }

    /// Append after older buffered updates and wait for the durable barrier.
    ///
    /// [`DurableAppendError::NotCommitted`] is safe to retry; [`DurableAppendError::Committed`]
    /// means the replay line landed; [`DurableAppendError::AcknowledgementLost`] has unknown status.
    /// No-op handles return `Unsupported`.
    pub async fn append_update_durably(
        &self,
        update: SessionUpdate,
    ) -> Result<(), DurableAppendError> {
        if self.noop {
            return Err(DurableAppendError::NotCommitted(io::Error::new(
                io::ErrorKind::Unsupported,
                "durable session update append is unsupported by a no-op persistence handle",
            )));
        }
        let (respond_to, response) = tokio::sync::oneshot::channel();
        self.tx
            .send(PersistenceMsg::AppendUpdateDurablyAndAck { update, respond_to })
            .map_err(|_| {
                DurableAppendError::NotCommitted(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "session persistence actor stopped before durable append dispatch",
                ))
            })?;
        response
            .await
            .map_err(|_| {
                DurableAppendError::AcknowledgementLost(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "session persistence actor stopped before durable append acknowledgement",
                ))
            })?
            .map_err(DurableAppendError::from)
    }
}

enum PendingAppendOutcome {
    CommittedOk(acp::SessionNotification),
    CommittedErr(acp::SessionNotification, io::Error),
    NotCommittedErr(acp::SessionNotification, io::Error),
}

struct SessionPersistence {
    info: Info,
    storage: Arc<dyn StorageAdapter>,
    /// Pending ACP notification for merging consecutive text chunks
    pending_notification: Option<acp::SessionNotification>,
    rx: mpsc::UnboundedReceiver<PersistenceMsg>,
    /// Session title generation lifecycle.
    summary: crate::session::summary::SummaryGenerator,
    /// Client gateway for `SessionSummaryGenerated` notifications. Used to
    /// announce an auto-generated title only once it has actually been adopted
    /// (see the `GeneratedTitle` handler), so a title rejected for racing a
    /// manual `/rename` never reaches the client. `None` for the subagent
    /// variant, whose lifecycle notifications are handled by the coordinator.
    gateway: Option<GatewaySender>,
}

impl SessionPersistence {
    fn try_merge_text(prev: &mut acp::ContentBlock, new: &acp::ContentBlock) -> bool {
        match (prev, new) {
            (acp::ContentBlock::Text(prev_text), acp::ContentBlock::Text(new_text))
                if prev_text.annotations.is_none()
                    && prev_text.meta.is_none()
                    && new_text.annotations.is_none()
                    && new_text.meta.is_none() =>
            {
                prev_text.text.push_str(&new_text.text);
                true
            }
            _ => false,
        }
    }

    // Empty chunks are chunks that have no content and no meta.
    fn is_empty_chunk(update: &acp::SessionUpdate) -> bool {
        match update {
            acp::SessionUpdate::AgentMessageChunk(chunk)
            | acp::SessionUpdate::AgentThoughtChunk(chunk) => {
                let empty_text =
                    matches!(&chunk.content, acp::ContentBlock::Text(t) if t.text.is_empty());
                let no_meta = chunk.meta.is_none();
                empty_text && no_meta
            }
            _ => false,
        }
    }

    /// Attempt to merge consecutive ACP text notifications to reduce storage writes.
    /// Returns Some(notification) if the pending notification should be written now.
    fn maybe_merge_notification(
        &mut self,
        incoming: &acp::SessionNotification,
    ) -> Option<acp::SessionNotification> {
        // Always skip empty chunks - don't store them at all
        if Self::is_empty_chunk(&incoming.update) {
            return None;
        }

        let Some(pending) = self.pending_notification.take() else {
            self.pending_notification = Some(incoming.clone());
            return None;
        };

        let pending_update = pending.update.clone();
        match (&incoming.update, pending_update) {
            (
                acp::SessionUpdate::AgentMessageChunk(new_chunk),
                acp::SessionUpdate::AgentMessageChunk(mut pending_chunk),
            )
            | (
                acp::SessionUpdate::AgentThoughtChunk(new_chunk),
                acp::SessionUpdate::AgentThoughtChunk(mut pending_chunk),
            ) => {
                let did_merge = pending_chunk.meta.is_none()
                    && new_chunk.meta.is_none()
                    && Self::try_merge_text(&mut pending_chunk.content, &new_chunk.content);

                if did_merge {
                    let merged_update = match &incoming.update {
                        acp::SessionUpdate::AgentMessageChunk(_) => {
                            acp::SessionUpdate::AgentMessageChunk(pending_chunk)
                        }
                        acp::SessionUpdate::AgentThoughtChunk(_) => {
                            acp::SessionUpdate::AgentThoughtChunk(pending_chunk)
                        }
                        _ => unreachable!(),
                    };
                    self.pending_notification = Some(
                        acp::SessionNotification::new(incoming.session_id.clone(), merged_update)
                            .meta(incoming.meta.clone()),
                    );
                    None
                } else {
                    self.pending_notification = Some(incoming.clone());
                    Some(pending)
                }
            }
            _ => {
                self.pending_notification = Some(incoming.clone());
                Some(pending)
            }
        }
    }

    async fn write_update(
        &self,
        update: &SessionUpdate,
    ) -> Result<(), crate::session::storage::AppendUpdateError> {
        self.storage
            .append_update_commit_aware(&self.info, update)
            .await
    }

    fn finish_pending_append(
        notification: acp::SessionNotification,
        result: Result<(), crate::session::storage::AppendUpdateError>,
    ) -> PendingAppendOutcome {
        match result {
            Ok(()) => PendingAppendOutcome::CommittedOk(notification),
            Err(crate::session::storage::AppendUpdateError::NotCommitted(error)) => {
                PendingAppendOutcome::NotCommittedErr(notification, error)
            }
            Err(crate::session::storage::AppendUpdateError::Committed(error)) => {
                PendingAppendOutcome::CommittedErr(notification, error)
            }
        }
    }

    /// Restore uncommitted failures; sync committed records before returning errors.
    async fn drain_pending(&mut self) -> Result<(), crate::session::storage::AppendUpdateError> {
        if let Some(notification) = self.pending_notification.take() {
            let result = self
                .write_update(&SessionUpdate::Acp(Box::new(notification.clone())))
                .await;
            match Self::finish_pending_append(notification, result) {
                PendingAppendOutcome::CommittedOk(_) => {}
                PendingAppendOutcome::CommittedErr(_, error) => {
                    return Err(crate::session::storage::AppendUpdateError::Committed(error));
                }
                PendingAppendOutcome::NotCommittedErr(notification, error) => {
                    self.pending_notification = Some(notification);
                    return Err(crate::session::storage::AppendUpdateError::NotCommitted(
                        error,
                    ));
                }
            }
        }
        Ok(())
    }

    async fn handle_durable_append(
        &mut self,
        update: SessionUpdate,
    ) -> Result<(), crate::session::storage::AppendUpdateError> {
        self.drain_pending().await?;
        let result = self
            .storage
            .append_update_durable_commit_aware(&self.info, &update)
            .await;
        result
    }

    /// Flush any pending merged ACP notification to disk.
    async fn flush_pending(&mut self) {
        if let Err(error) = self.drain_pending().await {
            tracing::warn!(%error, "failed to write pending update");
        }
    }

    async fn run(mut self) {
        // Persistence traffic counts as worktree activity; debounced so
        // long-resident sessions (leader/remote, active for days without a
        // re-open) stay out of gc expiry without per-message DB writes.
        // The constructors fire the t=0 touch, so this starts at now().
        let mut last_worktree_touch = std::time::Instant::now();
        while let Some(msg) = self.rx.recv().await {
            if last_worktree_touch.elapsed() >= WORKTREE_TOUCH_INTERVAL {
                last_worktree_touch = std::time::Instant::now();
                // Detached on purpose: opportunistic refresh, no ordering need.
                spawn_worktree_touch(&self.info);
            }
            match msg {
                PersistenceMsg::Flush => {
                    self.flush_pending().await;
                }
                PersistenceMsg::FlushAndAck { respond_to } => {
                    self.flush_pending().await;
                    let _ = respond_to.send(());
                }
                PersistenceMsg::Update(update) => {
                    match update {
                        SessionUpdate::Acp(notification) => {
                            // ACP notifications use merging to coalesce consecutive text chunks
                            if let Some(to_write) = self.maybe_merge_notification(&notification) {
                                match self
                                    .write_update(&SessionUpdate::Acp(Box::new(to_write.clone())))
                                    .await
                                {
                                    Ok(())
                                    | Err(crate::session::storage::AppendUpdateError::Committed(
                                        _,
                                    )) => {}
                                    Err(error) => tracing::warn!(%error, "failed to write update"),
                                }
                            }
                        }
                        SessionUpdate::Grow(_) => {
                            // Grow notifications are written directly without merging
                            if let Err(error) = self.write_update(&update).await {
                                tracing::warn!(%error, "failed to write update");
                            }
                        }
                    }
                }
                PersistenceMsg::AppendUpdateDurablyAndAck { update, respond_to } => {
                    let result = self.handle_durable_append(update).await;
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::Chat(chat_msg) => {
                    if let Err(e) = self
                        .storage
                        .append_chat_message(&self.info, &chat_msg)
                        .await
                    {
                        tracing::warn!(?e, "failed to write chat message");
                    }
                }
                PersistenceMsg::AppendCwdSwitchAndAck { item, respond_to } => {
                    let result = self
                        .storage
                        .append_cwd_switch_commit_aware(&self.info, &item)
                        .await
                        .map_err(|error| match error {
                            crate::session::storage::AppendCwdSwitchError::NotCommitted(error) => {
                                xai_chat_state::StrictAppendError::NotCommitted(error)
                            }
                            crate::session::storage::AppendCwdSwitchError::Committed {
                                acknowledgement,
                                source,
                            } => xai_chat_state::StrictAppendError::Committed {
                                acknowledgement,
                                source,
                            },
                        });
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::ReplaceChatHistory(messages) => {
                    tracing::info!(
                        num_messages = messages.len(),
                        "Replacing chat history (compaction)"
                    );
                    if let Err(e) = self
                        .storage
                        .replace_chat_history(&self.info, &messages)
                        .await
                    {
                        tracing::warn!(?e, "failed to replace chat history");
                    }
                }
                PersistenceMsg::CurrentModel {
                    model_id,
                    agent_name,
                    reasoning_effort,
                } => {
                    if let Err(e) = self
                        .storage
                        .update_current_model_and_agent(
                            &self.info,
                            &model_id,
                            agent_name.as_deref(),
                            reasoning_effort,
                        )
                        .await
                    {
                        tracing::warn!(?e, "failed to update current model");
                    }
                }
                PersistenceMsg::PlanState(state) => {
                    if let Err(e) = self.storage.write_plan_state(&self.info, &state).await {
                        tracing::warn!(?e, "failed to write plan state");
                    }
                }
                PersistenceMsg::PlanModeState(state) => {
                    if let Err(e) = self.storage.write_plan_mode_state(&self.info, &state).await {
                        tracing::warn!(?e, "failed to write plan mode state");
                    }
                }
                PersistenceMsg::GoalModeState(state) => {
                    if let Err(e) = self.storage.write_goal_mode_state(&self.info, &state).await {
                        tracing::warn!(?e, "failed to write goal mode state");
                    }
                }
                PersistenceMsg::DeleteGoalModeState { respond_to } => {
                    let result = self.storage.delete_goal_mode_state(&self.info).await;
                    if let Err(e) = &result {
                        tracing::warn!(?e, "failed to delete goal mode state");
                    }
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::WorkflowRunState(manifest) => {
                    if let Err(error) = self
                        .storage
                        .write_workflow_run_state(&self.info, &manifest)
                        .await
                    {
                        tracing::warn!(run_id = %manifest.state.run_id, ?error, "failed to write workflow run state");
                    }
                }
                PersistenceMsg::WorkflowRunStateAndAck {
                    manifest,
                    respond_to,
                } => {
                    let result = self
                        .storage
                        .write_workflow_run_state(&self.info, &manifest)
                        .await;
                    if let Err(error) = &result {
                        tracing::warn!(run_id = %manifest.state.run_id, ?error, "failed to write acknowledged workflow run state");
                    }
                    let _ = respond_to.send(result);
                }
                PersistenceMsg::DeleteWorkflowRunState(run_id) => {
                    if let Err(e) = self
                        .storage
                        .delete_workflow_run_state(&self.info, &run_id)
                        .await
                    {
                        tracing::warn!(%run_id, ?e, "failed to delete workflow run state");
                    }
                }
                PersistenceMsg::ContentChunk(content_chunks) => {
                    let content_part = content_chunks
                        .content_chunks
                        .into_iter()
                        .filter_map(|content_chunk| match content_chunk {
                            acp::ContentBlock::Text(text) => Some(text.text),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.summary.update(content_part);

                    // Notify session search index so this turn becomes searchable
                    crate::session::storage::search::notify_session_updated(
                        &self.info.id.to_string(),
                        &self.info.cwd,
                    );
                }
                PersistenceMsg::GeneratedTitle(title) => {
                    // Auto-generated titles must never overwrite a title the
                    // user set via `/rename`. `set_generated_title_if_absent`
                    // writes only when the session still has no title (checked
                    // atomically under the summary lock) and reports whether it
                    // did, so a manual rename that raced this generation wins
                    // and its title is not clobbered locally or on remotes.
                    match self
                        .storage
                        .set_generated_title_if_absent(&self.info, title.clone())
                        .await
                    {
                        Ok(true) => {
                            // Announce to clients only now that the title is
                            // adopted, so a title rejected for racing a manual
                            // `/rename` never overwrites the client's title.
                            crate::session::summary::notify_client(
                                &self.gateway,
                                &self.info,
                                &title,
                            );
                        }
                        Ok(false) => {
                            tracing::debug!(
                                "skipped auto-generated title; session already has a title"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(?e, "failed to persist generated session title");
                        }
                    }
                }
                PersistenceMsg::RewindPoint(point) => {
                    if let Err(e) = self.storage.append_rewind_point(&self.info, &point).await {
                        tracing::warn!(?e, "failed to write rewind point");
                    }
                }
                PersistenceMsg::TruncateRewindPoints { from_index } => {
                    if let Err(e) = self
                        .storage
                        .truncate_rewind_points_from(&self.info, from_index)
                        .await
                    {
                        tracing::warn!(?e, from_index, "failed to truncate rewind points");
                    }
                }
                PersistenceMsg::MergeRewindPointsFrom { target_index } => {
                    if let Err(e) = self
                        .storage
                        .merge_rewind_points_from(&self.info, target_index)
                        .await
                    {
                        tracing::warn!(?e, target_index, "failed to merge rewind points");
                    }
                }
                PersistenceMsg::Signals(signals) => {
                    if let Err(e) = self.storage.write_signals(&self.info, &signals).await {
                        tracing::warn!(?e, "failed to write session signals");
                    }
                }
                PersistenceMsg::AnnouncementState(state) => {
                    if let Err(e) = self
                        .storage
                        .write_announcement_state(&self.info, &state)
                        .await
                    {
                        tracing::warn!(?e, "failed to write announcement state");
                    }
                }
                PersistenceMsg::Btw(entry) => {
                    if let Err(e) = self.storage.append_btw(&self.info, &entry).await {
                        tracing::warn!(?e, "failed to write btw entry");
                    }
                }
                PersistenceMsg::GitHead { commit, branch } => {
                    if let Err(e) = self
                        .storage
                        .update_git_head(&self.info, commit, branch)
                        .await
                    {
                        tracing::warn!(?e, "failed to persist git HEAD");
                    }
                }
                PersistenceMsg::CompactionCheckpoint(checkpoint) => {
                    if let Err(e) = self
                        .storage
                        .write_compaction_checkpoint(&self.info, &checkpoint)
                        .await
                    {
                        tracing::warn!(?e, "failed to write compaction checkpoint file");
                    }
                }
                PersistenceMsg::CompactionRequest(request) => {
                    if let Err(e) = self
                        .storage
                        .write_compaction_request(&self.info, &request)
                        .await
                    {
                        tracing::warn!(?e, "failed to write compaction request artifact");
                    }
                }
                PersistenceMsg::RecapRequest(request) => {
                    if let Err(e) = self.storage.write_recap_request(&self.info, &request).await {
                        tracing::warn!(?e, "failed to write recap request artifact");
                    }
                }
                PersistenceMsg::CompactionSegment(segment) => {
                    if let Err(e) = self
                        .storage
                        .write_compaction_segment(&self.info, &segment)
                        .await
                    {
                        tracing::warn!(?e, "failed to write compaction segment");
                    }
                }
            }
        }

        // Drain the merge buffer on channel close.
        self.flush_pending().await;
    }
}

/// Map a persistence `io::Error` into an `acp::Error` with a human-friendly
/// `message` and a stable `data.code` for log aggregation.
pub(crate) fn io_error_to_acp(e: &io::Error) -> acp::Error {
    // Unix: ENOSPC / EDQUOT. Windows: ERROR_DISK_FULL (112). Also accept
    // `ErrorKind::StorageFull` when no raw OS code is present.
    #[cfg(unix)]
    let is_disk_full_os = matches!(
        e.raw_os_error(),
        Some(raw) if raw == libc::ENOSPC || raw == libc::EDQUOT
    );
    #[cfg(windows)]
    const ERROR_DISK_FULL: i32 = 112;
    #[cfg(windows)]
    let is_disk_full_os = matches!(e.raw_os_error(), Some(ERROR_DISK_FULL));
    let is_disk_full = is_disk_full_os || e.kind() == io::ErrorKind::StorageFull;

    let (message, code) = if is_disk_full {
        ("No space left on device", "FS_DISK_QUOTA_EXCEEDED")
    } else {
        match e.kind() {
            io::ErrorKind::NotFound => ("Path not found.", "FS_NOT_FOUND"),
            io::ErrorKind::PermissionDenied => ("Permission denied.", "FS_PERMISSION_DENIED"),
            _ => {
                tracing::warn!(error = %e, kind = ?e.kind(), raw_os = ?e.raw_os_error(), "unclassified persistence I/O error");
                ("An unexpected I/O error occurred.", "FS_OTHER")
            }
        }
    };
    acp::Error::new(acp::ErrorCode::InternalError.into(), message.to_string()).data(Some(
        serde_json::json!({
            "code": code,
            "detail": e.to_string(),
        }),
    ))
}

#[cfg(test)]
mod io_error_to_acp_tests {
    use super::io_error_to_acp;
    use std::io;

    #[test]
    fn storage_full_maps_to_no_space_left() {
        let acp_err = io_error_to_acp(&io::Error::from(io::ErrorKind::StorageFull));
        assert_eq!(acp_err.message, "No space left on device");
        assert_eq!(acp_err.data.unwrap()["code"], "FS_DISK_QUOTA_EXCEEDED");
    }
}

/// Best-effort worktree liveness touch: stamp `last_accessed_at` on the
/// worktree containing this session's cwd so `grow worktree gc` expires by
/// last use, not creation time. Lives here — not in a `StorageAdapter` —
/// so every session create/load path shares it regardless of backend.
fn spawn_worktree_touch(info: &Info) -> tokio::task::JoinHandle<()> {
    let cwd = info.cwd.clone();
    tokio::task::spawn_blocking(move || {
        crate::session::worktree::touch_worktree_for_cwd(&cwd);
    })
}

/// Bound on how long session open waits for the liveness touch to commit —
/// generous vs the DB's 5s busy_timeout without letting a pathologically
/// locked worktrees.db stall init.
const WORKTREE_TOUCH_INIT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Touch the worktree and wait (bounded) for the write to commit before the
/// session open completes: a detached touch can land after gc's pre-removal
/// re-check reads the row, letting gc delete a worktree that is actively
/// being opened or resumed. Awaiting a blocking-pool task does not block the
/// runtime; on timeout the task keeps running detached (the old
/// fire-and-forget behavior) and init proceeds.
async fn touch_worktree_for_session(info: &Info) {
    if tokio::time::timeout(WORKTREE_TOUCH_INIT_TIMEOUT, spawn_worktree_touch(info))
        .await
        .is_err()
    {
        tracing::debug!(
            cwd = %info.cwd,
            "worktree liveness touch still pending at session open"
        );
    }
}

/// Floor between activity-driven worktree touches from the persistence actor.
const WORKTREE_TOUCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3600);

pub(crate) async fn new(
    info: &Info,
    model_id: acp::ModelId,
    sampling_client: OaiCompatClient,
    gateway: Option<GatewaySender>,
    session_summary_model: String,
) -> io::Result<PersistenceHandle> {
    let root_dir = grow_home();
    let storage: Box<dyn StorageAdapter> = Box::new(JsonlStorageAdapter::with_root(root_dir));

    // Initialize session in storage
    let mut summary = storage.init_session(info, model_id.clone()).await?;
    touch_worktree_for_session(info).await;

    // Update model if different
    if summary.current_model_id != model_id {
        storage.update_current_model(info, &model_id).await?;
        summary.current_model_id = model_id;
    }

    let (tx, rx) = mpsc::unbounded_channel::<PersistenceMsg>();

    let info_clone = info.clone();
    let storage: Arc<dyn StorageAdapter> = Arc::from(storage);
    let handle = PersistenceHandle {
        tx: tx.clone(),
        noop: false,
    };

    tokio::task::spawn(async move {
        let persistence = SessionPersistence {
            info: info_clone,
            storage: storage.clone(),
            pending_notification: None,
            rx,
            summary: crate::session::summary::SummaryGenerator::new(
                crate::session::summary::SummaryConfig {
                    sampling_client,
                    model: session_summary_model,
                    persistence_tx: tx,
                },
            ),
            gateway,
        };
        persistence.run().await;
    });

    Ok(handle)
}

/// Create a persistence handle that writes to an explicit directory on disk.
///
/// Used for subagent child sessions whose files live under the parent's
/// session directory: `{parent_session_dir}/subagents/{subagent_id}/`.
///
/// Unlike [`new()`], this:
/// - Uses `JsonlStorageAdapter::with_explicit_session_dir()` to bypass
///   the standard `{root}/sessions/{cwd}/{id}/` path computation.
/// - Skips remote sync (subagent sessions are not synced to cloud).
/// - Skips relay sync (subagent sessions are not shared).
/// - Skips gateway (lifecycle notifications are handled by the coordinator).
pub async fn new_with_explicit_dir(
    info: &Info,
    target_dir: PathBuf,
    model_id: acp::ModelId,
    sampling_client: OaiCompatClient,
    session_summary_model: String,
) -> io::Result<PersistenceHandle> {
    let summary_path = target_dir.join("summary.json");
    let storage: Box<dyn StorageAdapter> =
        Box::new(JsonlStorageAdapter::with_explicit_session_dir(target_dir));

    // Initialize session in storage (creates summary.json, etc.)
    let mut summary = storage.init_session(info, model_id.clone()).await?;
    touch_worktree_for_session(info).await;
    if summary.session_kind.is_none() {
        summary.session_kind = Some("subagent".to_string());
    }
    let summary_json = serde_json::to_vec_pretty(&summary)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&summary_path, summary_json)?;

    if summary.current_model_id != model_id {
        storage.update_current_model(info, &model_id).await?;
        summary.current_model_id = model_id;
    }

    let (tx, rx) = mpsc::unbounded_channel::<PersistenceMsg>();

    let info_clone = info.clone();
    let storage: Arc<dyn StorageAdapter> = Arc::from(storage);
    let handle = PersistenceHandle {
        tx: tx.clone(),
        noop: false,
    };

    tokio::task::spawn(async move {
        let persistence = SessionPersistence {
            info: info_clone,
            storage: storage.clone(),
            pending_notification: None,
            rx,
            summary: crate::session::summary::SummaryGenerator::new(
                crate::session::summary::SummaryConfig {
                    sampling_client,
                    model: session_summary_model,
                    persistence_tx: tx,
                },
            ),
            gateway: None,
        };
        persistence.run().await;
    });

    Ok(handle)
}

pub struct PersistedInfo {
    pub summary: Summary,
    pub chat_history: Vec<ConversationItem>,
    /// All session updates (ACP updates and Grow extension updates) in chronological order
    pub updates: Vec<SessionUpdate>,
    pub plan_state: Option<TodoState>,
    pub rewind_points: Vec<RewindPoint>,
    /// Persisted session signals (None for old sessions without signals file)
    pub signals: Option<SessionSignals>,
    pub workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
}

/// Same as PersistedInfo but without updates - for memory efficiency when streaming
pub struct PersistedInfoLight {
    pub summary: Summary,
    pub chat_history: Vec<ConversationItem>,
    pub plan_state: Option<TodoState>,
    pub plan_mode_state: Option<crate::session::plan_mode::PlanModeSnapshot>,
    /// Path to updates file for streaming reads
    pub updates_file_path: Option<std::path::PathBuf>,
    /// Adapter-owned path to `rewind_points.jsonl` for the session's
    /// `FileStateTracker` to load lazily. `None` if the backend doesn't persist
    /// rewind points to a streamable file.
    pub rewind_points_file_path: Option<std::path::PathBuf>,
    /// Persisted session signals (None for old sessions without signals file)
    pub signals: Option<SessionSignals>,
    /// Persisted announcement tracking state (None for sessions before this feature)
    pub announcement_state: Option<crate::session::announcement_state::AnnouncementState>,
    /// Persisted goal mode orchestration state (None for sessions without goal mode)
    pub goal_mode_state: Option<crate::session::goal_tracker::GoalOrchestration>,
    pub workflow_runs: Vec<crate::session::workflow::store::RestoredWorkflowRun>,
}

#[expect(dead_code, reason = "wired when session restore flow calls load")]
pub(crate) async fn load(
    info: &Info,
    sampling_client: OaiCompatClient,
    gateway: Option<GatewaySender>,
    session_summary_model: String,
) -> io::Result<(PersistedInfo, PersistenceHandle)> {
    let root_dir = grow_home();
    let storage: Box<dyn StorageAdapter> = Box::new(JsonlStorageAdapter::with_root(root_dir));

    let persisted = storage.load_session(info).await?;
    let loaded_info = info.clone();
    // Touch on load too: resuming must reset the worktree's gc expiry clock.
    touch_worktree_for_session(&loaded_info).await;

    let persisted_info = PersistedInfo {
        summary: persisted.summary,
        chat_history: persisted.chat_history,
        updates: persisted.updates,
        plan_state: persisted.plan_state,
        rewind_points: persisted.rewind_points,
        signals: persisted.signals,
        workflow_runs: persisted.workflow_runs,
    };

    let (tx, rx) = mpsc::unbounded_channel::<PersistenceMsg>();

    let storage: Arc<dyn StorageAdapter> = Arc::from(storage);

    let has_title = !persisted_info.summary.display_title().is_empty();
    let handle = PersistenceHandle {
        tx: tx.clone(),
        noop: false,
    };
    tokio::task::spawn(async move {
        let mut summary_gen = crate::session::summary::SummaryGenerator::new(
            crate::session::summary::SummaryConfig {
                sampling_client,
                model: session_summary_model,
                persistence_tx: tx,
            },
        );
        if has_title {
            summary_gen.mark_done();
        }
        let persistence = SessionPersistence {
            info: loaded_info,
            storage: storage.clone(),
            pending_notification: None,
            rx,
            summary: summary_gen,
            gateway,
        };
        persistence.run().await;
    });

    Ok((persisted_info, handle))
}

/// Like `load`, but doesn't load updates into memory.
/// Instead, provides the path to the updates file for streaming reads.
/// Use this for memory-efficient session loading when replaying updates.
pub(crate) async fn load_light(
    info: &Info,
    sampling_client: OaiCompatClient,
    gateway: Option<GatewaySender>,
    session_summary_model: String,
) -> io::Result<(PersistedInfoLight, PersistenceHandle)> {
    let root_dir = grow_home();
    let storage: Box<dyn StorageAdapter> =
        Box::new(JsonlStorageAdapter::with_root(root_dir.clone()));

    let persisted = storage.load_session_without_updates(info).await?;
    let loaded_info = info.clone();
    // Touch on load too: resuming must reset the worktree's gc expiry clock.
    touch_worktree_for_session(&loaded_info).await;

    let updates_file_path = storage.updates_file_path(&loaded_info);
    let rewind_points_file_path = storage.rewind_points_file_path(&loaded_info);

    let persisted_info = PersistedInfoLight {
        summary: persisted.summary,
        chat_history: persisted.chat_history,
        plan_state: persisted.plan_state,
        plan_mode_state: persisted.plan_mode_state,
        updates_file_path,
        rewind_points_file_path,
        signals: persisted.signals,
        announcement_state: persisted.announcement_state,
        goal_mode_state: persisted.goal_mode_state,
        workflow_runs: persisted.workflow_runs,
    };

    let (tx, rx) = mpsc::unbounded_channel::<PersistenceMsg>();

    let storage: Arc<dyn StorageAdapter> = Arc::from(storage);

    let has_title = !persisted_info.summary.display_title().is_empty();
    let handle = PersistenceHandle {
        tx: tx.clone(),
        noop: false,
    };
    tokio::task::spawn(async move {
        let mut summary_gen = crate::session::summary::SummaryGenerator::new(
            crate::session::summary::SummaryConfig {
                sampling_client,
                model: session_summary_model,
                persistence_tx: tx,
            },
        );
        if has_title {
            summary_gen.mark_done();
        }
        let persistence = SessionPersistence {
            info: loaded_info,
            storage: storage.clone(),
            pending_notification: None,
            rx,
            summary: summary_gen,
            gateway,
        };
        persistence.run().await;
    });

    Ok((persisted_info, handle))
}

/// List session summaries, optionally filtered by cwd (absolute path string).
/// Returns summaries sorted by `last_active_at` (else `updated_at`) descending.
fn recover_session_relocations_in(root: &Path) -> crate::session::storage::relocation::Result<()> {
    crate::session::storage::relocation::RelocationStorage::new(root.into()).recover_all()
}

pub async fn list_summaries(cwd: Option<&str>) -> io::Result<Vec<Summary>> {
    let root_dir = crate::util::grow_home::grow_home();
    let recovery_root = root_dir.clone();
    tokio::task::spawn_blocking(move || recover_session_relocations_in(&recovery_root))
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)?;
    let storage: Box<dyn StorageAdapter> = Box::new(JsonlStorageAdapter::with_root(root_dir));
    storage.list_sessions(cwd).await
}

#[derive(Debug, thiserror::Error)]
pub enum DeleteSessionError {
    /// Listing local summaries (to resolve the on-disk session dir) failed.
    #[error("failed to list sessions: {0}")]
    List(#[source] io::Error),
    /// The local on-disk session directory could not be removed.
    #[error("failed to delete session: {0}")]
    Local(#[source] io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionDeletion {
    pub local_removed: bool,
}

impl SessionDeletion {
    /// `true` when a copy was removed from at least one location.
    pub fn any_removed(self) -> bool {
        self.local_removed
    }
}

/// Permanently delete a local session directory and its search-index entry.
/// Missing sessions are treated as an idempotent success.
pub async fn delete_session_history(
    session_id: &str,
    cwd: Option<&str>,
) -> Result<SessionDeletion, DeleteSessionError> {
    let sid = acp::SessionId::new(Arc::from(session_id));

    let summaries = list_summaries(cwd)
        .await
        .map_err(DeleteSessionError::List)?;
    let local_info = summaries
        .iter()
        .find(|s| s.info.id == sid)
        .map(|s| s.info.clone());

    let Some(info) = local_info else {
        return Ok(SessionDeletion::default());
    };

    JsonlStorageAdapter::default()
        .delete_session(&info)
        .await
        .map_err(DeleteSessionError::Local)?;

    // Evict from the search index: the indexer re-reads the (now
    // missing) summary and drops the document.
    crate::session::storage::search::notify_session_updated(&info.id.to_string(), &info.cwd);

    Ok(SessionDeletion {
        local_removed: true,
    })
}

#[cfg(test)]
#[path = "persistence_tests.rs"]
mod durable_update_tests;

/// List the `limit` most recently modified session summaries across all
/// workspaces. Uses stat-based mtime sorting to avoid reading every
/// summary file on disk; final order uses `last_active_at` else `updated_at`.
pub async fn list_recent_summaries(limit: usize) -> io::Result<Vec<Summary>> {
    let root_dir = crate::util::grow_home::grow_home();
    let recovery_root = root_dir.clone();
    tokio::task::spawn_blocking(move || recover_session_relocations_in(&recovery_root))
        .await
        .map_err(io::Error::other)?
        .map_err(io::Error::other)?;
    let storage = JsonlStorageAdapter::with_root(root_dir);
    storage.list_sessions_recent(limit).await
}

// Session folder TTL cleanup

/// Guard ensuring session cleanup runs at most once per process.
static CLEANUP_SESSIONS_ONCE: std::sync::Once = std::sync::Once::new();

/// Default TTL for stale session files (30 days).
const DEFAULT_CLEANUP_TTL_DAYS: u32 = 30;

/// Walk `~/.grow/sessions/` and delete files with mtime older than `ttl_days`.
/// Removes empty session directories after file cleanup.
/// Skips `skip_session_dir` if provided (current session).
///
/// This is a **synchronous** function intended to be called via
/// `tokio::task::spawn_blocking` so it runs on the thread pool and
/// never competes with the agent's single-threaded `LocalSet`.
pub fn cleanup_stale_sessions(skip_session_dir: Option<&Path>) {
    CLEANUP_SESSIONS_ONCE.call_once(|| {
        let ttl_days = resolve_cleanup_ttl_days();
        let root = grow_home();
        if let Err(error) = recover_session_relocations_in(&root) {
            tracing::error!(%error, "session relocation recovery failed before TTL cleanup");
            return;
        }
        let sessions_root = root.join("sessions");
        let relocation_view = match storage_view(&sessions_root) {
            Ok(view) => view,
            Err(error) => {
                tracing::error!(%error, "session relocation snapshot failed before TTL cleanup");
                return;
            }
        };

        tracing::info!(
            target: "grow_shell::session::persistence",
            sessions_root = %sessions_root.display(),
            ttl_days,
            skip = ?skip_session_dir.map(|p| p.display().to_string()),
            "SESSION_CLEANUP_START: scanning for stale session files"
        );

        let stats = cleanup_stale_sessions_inner(
            &sessions_root,
            ttl_days,
            skip_session_dir,
            &relocation_view,
            &root,
            CleanupLevel::SessionsRoot,
        );

        tracing::info!(
            target: "grow_shell::session::persistence",
            sessions_root = %sessions_root.display(),
            files_deleted = stats.files_deleted,
            dirs_removed = stats.dirs_removed,
            errors = stats.errors,
            "SESSION_CLEANUP_DONE"
        );
    });
}

/// Resolve TTL from config.toml `[storage] cleanup_ttl_days`, falling back to 30.
fn resolve_cleanup_ttl_days() -> u32 {
    // Try to load config and read [storage] section
    if let Ok(layers) = crate::config::ConfigLayers::load() {
        let effective = layers.effective_config_disk_only();
        if let Some(storage) = effective.get("storage")
            && let Some(ttl) = storage.get("cleanup_ttl_days")
            && let Some(days) = ttl.as_integer()
            && days > 0
        {
            return days as u32;
        }
    }
    DEFAULT_CLEANUP_TTL_DAYS
}

#[derive(Default)]
struct CleanupStats {
    files_deleted: u32,
    dirs_removed: u32,
    errors: u32,
}

#[derive(Clone, Copy)]
enum CleanupLevel {
    SessionsRoot,
    Cwd,
    Session,
}

/// Recursive cleanup: delete stale files, then rmdir empty dirs (post-order).
fn cleanup_stale_sessions_inner(
    root: &Path,
    ttl_days: u32,
    skip: Option<&Path>,
    relocation_view: &crate::session::storage::relocation::RelocationView,
    grow_home: &Path,
    level: CleanupLevel,
) -> CleanupStats {
    let mut stats = CleanupStats::default();

    if root
        .file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
    {
        return stats;
    }
    if let Some(skip_dir) = skip
        && root == skip_dir
    {
        return stats;
    }

    let Ok(entries) = std::fs::read_dir(root) else {
        return stats;
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(
                    target: "grow_shell::session::persistence",
                    error = %e,
                    "SESSION_CLEANUP_READ_ERROR"
                );
                stats.errors += 1;
                continue;
            }
        };
        let path = entry.path();

        if let Some(skip_dir) = skip
            && path == skip_dir
        {
            continue;
        }

        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                stats.errors += 1;
                continue;
            }
        };
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            if matches!(level, CleanupLevel::SessionsRoot)
                && relocation_view.protects_cwd_dir(&path)
            {
                continue;
            }
            let lease = if matches!(level, CleanupLevel::Cwd) {
                let summary = path.join("summary.json");
                let summary_type = match std::fs::symlink_metadata(&summary) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        let child_stats = cleanup_stale_sessions_inner(
                            &path,
                            ttl_days,
                            skip,
                            relocation_view,
                            grow_home,
                            CleanupLevel::Session,
                        );
                        stats.files_deleted += child_stats.files_deleted;
                        stats.dirs_removed += child_stats.dirs_removed;
                        stats.errors += child_stats.errors;
                        if child_stats.files_deleted > 0 && std::fs::remove_dir(&path).is_ok() {
                            stats.dirs_removed += 1;
                        }
                        continue;
                    }
                    Err(error) => {
                        stats.errors += 1;
                        tracing::debug!(
                            target: "grow_shell::session::persistence",
                            path = %summary.display(),
                            %error,
                            "SESSION_CLEANUP_METADATA_ERROR"
                        );
                        continue;
                    }
                };
                if !summary_type.file_type().is_file() || summary_type.file_type().is_symlink() {
                    continue;
                }
                let Some(id) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let storage = crate::session::storage::relocation::RelocationStorage::new(
                    grow_home.to_path_buf(),
                );
                let Ok(lease) = storage.acquire(id) else {
                    continue;
                };
                match storage.read_journal(id) {
                    Err(crate::session::storage::relocation::RelocationError::JournalMissing(
                        _,
                    )) => Some(lease),
                    _ => continue,
                }
            } else {
                None
            };
            let next = match level {
                CleanupLevel::SessionsRoot => CleanupLevel::Cwd,
                CleanupLevel::Cwd | CleanupLevel::Session => CleanupLevel::Session,
            };
            let child_stats = cleanup_stale_sessions_inner(
                &path,
                ttl_days,
                skip,
                relocation_view,
                grow_home,
                next,
            );
            stats.files_deleted += child_stats.files_deleted;
            stats.dirs_removed += child_stats.dirs_removed;
            stats.errors += child_stats.errors;

            // Only attempt remove_dir if this subtree actually had stale
            // files deleted in this pass. Otherwise we risk removing dirs
            // that were deliberately created for use by concurrent sessions.
            if child_stats.files_deleted > 0 && std::fs::remove_dir(&path).is_ok() {
                stats.dirs_removed += 1;
                tracing::debug!(
                    target: "grow_shell::session::persistence",
                    dir = %path.display(),
                    "SESSION_CLEANUP_RMDIR"
                );
            }
            drop(lease);
        } else if let Ok(mtime) = metadata.modified()
            && is_stale(mtime, ttl_days)
        {
            if std::fs::remove_file(&path).is_ok() {
                stats.files_deleted += 1;
                tracing::debug!(
                    target: "grow_shell::session::persistence",
                    file = %path.display(),
                    "SESSION_CLEANUP_DELETE"
                );
            } else {
                stats.errors += 1;
            }
        }
    }

    stats
}

fn is_stale(mtime: std::time::SystemTime, ttl_days: u32) -> bool {
    let ttl = std::time::Duration::from_secs(u64::from(ttl_days) * 86400);
    mtime.elapsed().is_ok_and(|age| age > ttl)
}

#[cfg(test)]
mod agent_name_persistence_tests {
    use super::*;

    #[test]
    fn summary_round_trips_agent_name_through_json() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.agent_name = Some("cursor".into());

        let json = serde_json::to_string(&summary).unwrap();
        let deserialized: Summary = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.agent_name.as_deref(), Some("cursor"));
    }

    #[test]
    fn summary_deserializes_without_agent_name_backward_compat() {
        // Simulate an old summary.json that lacks agent_name — must still
        // deserialize successfully (serde default → None).
        let json = r#"{
            "info": { "id": "old-session", "cwd": "/tmp" },
            "session_summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 0,
            "num_chat_messages": 0,
            "current_model_id": "test-model"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert!(
            summary.agent_name.is_none(),
            "old summaries without agent_name should deserialize as None"
        );
    }

    #[test]
    fn summary_skips_none_agent_name_in_serialized_json() {
        let summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        let json = serde_json::to_string(&summary).unwrap();
        assert!(
            !json.contains("agent_name"),
            "None agent_name should not appear in serialized JSON"
        );
    }

    #[test]
    fn summary_includes_agent_name_when_set() {
        let mut summary = Summary::new(
            &Info {
                id: acp::SessionId::new("test"),
                cwd: "/tmp".into(),
            },
            default_model_id(),
        )
        .unwrap();
        summary.agent_name = Some("cursor".into());
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("agent_name"));
        assert!(json.contains("cursor"));
    }

    #[test]
    fn summary_round_trips_various_agent_names() {
        for name in [
            "cursor",
            "grow-build",
            "grow-build-plan",
            "codex",
            "browser-use",
        ] {
            let mut summary = Summary::new(
                &Info {
                    id: acp::SessionId::new("test"),
                    cwd: "/tmp".into(),
                },
                default_model_id(),
            )
            .unwrap();
            summary.agent_name = Some(name.into());

            let json = serde_json::to_string(&summary).unwrap();
            let deserialized: Summary = serde_json::from_str(&json).unwrap();
            assert_eq!(
                deserialized.agent_name.as_deref(),
                Some(name),
                "round-trip failed for agent_name={name}"
            );
        }
    }

    #[test]
    fn summary_with_agent_name_in_full_json() {
        // Verify agent_name deserializes correctly alongside all other fields.
        let json = r#"{
            "info": { "id": "full-session", "cwd": "/tmp" },
            "session_summary": "test session",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 10,
            "num_chat_messages": 5,
            "current_model_id": "cursor-model",
            "agent_name": "cursor",
            "generated_title": "Fix editor mode",
            "head_branch": "main"
        }"#;
        let summary: Summary = serde_json::from_str(json).unwrap();
        assert_eq!(summary.agent_name.as_deref(), Some("cursor"));
        assert_eq!(summary.current_model_id.0.as_ref(), "cursor-model");
        assert_eq!(summary.generated_title.as_deref(), Some("Fix editor mode"));
    }
}

#[cfg(test)]
mod session_exists_tests {
    use super::session_exists_in_root;
    use std::fs;
    use tempfile::TempDir;

    fn make_root() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn returns_false_when_root_does_not_exist() {
        let root = std::path::PathBuf::from("/nonexistent/grow/sessions");
        assert!(!session_exists_in_root("any-id", &root));
    }

    #[test]
    fn returns_false_when_root_is_empty() {
        let tmp = make_root();
        let root = tmp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();
        assert!(!session_exists_in_root("my-session", &root));
    }

    #[test]
    fn returns_true_when_session_dir_exists_under_any_cwd() {
        let tmp = make_root();
        let root = tmp.path().join("sessions");
        // Simulate sessions/<encoded-cwd>/<session-id>/
        let session_dir = root.join("some_cwd_dir").join("my-session-id");
        fs::create_dir_all(&session_dir).unwrap();
        fs::write(session_dir.join("summary.json"), b"{}").unwrap();

        assert!(session_exists_in_root("my-session-id", &root));
    }

    #[test]
    fn returns_false_when_session_id_is_a_file_not_a_dir() {
        let tmp = make_root();
        let root = tmp.path().join("sessions");
        let cwd_dir = root.join("some_cwd_dir");
        fs::create_dir_all(&cwd_dir).unwrap();
        // Create a file instead of a directory with the session id name
        fs::write(cwd_dir.join("my-session-id"), b"").unwrap();

        assert!(!session_exists_in_root("my-session-id", &root));
    }

    #[test]
    fn returns_false_for_different_session_id() {
        let tmp = make_root();
        let root = tmp.path().join("sessions");
        let session_dir = root.join("some_cwd_dir").join("session-a");
        fs::create_dir_all(&session_dir).unwrap();

        assert!(!session_exists_in_root("session-b", &root));
    }

    #[test]
    fn finds_session_across_multiple_cwd_dirs() {
        let tmp = make_root();
        let root = tmp.path().join("sessions");
        // Two persisted sessions under different cwd directories.
        let other = root.join("cwd1").join("other-session");
        let target = root.join("cwd2").join("target-session");
        fs::create_dir_all(&other).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(other.join("summary.json"), b"{}").unwrap();
        fs::write(target.join("summary.json"), b"{}").unwrap();

        assert!(session_exists_in_root("target-session", &root));
        assert!(!session_exists_in_root("missing-session", &root));
    }
}

#[cfg(test)]
mod find_summary_by_session_id_tests {
    use super::find_summary_by_session_id_in_root;
    use std::fs;
    use tempfile::TempDir;

    fn write_summary(root: &std::path::Path, cwd_dir: &str, session_id: &str, json: &str) {
        let dir = root.join(cwd_dir).join(session_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("summary.json"), json).unwrap();
    }

    fn minimal_summary(head_commit: &str, head_branch: &str) -> String {
        serde_json::json!({
            "info": { "id": "test-session", "cwd": "/tmp" },
            "session_summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "num_messages": 0,
            "current_model_id": "grow-3",
            "head_commit": head_commit,
            "head_branch": head_branch
        })
        .to_string()
    }

    #[test]
    fn returns_none_when_root_missing() {
        let result =
            find_summary_by_session_id_in_root("any", &std::path::PathBuf::from("/nonexistent"));
        assert!(result.is_none());
    }

    #[test]
    fn returns_none_when_no_matching_session() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        write_summary(&root, "cwd1", "other-id", &minimal_summary("abc", "main"));
        assert!(find_summary_by_session_id_in_root("missing-id", &root).is_none());
    }

    #[test]
    fn finds_summary_across_cwd_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        write_summary(
            &root,
            "encoded_cwd",
            "target-session",
            &minimal_summary("deadbeef", "feature/x"),
        );

        let found = find_summary_by_session_id_in_root("target-session", &root).unwrap();
        assert_eq!(found.head_commit.as_deref(), Some("deadbeef"));
        assert_eq!(found.head_branch.as_deref(), Some("feature/x"));
    }

    #[test]
    fn skips_malformed_summary() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        // Write invalid JSON
        let dir = root.join("cwd1").join("bad-session");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("summary.json"), b"not-json").unwrap();

        assert!(find_summary_by_session_id_in_root("bad-session", &root).is_none());
    }
}

#[cfg(test)]
mod resumed_sandbox_profile_tests {
    use super::{
        RelocationError, RelocationView, most_recent_local_summary_for_cwd_in_root,
        most_recent_local_summary_for_cwd_in_view, read_summary_from_dir,
        resumed_session_sandbox_profile_in_root,
    };
    use std::{fs, io};
    use tempfile::TempDir;

    /// Write a session summary under the *encoded* cwd dir (matching how the
    /// resume helpers locate sessions). `sandbox_profile` is included only when
    /// `Some`, mirroring older summaries that predate the field.
    fn write_session(
        root: &std::path::Path,
        cwd: &str,
        session_id: &str,
        updated_at: &str,
        last_active_at: Option<&str>,
        sandbox_profile: Option<&str>,
        hidden: bool,
    ) {
        let encoded = crate::util::grow_home::encode_cwd_dirname(cwd);
        let dir = root.join(&encoded).join(session_id);
        fs::create_dir_all(&dir).unwrap();
        let mut summary = serde_json::json!({
            "info": { "id": session_id, "cwd": cwd },
            "session_summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": updated_at,
            "num_messages": 0,
            "current_model_id": "grow-3",
        });
        if let Some(la) = last_active_at {
            summary["last_active_at"] = serde_json::Value::String(la.to_string());
        }
        if let Some(profile) = sandbox_profile {
            summary["sandbox_profile"] = serde_json::Value::String(profile.to_string());
        }
        if hidden {
            summary["hidden"] = serde_json::Value::Bool(true);
        }
        fs::write(dir.join("summary.json"), summary.to_string()).unwrap();
    }

    #[test]
    fn explicit_id_returns_persisted_profile() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        write_session(
            &root,
            "/work/a",
            "sess-1",
            "2026-01-01T00:00:00Z",
            None,
            Some("strict"),
            false,
        );

        assert_eq!(
            resumed_session_sandbox_profile_in_root(Some("sess-1"), None, &root),
            Some("strict".to_string())
        );
    }

    #[test]
    fn explicit_id_without_persisted_profile_is_none() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        // Older session, created before the field existed.
        write_session(
            &root,
            "/work/a",
            "sess-old",
            "2026-01-01T00:00:00Z",
            None,
            None,
            false,
        );

        assert_eq!(
            resumed_session_sandbox_profile_in_root(Some("sess-old"), None, &root),
            None
        );
    }

    #[test]
    fn empty_or_missing_id_and_no_cwd_is_none() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        assert_eq!(
            resumed_session_sandbox_profile_in_root(Some(""), None, &root),
            None
        );
        assert_eq!(
            resumed_session_sandbox_profile_in_root(None, None, &root),
            None
        );
    }

    #[test]
    fn most_recent_cwd_picks_latest_session_profile() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/work/proj";
        write_session(
            &root,
            cwd,
            "older",
            "2026-01-01T00:00:00Z",
            None,
            Some("workspace"),
            false,
        );
        write_session(
            &root,
            cwd,
            "newer",
            "2026-06-01T00:00:00Z",
            None,
            Some("off"),
            false,
        );

        assert_eq!(
            most_recent_local_summary_for_cwd_in_root(cwd, &root)
                .unwrap()
                .info
                .id
                .0
                .to_string(),
            "newer"
        );
        assert_eq!(
            resumed_session_sandbox_profile_in_root(None, Some(cwd), &root),
            Some("off".to_string())
        );
    }

    #[test]
    fn most_recent_cwd_skips_corrupt_summary() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/work/proj";
        write_session(
            &root,
            cwd,
            "valid",
            "2026-06-01T00:00:00Z",
            None,
            Some("workspace"),
            false,
        );
        let corrupt_dir = root
            .join(crate::util::grow_home::encode_cwd_dirname(cwd))
            .join("corrupt");
        fs::create_dir_all(&corrupt_dir).unwrap();
        fs::write(corrupt_dir.join("summary.json"), b"not-json").unwrap();

        let picked = most_recent_local_summary_for_cwd_in_root(cwd, &root).unwrap();
        assert_eq!(picked.info.id.0.as_ref(), "valid");
    }

    #[test]
    fn most_recent_cwd_skips_raced_not_found() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/work/proj";
        write_session(
            &root,
            cwd,
            "valid",
            "2026-06-01T00:00:00Z",
            None,
            Some("workspace"),
            false,
        );
        write_session(
            &root,
            cwd,
            "removed",
            "2026-07-01T00:00:00Z",
            None,
            Some("strict"),
            false,
        );
        let view = RelocationView::load_for_sessions_root(&root).unwrap();

        let picked = most_recent_local_summary_for_cwd_in_view(cwd, &view, |session_dir| {
            if session_dir.ends_with("removed") {
                Err(RelocationError::Io {
                    operation: "read",
                    path: session_dir.join("summary.json"),
                    source: io::Error::new(io::ErrorKind::NotFound, "injected"),
                })
            } else {
                read_summary_from_dir(session_dir)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(picked.info.id.0.as_ref(), "valid");
    }

    #[test]
    fn most_recent_cwd_propagates_non_not_found_io_errors() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/work/proj";
        write_session(
            &root,
            cwd,
            "older",
            "2026-01-01T00:00:00Z",
            None,
            Some("workspace"),
            false,
        );
        write_session(
            &root,
            cwd,
            "unreadable-newer",
            "2026-06-01T00:00:00Z",
            None,
            Some("strict"),
            false,
        );
        let view = RelocationView::load_for_sessions_root(&root).unwrap();

        let error = most_recent_local_summary_for_cwd_in_view(cwd, &view, |session_dir| {
            if session_dir.ends_with("unreadable-newer") {
                Err(RelocationError::Io {
                    operation: "read",
                    path: session_dir.join("summary.json"),
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "injected"),
                })
            } else {
                read_summary_from_dir(session_dir)
            }
        })
        .unwrap_err();
        assert!(matches!(
            error,
            RelocationError::Io { source, .. }
                if source.kind() == io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn most_recent_cwd_prefers_last_active_at_over_updated_at() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/work/proj";
        write_session(
            &root,
            cwd,
            "recent_activity",
            "2026-02-01T00:00:00Z",
            Some("2026-05-01T00:00:00Z"),
            Some("workspace"),
            false,
        );
        write_session(
            &root,
            cwd,
            "stale_activity",
            "2026-04-01T00:00:00Z",
            Some("2026-01-01T00:00:00Z"),
            Some("off"),
            false,
        );

        let picked = most_recent_local_summary_for_cwd_in_root(cwd, &root).unwrap();
        assert_eq!(picked.info.id.0.as_ref(), "recent_activity");
        assert_eq!(
            resumed_session_sandbox_profile_in_root(None, Some(cwd), &root),
            Some("workspace".to_string())
        );
    }

    #[test]
    fn most_recent_cwd_skips_hidden_session() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/work/proj";
        // Older, visible session.
        write_session(
            &root,
            cwd,
            "visible",
            "2026-01-01T00:00:00Z",
            None,
            Some("workspace"),
            false,
        );
        // Newer, hidden (e.g. subagent) session — the most-recent peek must
        // ignore it, matching what `list_sessions` resumes.
        write_session(
            &root,
            cwd,
            "hidden-newer",
            "2026-06-01T00:00:00Z",
            None,
            Some("off"),
            true,
        );

        assert_eq!(
            most_recent_local_summary_for_cwd_in_root(cwd, &root)
                .unwrap()
                .info
                .id
                .0
                .to_string(),
            "visible"
        );
        assert_eq!(
            resumed_session_sandbox_profile_in_root(None, Some(cwd), &root),
            Some("workspace".to_string())
        );
    }

    #[test]
    fn most_recent_cwd_with_no_sessions_is_none() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        assert_eq!(
            resumed_session_sandbox_profile_in_root(None, Some("/empty/cwd"), &root),
            None
        );
    }
}

#[cfg(test)]
mod session_exists_for_cwd_tests {
    use super::{
        resolve_local_session_any_cwd_in_root, session_exists_for_cwd_in_root,
        session_exists_in_root,
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn returns_true_when_session_exists_under_matching_cwd() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/project/alpha";
        let session_id = "my-session";

        let encoded = crate::util::grow_home::encode_cwd_dirname(cwd);
        let dir = root.join(&encoded).join(session_id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("summary.json"), b"{}").unwrap();

        assert!(session_exists_for_cwd_in_root(session_id, cwd, &root));
    }

    #[test]
    fn returns_false_when_session_absent_under_cwd() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        fs::create_dir_all(&root).unwrap();

        assert!(!session_exists_for_cwd_in_root(
            "missing",
            "/project/alpha",
            &root
        ));
    }

    /// Regression test for the cross-cwd false-positive.
    ///
    /// A cwd-specific lookup must not confuse a same-id session from another cwd
    /// with the requested local session.
    #[test]
    fn session_under_different_cwd_is_not_considered_present() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let session_id = "cross-cwd-session";

        // Create the session only under cwd-A (a real session has a summary.json).
        let encoded_a = crate::util::grow_home::encode_cwd_dirname("/project/alpha");
        let dir_a = root.join(&encoded_a).join(session_id);
        fs::create_dir_all(&dir_a).unwrap();
        fs::write(dir_a.join("summary.json"), b"{}").unwrap();

        // Global scan (old behaviour) finds it — this is the incorrect check
        assert!(
            session_exists_in_root(session_id, &root),
            "global scan must find the session under cwd-A"
        );

        // Cwd-specific check must return false for cwd-B
        assert!(
            !session_exists_for_cwd_in_root(session_id, "/project/beta", &root),
            "cwd-specific check must return false for cwd-B"
        );

        // And true for cwd-A (sanity)
        assert!(
            session_exists_for_cwd_in_root(session_id, "/project/alpha", &root),
            "cwd-specific check must return true for the matching cwd-A"
        );
    }

    /// An `images/`-only stub (no `summary.json`) is not a resumable session.
    #[test]
    fn images_only_stub_is_not_a_session() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let cwd = "/project/alpha";
        let session_id = "stub-session";

        let encoded = crate::util::grow_home::encode_cwd_dirname(cwd);
        let images = root.join(&encoded).join(session_id).join("images");
        fs::create_dir_all(&images).unwrap();
        fs::write(images.join("image-1.png"), b"png").unwrap();

        assert!(
            !session_exists_for_cwd_in_root(session_id, cwd, &root),
            "an images-only stub (no summary.json) must not be a resumable session"
        );
    }

    /// The all-cwd scan skips a stub and returns the real session's cwd.
    #[test]
    fn resolve_local_session_any_cwd_skips_stub_and_finds_real() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("sessions");
        let session_id = "real-session";

        // Real session under cwd-A.
        let cwd_a = "/project/alpha";
        let encoded_a = crate::util::grow_home::encode_cwd_dirname(cwd_a);
        let dir_a = root.join(&encoded_a).join(session_id);
        fs::create_dir_all(&dir_a).unwrap();
        fs::write(dir_a.join("summary.json"), b"{}").unwrap();

        // Images-only stub for the SAME id under cwd-B.
        let cwd_b = "/project/beta";
        let encoded_b = crate::util::grow_home::encode_cwd_dirname(cwd_b);
        let images_b = root.join(&encoded_b).join(session_id).join("images");
        fs::create_dir_all(&images_b).unwrap();
        fs::write(images_b.join("image-1.png"), b"png").unwrap();

        assert_eq!(
            resolve_local_session_any_cwd_in_root(session_id, &root)
                .unwrap()
                .as_deref(),
            Some(cwd_a),
            "must anchor to the real session's cwd, not the stub's"
        );
    }
}

#[cfg(test)]
mod repo_wide_resolution_tests {
    use super::*;

    fn setup_session(root: &Path, cwd: &str, session_id: &str) {
        let encoded = crate::util::grow_home::encode_cwd_dirname(cwd);
        let dir = root.join(encoded).join(session_id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("summary.json"), "{}").unwrap();
    }

    #[test]
    fn resolves_exact_before_later_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_session(tmp.path(), "/repo/main", "session");
        setup_session(tmp.path(), "/repo/other", "session");
        let resolved = resolve_local_session_for_repo_in_root(
            "session",
            &["/repo/main", "/repo/other"],
            tmp.path(),
        )
        .unwrap();
        assert_eq!(resolved.cwd, "/repo/main");
        assert_eq!(
            resolved.resolution_kind,
            LocalSessionResolutionKind::ExactCwd
        );
    }

    #[test]
    fn resolves_same_repo_different_cwd() {
        let tmp = tempfile::TempDir::new().unwrap();
        setup_session(tmp.path(), "/repo/other", "session");
        let resolved = resolve_local_session_for_repo_in_root(
            "session",
            &["/repo/main", "/repo/other"],
            tmp.path(),
        )
        .unwrap();
        assert_eq!(resolved.cwd, "/repo/other");
        assert_eq!(
            resolved.resolution_kind,
            LocalSessionResolutionKind::SameRepoDifferentCwd
        );
    }

    #[test]
    fn returns_none_without_local_match() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(
            resolve_local_session_for_repo_in_root("missing", &["/repo/main"], tmp.path(),)
                .is_none()
        );
    }
}
