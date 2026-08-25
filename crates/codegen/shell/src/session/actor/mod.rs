#![allow(clippy::await_holding_refcell_ref)]
#![allow(clippy::arc_with_non_send_sync)]
//! Session actor implementation for the MVP ACP agent.
//!
//! Each session runs as an actor over one Timeline-derived Surface and tool context.
//! The agent owns the client connection and routes commands and events via
//! channels:
//! - Agent → Session: `SessionCommand` (prompt, cancel, shutdown)
//! - Session → Client: `session_notification` via a shared gateway handle
//!
use super::commands::{
    PromptCompletionKind, PromptTurnOk, PromptTurnResult, SessionCommand, ok_end_turn,
};
use super::handle::SessionHandle;
use super::notifications::NotificationSender;
use crate::agent::update_chunk_merge::{BufferingSettings, ReplayBuffer};
use crate::extensions::notification::SessionUpdate as GrowSessionUpdate;
use crate::extensions::notification::{RetryState, SessionNotification as GrowSessionNotification};
use crate::sampling::error::map_sampling_err_to_acp;
use crate::sampling::types::{ChatRequestMessage, ToolCallResponse, ToolDefinition};
use crate::sampling::{
    ContentPart, ConversationItem, ConversationRequest, ConversationResponse, SamplingError,
    SyntheticReason, ToolSpec, conversation_truncate_for_prompt,
};
use crate::session::ClientFsConfig;
use crate::session::fs_watch::{self, git_head_dedup_key};
use crate::session::info::Info as SessionInfo;
use crate::session::mcp_servers::McpInitStrategy;
use crate::session::mcp_servers::McpMetaConfigMap;
use crate::session::mcp_servers::McpState;
use crate::session::mcp_servers::build_pending_clients;
use crate::session::mcp_servers::mcp_server_name;
use crate::session::mcp_servers::mcp_target_str;
use crate::session::mcp_servers::mcp_transport_str;
use crate::session::mcp_servers::parse_mcp_tool_name;
use crate::session::persistence::{PersistenceHandle, PersistenceMsg, get_prompt_blob_ref};
use crate::session::prompt_parser::parse_prompt_with_skills;
use crate::session::replay_events::{SessionEvent, SessionNotification};
use crate::session::result::ExtMethodResult;
use crate::session::signals::{SessionSignalsHandle, TurnDeltaSnapshot};
use crate::session::slash_commands::{self, BuiltinAction, SlashCommandOutcome};
use crate::session::storage::SessionUpdate;
use crate::session::user_message::extract_user_query;
use crate::terminal::TerminalRunRequest;
use crate::tools::ToolContext;
use acp_transport::AcpAgentGatewaySender as GatewaySender;
use agent::AgentDefinition;
use agent::prompt::skills::SkillsConfig;
use agent_client_protocol as acp;
use agent_client_protocol::ContentBlock;
use parking_lot::Mutex;
use sampler::SamplerConfig as SamplingConfig;
use sampling_types::truncate_bytes;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(test)]
use std::sync::OnceLock;
use tokio::sync::{Mutex as TokioMutex, mpsc, oneshot};
use tokio::time::{Duration, sleep};
use tools::computer::local::LocalTerminalBackend;
use tools::implementations::BashToolInput;
use tools::implementations::grow_build::web_fetch::WebFetchConfig;
use tools::types::ToolInput;
use tools::types::output::{
    BashOutput, ReadFileOutput, ToolOutput as ToolsToolOutput, ToolRunResult,
};
use workspace::file_system::CodebaseIndexManager;
use workspace::permission::{AccessKind, ClientType, Decision, PermissionHandle};
use workspace::session::file_state::{FileStateHandle, FileStateTracker};
const SESSION_LOG: &str = "grow_session";
mod compaction;
pub(crate) mod context_recall;
mod types;
pub(crate) use types::*;
pub use types::{TodoGateDecision, TodoGateReason};
mod auth_retry;
use auth_retry::{AuthRetryDecision, AuthRetrySchedule};
mod completion_delivery;
mod goal;
mod interjection;
mod tool;
use tool::wait_for_pending_interjection;
use tool::*;
pub(in crate::session::actor) use tool::{
    MAX_ARGS_IN_ERROR, build_tool_parse_error_message, lock_path_for_args,
};
mod turn;
use turn::*;
mod workflow_run;
pub(crate) use interjection::*;
mod laziness;
pub(crate) use laziness::*;
mod hooks_plugins;
mod mcp;
mod model_switch;
mod prompt_queue;
mod slash_exec;
use super::PromptOrigin;
use super::acp_types;
use super::compaction_config;
use super::diagnostics;
use super::helpers;
use super::memory_state;
use super::timeline_persistence;
mod prompt_build;
use prompt_build::*;
mod session_mode;
use session_mode::*;
mod mcp_snapshot;
use mcp_snapshot::*;
mod tasks_cancel;
use tasks_cancel::*;
mod reminders;
use reminders::*;
pub use reminders::{CollectedTodoGateInput, TodoGateInput, evaluate_todo_gate};
mod laziness_classifier;
pub(crate) use laziness_classifier::*;
mod notification_drain;
use notification_drain::*;
mod extensions;
use extensions::*;
mod memory_dream;
use memory_dream::*;
mod goal_support;
pub(crate) use goal_support::*;
mod hook_dispatch;
use hook_dispatch::*;
mod stop_gate;
pub use stop_gate::MAX_STOP_HOOK_CONTINUATIONS_PER_TURN;
mod idle_arbitration;
mod recap;
mod rewind;
mod run_loop;
use idle_arbitration::*;
mod teardown;
use teardown::*;
mod session_setup;
mod updates;
use run_loop::*;
pub(crate) mod sideband;
mod spawn;
pub(crate) mod summary;
use super::acp_types::*;
use sideband::*;
pub use spawn::SessionThread;
pub(crate) use spawn::*;
/// Client-registered hook gates (the `grow/hooks/run` reverse request).
mod hooks;
pub(crate) struct InputItem {
    pub(crate) prompt_id: String,
    pub(crate) turn_kind: super::TurnKind,
    pub(crate) prompt_blocks: Vec<ContentBlock>,
    /// Optional client identifier from the prompt request meta (overrides session-level one)
    pub(crate) client_identifier: Option<String>,
    /// See [`SessionCommand::QueuePrompt::screen_mode`]. Diagnostic-only.
    pub(crate) screen_mode: Option<String>,
    /// See [`SessionCommand::QueuePrompt::verbatim`].
    pub(crate) verbatim: bool,
    pub(crate) json_schema: Option<serde_json::Value>,
    /// Who originated this prompt — user or auto-wake system.
    pub(crate) origin: super::PromptOrigin,
    /// Durable notification receipts atomically consumed by this synthetic
    /// turn's model-visible input. Empty for ordinary prompts.
    pub(crate) notification_ids: Vec<String>,
    pub(crate) respond_to: oneshot::Sender<PromptTurnResult>,
    /// Fired after the user message is committed to Timeline and a persistence flush
    /// barrier has completed (see `SessionCommand::QueuePrompt::persist_ack`).
    pub(crate) persist_ack: Option<oneshot::Sender<()>>,
    /// Server-authoritative prompt-queue metadata. `Some` for
    /// user-originated prompts (they appear in the shared queue); `None` for
    /// synthetic / system inputs (auto-wake, nudges, notification drains).
    pub(crate) queue_meta: Option<crate::session::prompt_queue::QueueEntryMeta>,
}
/// Task scheduling state — the only fields that remain behind `TokioMutex`.
///
/// All chat state (conversation, tokens, timing, prompt coordinates,
/// agent_edited_paths, last_compaction_prompt_index, sampling_config) has been
/// fully migrated to `ChatStateActor` via `chat_state_handle`.
/// Credentials (api_key, optional extra access key, client_version) live in
/// the `credentials` sync mutex on `SessionActor`.
struct AdmissionState {
    /// The sole owner of foreground execution. Goal's future continuation
    /// right is not foreground work and therefore cannot block user admission.
    pub(crate) foreground: ForegroundState,
    /// One coalescing manual-compaction request admitted during a running turn.
    pub(crate) pending_manual_compact: Option<Option<String>>,
    pub(crate) pending_inputs: VecDeque<InputItem>,
    /// Prompt ids held out of combine-on-promote (composer edit in progress).
    pub(crate) combine_edit_holds: std::collections::HashSet<String>,
    /// When true, notifications are buffered but not drained until genuine
    /// user re-engagement. Set by interactive Ctrl+C, cleared by a user prompt.
    pub(crate) notifications_suppressed: bool,
    /// Active prompt is still rewindable until the first outbound prompt-scoped
    /// event is emitted.
    pub(crate) rewindable: bool,
    /// Layer-3 LazinessDetector: number of `<system-reminder>` nudges
    /// injected so far in this (session, model) pair. Reset to 0 by
    /// the actor's main `select!` loop when its `model_switch_rx`
    /// watch channel fires — see the `model_switch_rx.changed()` arm
    /// in `run_session`. The cap is therefore per-(session, model):
    /// switching models is a deliberate user action that resets
    /// expectations.
    pub(crate) nudges_used_this_session: u32,
    pub(crate) recent_terminals: VecDeque<crate::session::prompt_queue::RecentPromptTerminal>,
}

pub(crate) enum ForegroundState {
    Idle,
    RegularTurn(AgentTask),
    Compaction,
}

impl ForegroundState {
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub(crate) fn regular(&self) -> Option<&AgentTask> {
        match self {
            Self::RegularTurn(task) => Some(task),
            Self::Idle | Self::Compaction => None,
        }
    }

    pub(crate) fn snapshot(&self) -> Option<::prompt_queue::ForegroundSnapshot> {
        self.regular()
            .map(|task| ::prompt_queue::ForegroundSnapshot {
                prompt_id: task.prompt_id.clone(),
                origin: task.origin.wire_name().to_string(),
                turn_kind: task.turn_kind.wire_name().to_string(),
                turn_start_ms: task.turn_start_ms,
            })
    }

    pub(crate) fn take_regular(&mut self) -> Option<AgentTask> {
        match std::mem::replace(self, Self::Idle) {
            Self::RegularTurn(task) => Some(task),
            other => {
                *self = other;
                None
            }
        }
    }
}

impl AdmissionState {
    /// Prompt id of the in-flight regular turn, if any. Foreground ownership
    /// and the FIFO share this lock, so completion and queue mutations compare
    /// against one race-free identity.
    pub(crate) fn running_prompt_id(&self) -> Option<&str> {
        self.foreground.regular().map(|t| t.prompt_id.as_str())
    }

    pub(crate) fn record_recent_terminal(
        &mut self,
        terminal: crate::session::prompt_queue::RecentPromptTerminal,
    ) {
        self.recent_terminals
            .retain(|entry| entry.prompt_id != terminal.prompt_id);
        self.recent_terminals.push_back(terminal);
        while self.recent_terminals.len() > 128 {
            self.recent_terminals.pop_front();
        }
    }
    /// Sweep `pending_inputs`, removing entries matching `drop_if` EXCEPT the
    /// running turn's own slot, and return the removed items (callers harvest
    /// them for diagnostics counts / reservation releases).
    ///
    /// Returned items still carry live `respond_to` senders that this helper
    /// does NOT resolve — dropping them unfulfilled is correct only for
    /// synthetic items (no client RPC awaits them, the current callers); a
    /// caller whose predicate can match user-originated items must resolve
    /// each returned item (see `respond_removed_prompt`) or the
    /// client's `session/prompt` hangs and fails spuriously.
    ///
    /// The guard protects the active regular turn's retained input slot from
    /// synthetic-input sweeps. Match the structured foreground identity, not
    /// a queue position: the FIFO is not an execution-owner protocol.
    pub(crate) fn sweep_pending_inputs(
        &mut self,
        drop_if: impl Fn(&InputItem) -> bool,
    ) -> Vec<InputItem> {
        let running_pid = self.foreground.regular().map(|t| t.prompt_id.clone());
        let mut dropped = Vec::new();
        let mut kept = VecDeque::with_capacity(self.pending_inputs.len());
        for item in std::mem::take(&mut self.pending_inputs) {
            if running_pid.as_deref() != Some(item.prompt_id.as_str()) && drop_if(&item) {
                dropped.push(item);
            } else {
                kept.push_back(item);
            }
        }
        self.pending_inputs = kept;
        dropped
    }
}
/// Canonical "session is idle and safe to inject a synthetic turn"
/// predicate. The post-turn idle consumers — `maybe_drain_notifications`
/// (notification batching), `maybe_fire_laziness_check` (Layer 3 classifier),
/// and `arm_idle_notification` (idle-notification debounce) — all consult this
/// so they share one definition of idleness, with no drift between them.
///
/// Returns `true` exactly when: no turn or manual compaction is running, no
/// user prompt is queued, and interactive Ctrl+C has not suppressed
/// notifications pending genuine user re-engagement.
fn is_session_idle_for_injection(state: &AdmissionState) -> bool {
    state.foreground.is_idle() && state.pending_inputs.is_empty() && !state.notifications_suppressed
}
/// Canonical actor-owned blocker for idle unload. An Active Goal remains
/// resident because it owns the right to request the next idle continuation.
/// A parked Plan approval is also live work even though its reverse-request
/// runs in a detached task.
fn session_has_work(
    state: &AdmissionState,
    goal_status: Option<crate::session::goal_tracker::GoalStatus>,
    has_parked_plan_approval: bool,
) -> bool {
    !state.foreground.is_idle()
        || state.pending_manual_compact.is_some()
        || !state.pending_inputs.is_empty()
        || goal_status == Some(crate::session::goal_tracker::GoalStatus::Active)
        || has_parked_plan_approval
}
/// Data carried from prepare_tool_call → dispatch_tool → finalize.
#[derive(Debug, Clone)]
pub(crate) struct PreparedToolCall {
    /// The model's tool call ID (for tool_result matching).
    call_id: String,
    /// ACP-internal tool call ID.
    tool_call_id: acp::ToolCallId,
    /// The tool name as requested by the model.
    tool_name: String,
    /// The raw arguments string (for post_tool_use hook payload).
    raw_arguments: String,
    /// Parsed JSON arguments ready for bridge.call().
    parsed_args: serde_json::Value,
    /// Model ID at time of call.
    model_id: String,
    /// Whether concatenated JSON recovery was used, and how many objects were found.
    concatenated_json_count: usize,
    /// Resolved target for meta-dispatch tools (`use_tool`, `CallMcpTool`);
    /// `None` for ordinary tools. See [`ToolInput::dispatch_target_name`].
    dispatch_target_name: Option<String>,
    /// Authority projected from the frozen typed arguments. This, never the
    /// descriptor ceiling, decides call authorization and write coordination.
    required_access: tool_protocol::ToolAccess,
    /// One-shot authorization proof bound to this exact frozen invocation.
    permit: ToolCallPermit,
    /// True when this native call writes a session Workflow draft. Dispatch
    /// rechecks live Behavior while holding Workflow admission.
    workflow_draft_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpPermitBinding {
    server: String,
    client_id: u64,
    generation: u64,
    max_access: tool_protocol::ToolAccess,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolCallPermit {
    call_id: String,
    tool_name: String,
    dispatch_target_name: Option<String>,
    canonical_args_hash: String,
    cwd: PathBuf,
    descriptor_max: tool_protocol::ToolAccess,
    required_access: tool_protocol::ToolAccess,
    actor_source: String,
    actor_epoch: Option<u64>,
    mcp: Option<McpPermitBinding>,
    consumed: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone)]
pub(crate) struct ToolDispatchAuthority {
    bridge: Arc<tools::bridge::ToolBridge>,
    subagent: Option<crate::session::subagent_capability::SubagentCapabilityState>,
    mcp_state: Arc<TokioMutex<McpState>>,
    cwd: PathBuf,
    actor_source: String,
}
impl PreparedToolCall {
    /// The tool name hooks see: the resolved dispatch target, else the wire name.
    /// The single source for the resolved name across the dispatch-phase hook
    /// events (PostToolUse / PostToolUseFailure) and their diagnostics labels.
    pub(crate) fn hook_tool_name(&self) -> &str {
        self.dispatch_target_name
            .as_deref()
            .unwrap_or(&self.tool_name)
    }
}
/// One memoized model's auth state, keyed by model id; see
/// [`SessionActor::model_auth_memo`] for the invalidation contract.
#[derive(Clone)]
pub(crate) struct ModelAuthMemo {
    pub(crate) model_id: String,
    pub(crate) facts: crate::agent::config::ModelAuthFacts,
    pub(crate) provider: Option<crate::auth::AuthProviderRef>,
}

/// Session-local MCP policy and readiness state.
///
/// This groups MCP configuration and reminder bookkeeping without taking
/// ownership of foreground admission, Timeline notifications, or MCP process
/// lifetimes. The individual synchronization primitives retain their existing
/// boundaries.
struct McpSessionState {
    strategy: McpInitStrategy,
    initial_client_servers: Vec<acp::McpServer>,
    tool_metadata_snapshot: Arc<std::sync::Mutex<crate::session::tool_index::ToolMetadataSnapshot>>,
    announced_servers:
        Mutex<HashMap<String, tools::implementations::search_tool::ServerFingerprint>>,
    reminder_mode: McpReminderMode,
    reminder_dirty: Arc<std::sync::atomic::AtomicBool>,
    connecting_reminder_injected: std::cell::Cell<bool>,
    handshakes_done: Arc<tokio::sync::Notify>,
}

/// Session-local hook discovery and workspace context.
///
/// Plugin registry state deliberately remains on [`SessionActor`] as a
/// separate lifecycle concern.
struct HookSessionState {
    registry: std::cell::RefCell<Option<Arc<::hooks::discovery::HookRegistry>>>,
    client_hooks: std::cell::RefCell<crate::extensions::hooks::ClientHooks>,
    resolved_workspace_root: String,
    vcs_kind: workspace::session::git::VcsKind,
    load_errors: std::cell::RefCell<Vec<String>>,
}

/// Phase 3: Post-flight handling after dispatch (inline in execute_tool_calls for now).
pub(crate) struct SessionActor {
    pub(crate) session_info: SessionInfo,
    /// Canonical storage location of this Timeline entity. This is explicit
    /// because child entities are nested under their parent and cannot be
    /// reconstructed from `cwd + id`.
    pub(crate) session_dir: PathBuf,
    /// Identity-checked directory handle shared with the persistence actor.
    /// `session_dir` is only a display label; all session-contained I/O must
    /// descend from this capability.
    pub(crate) session_directory: std::sync::Arc<crate::session::storage::ContainedDirectory>,
    /// Serializes notification artifact writes, Timeline admission/resolution,
    /// and post-commit reclamation so a same-content receipt cannot race an
    /// in-flight content-addressed payload deletion.
    pub(crate) notification_artifact_gate: TokioMutex<()>,
    /// ACP method selected for this BYOK-only session.
    pub(crate) auth_method_id: crate::agent::auth_method::SharedAuthMethodId,
    /// Memoized per-model auth state, read through
    /// [`SessionActor::model_auth_facts`] and
    /// [`SessionActor::model_auth_provider`].
    ///
    /// A fresh `Unknown` (config currently unparseable) falls back to the
    /// last definite value for the same model rather than demoting a live
    /// session to non-refreshable api-key mode. Because a config edit can
    /// turn the selected model into a per-model BYOK model without changing
    /// its id, keying on the id alone is insufficient: each model/credential
    /// chokepoint must clear this memo (`replace(None)`).
    pub(crate) model_auth_memo: std::cell::RefCell<Option<ModelAuthMemo>>,
    state: TokioMutex<AdmissionState>,
    /// Notification transport: gateway, persistence channel, replay buffer.
    pub(crate) notifications: NotificationSender,
    pub(crate) permissions: PermissionHandle,
    pub(crate) tool_context: ToolContext,
    /// Managed Read-deny glob patterns, resolved once at construction and
    /// (re-)injected into the ToolBridge so the Grep tool excludes policy-forbidden
    pub(crate) deny_read_globs: Vec<String>,
    /// Consolidated MCP state (configs, clients, init status) protected by a single lock.
    /// This ensures atomicity when updating configs or checking initialization status.
    pub(crate) mcp_state: Arc<TokioMutex<McpState>>,
    mcp: McpSessionState,
    /// Actor-based chat state handle — manages conversation, tokens, timing, and persistence.
    /// Also stores credentials (api_key, optional extra access key,
    /// client_version) opaquely.
    pub(crate) chat_state_handle: chat_state::ChatStateHandle,
    /// Current running prompt/turn id, shared with SessionHandle.
    pub(crate) current_prompt_id: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    pub(crate) unattributed_background_usage: std::sync::atomic::AtomicBool,
    /// Open blocking reverse-requests (permission / question / plan-approval),
    /// keyed by `tool_call_id`. Shared with `SessionHandle` so the roster can
    /// read it synchronously to surface `NeedsInput`. Mutated by
    /// `PendingInteractionGuard` at each reverse-request site. Never persisted.
    pub(crate) pending_interactions: crate::session::pending_interaction::PendingInteractions,
    pub(crate) compactions_remaining: std::cell::Cell<Option<sampling_types::CompactionsRemaining>>,
    pub(crate) compaction_at_tokens: std::cell::Cell<Option<sampling_types::CompactionAtTokens>>,
    /// Server-side doom-loop check policy, resolved once at spawn by
    /// `Config::resolve_doom_loop_recovery`; `None` = disabled.
    /// `reconstruct_full_config` threads it into the sampler config, and the
    /// sampler itself sends the matching `x-grow-doom-loop-check` header.
    pub(crate) doom_loop_recovery: Option<sampling_types::DoomLoopRecoveryPolicy>,
    /// Diagnostic-only per-turn doom-loop recovery tally (attempts, whether a
    /// budget-spent accept happened, tightest trigger label). Accumulated by
    /// the event drainer, taken at turn end for the per-turn analytics event.
    pub(crate) doom_loop_turn_tally: parking_lot::Mutex<crate::session::signals::DoomLoopTurnTally>,
    /// File state tracker for rewind functionality
    pub(crate) file_state_tracker: Arc<FileStateTracker>,
    /// Last prompt text before the most recent rewind.
    /// When set, the next `prompt()` compares its text to distinguish
    /// regeneration (same text) from edit-and-retry (different text).
    pub(crate) rewind_pending_prompt: std::sync::Mutex<Option<String>>,
    /// Startup hints for the session: currently responsible for customizing the user message prefix and the git status mode (fast no untracked for non-interactive mode)
    pub(crate) startup_hints: StartupHints,
    /// Live, non-persisted grant state for a subagent session.
    pub(crate) subagent_capabilities:
        Option<crate::session::subagent_capability::SubagentCapabilityState>,
    /// Compaction configuration and runtime state.
    pub(crate) compaction: super::compaction_config::CompactionConfig,
    /// Session-owned turn-end continuation gate. Agent switches cannot alter
    /// this runtime policy.
    pub(crate) todo_gate: reminders::TodoGateConfig,
    /// Memory subsystem: storage, flush config, injection state, diagnostics.
    pub(crate) memory: super::memory_state::SessionMemory,
    /// Diagnostic counters for session summary.
    pub(crate) session_start: std::time::Instant,
    /// Per-chunk idle timeout for inference streaming. If no SSE chunk is received
    /// within this duration, the stream is aborted with a non-retryable error.
    /// Resolved at construction: per-model config.toml → remote settings → 300s default.
    pub(crate) inference_idle_timeout: std::cell::Cell<Duration>,
    pub(crate) max_retries: std::cell::Cell<u32>,
    /// Immutable session snapshot of `[subagents].classifier_input`. Permission
    /// judgments are latency-bounded and must never launch uncancellable
    /// blocking config reads on the session runtime.
    pub(crate) subagent_classifier_input: crate::config::SubagentClassifierInput,
    /// Maximum tool-use turns before the session stops. `None` = unlimited.
    pub(crate) max_turns: Option<usize>,
    /// Pending mid-turn interjections from the user (Ctrl+Enter).
    /// Pushed by `SessionCommand::Interject` handler, drained at safe
    /// points in `process_conversation_turn`. Internally synchronized.
    pub(crate) pending_interjections: InterjectionBuffer<acp::ImageContent>,
    /// Results of waits that user steering moved to the background. Keeps the
    /// original tool result paired while routing eventual completion through
    /// a hidden system reminder.
    pub(crate) completion_delivery: completion_delivery::CompletionDeliveryTracker,
    /// Hidden system reminders that arrived while a turn was running (skill
    /// announcements and Goal control-plane revisions). Flushed at the same
    /// safe points as `pending_interjections` plus on cancel/idle.
    pub(crate) pending_system_reminders: Mutex<Vec<ConversationItem>>,
    /// Idle flush timeout: `None` = disabled, `Some(duration)` = flush after inactivity.
    pub(crate) idle_flush_timeout: Option<std::time::Duration>,
    /// Periodic dream check interval: `None` = disabled.
    pub(crate) dream_check_timeout: Option<std::time::Duration>,
    /// Conversation length at last idle flush — skip if unchanged (no new messages).
    pub(crate) last_idle_flush_conversation_len: std::sync::atomic::AtomicUsize,
    /// Internal event queue for actor-owned replay buffering and flush barriers.
    pub(crate) event_tx: mpsc::UnboundedSender<SessionEvent>,
    /// Central idle-arbiter wake. Its select branch is ordered after the user
    /// command mailbox, so queued user work wins a simultaneous wake.
    pub(crate) idle_arbiter: Arc<tokio::sync::Notify>,
    /// Buffering settings captured at session creation. The concrete ReplayBuffer
    /// is owned by `run_session()`.
    pub(crate) buffering_settings: Option<BufferingSettings>,
    /// Client identifier for diagnostics - passed from the MvpAgent (extracted from initialize meta)
    pub(crate) client_identifier: Option<String>,
    /// Origin client for User-Agent on sampling requests.
    pub(crate) origin_client: Option<crate::http::OriginClientInfo>,
    /// Session-local usage and lifecycle signals.
    pub(crate) signals_handle: SessionSignalsHandle,
    /// The fully-built Agent: owns the ToolBridge, stable prompt head, typed
    /// role projection, and AgentDefinition. Session lifecycle policy remains
    /// on this actor.
    /// Wrapped in `RefCell` for mid-session mutation (skill refresh, prompt regen).
    /// Safe: session actor is single-threaded (LocalSet), no concurrent access.
    pub(crate) agent: std::cell::RefCell<agent::Agent>,
    /// Dedup slot for `grow/git_head_changed`, shared with the fs-watch
    /// `GitHead` consumer (see `git_head_dedup_key`).
    pub(crate) last_reported_branch: Arc<parking_lot::Mutex<Option<String>>>,
    /// Client opted into `grow/gitHeadChanged`. When false (headless/SDK),
    /// `maybe_notify_git_branch` no-ops — no git subprocess.
    git_head_enabled: bool,
    /// Shared models manager for etag-triggered refresh from response headers.
    pub(crate) models_manager: crate::agent::models::ModelsManager,
    /// Only the primary that created the shared permission manager may stop
    /// it. Children merely clone the handle.
    pub(crate) owns_permission_manager: bool,
    /// Primary-owned receiver bridge. Shutdown joins it after the permission
    /// actor closes its event sender and before the final persistence flush.
    pub(crate) permission_audit_bridge: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Stable display path for forked sessions (original project path).
    ///
    /// Used by `build_user_message_prefix` (user-message `Workspace Path`),
    /// `PathRewriter` (tool result path sanitization), and hunk tracker
    /// (client-facing diff paths). AgentBuilder also uses it for model-facing
    /// Skill and AGENTS.md paths.
    ///
    /// Set once at session spawn from the `prompt_display_cwd` parameter
    /// (e.g. for forked sessions that should display the original project
    /// path). Uses `OnceLock` for lock-free reads, set-once semantics, and
    /// `&self` mutability (SessionActor is behind `Arc`).
    pub(crate) display_cwd: std::sync::OnceLock<String>,
    /// Actor-owned stable catalog selection. Provider wire routing lives in
    /// ChatState's SamplingConfig; keeping the catalog axis here prevents UI
    /// mirrors from inventing the `from` side of durable model transitions.
    pub(crate) selected_model_id: std::cell::RefCell<acp::ModelId>,
    /// First skill the current prompt activated via its slash-skill path,
    /// recorded as `skill.name` on the turn span. Reset at the start of each
    /// prompt (`handle_prompt`), so it never leaks across turns.
    pub(crate) active_skill: parking_lot::Mutex<Option<String>>,
    /// Behavior captured atomically when the current user-visible turn wins
    /// foreground admission. Selection changes never mutate an admitted turn.
    pub(crate) turn_behavior: Arc<parking_lot::Mutex<tool_types::BehaviorId>>,
    /// Session-scoped primary-Agent Behavior controller. It owns the selected
    /// collaboration protocol and Plan phase, not permissions or runtimes.
    pub(crate) behavior: Arc<parking_lot::Mutex<crate::session::behavior::BehaviorCoordinator>>,
    /// Monotonic revision of the Timeline control projection.
    pub(crate) control_revision: Arc<std::sync::atomic::AtomicU64>,
    /// Whether goal mode (`/goal`) is enabled for this session (feature flag).
    pub(crate) goal_enabled: bool,
    pub(crate) background_workflows_enabled: bool,
    goal_runtime_available: std::sync::atomic::AtomicBool,
    /// Durable long-lived Goal state. Idle continuation authority is runtime
    /// state and is deliberately not persisted in this value.
    pub(crate) goal_tracker: Arc<parking_lot::Mutex<crate::session::goal_tracker::GoalTracker>>,
    /// `task_id`s of background tasks (and monitors) that originated during
    /// a Goal turn, including surviving tasks reparented from delegated
    /// children. Their late
    /// auto-wake completions are dropped by [`Self::maybe_drain_notifications`]
    /// regardless of the goal's current status, so a leftover dev/verification
    /// server that completes after the run ended (Blocked / paused / cleared)
    /// cannot wake the idle parent. Reset only when a new Goal starts; clearing
    /// the old Goal must not erase ownership of work that is still running.
    pub(crate) goal_turn_task_ids: parking_lot::Mutex<std::collections::HashSet<String>>,
    pub(crate) goal_command_rx: std::cell::RefCell<
        Option<
            tokio::sync::mpsc::UnboundedReceiver<
                tools::implementations::grow_build::update_goal::GoalCommand,
            >,
        >,
    >,
    pub(crate) goal_command_tx: tokio::sync::mpsc::UnboundedSender<
        tools::implementations::grow_build::update_goal::GoalCommand,
    >,
    pub(crate) workflow_manager:
        Arc<tokio::sync::Mutex<crate::session::workflow::manager::WorkflowManager>>,
    pub(crate) workflow_tx: tokio::sync::mpsc::UnboundedSender<
        tools::implementations::grow_build::workflow::WorkflowEnvelope,
    >,
    /// Background-computed user-message prefix, injected before the first prompt.
    pub(crate) deferred_prefix: TaskSlot<String>,
    /// Debounced idle notification state. Tests that construct the actor
    /// directly leave this disabled.
    pub(crate) idle_prompt_extension: Option<IdlePromptExtension>,
    /// Local date last surfaced to the model, via the `<user_info>` prefix (session start,
    /// compaction, model switch) or a date-rollover `<system-reminder>`. Plain resume reuses the
    /// cached prefix. Drives [`SessionActor::maybe_inject_date_rollover_reminder`].
    pub(crate) last_announced_local_date: std::cell::Cell<chrono::NaiveDate>,
    /// Prompt index when search_tool last ran. -1 = never. Used for turns_since_last_search.
    pub(crate) last_search_prompt_index: std::sync::atomic::AtomicI64,
    /// Timestamp (millis since epoch) of the last successful API request.
    /// Used to detect session resume after idle and proactively refresh model metadata.
    pub(crate) last_api_request_at: std::sync::atomic::AtomicI64,
    hooks: HookSessionState,
    /// Plugin registry snapshot for this session. Updated on `/plugins reload`.
    /// `RefCell` for mid-session reload from `&self` methods.
    pub(crate) plugin_registry:
        std::cell::RefCell<Option<std::sync::Arc<agent::plugins::PluginRegistry>>>,
    /// Shared handle to the agent-level plugin registry.
    /// Used by `/plugins reload` to trigger a rebuild that new sessions see.
    pub(crate) plugin_registry_handle: Option<agent::plugins::SharedPluginRegistryHandle>,
    /// Centralized event tracking: event log, turn-end guard, active tool,
    /// doom loop terminate flag. All event-related state lives here.
    pub(crate) events: crate::session::events::EventTracker,
    /// Turn number captured at the start of each turn (before prompt index
    /// increment).  Used by `ToolCallStarted` bridge emissions so they
    /// report the same turn number as `TurnStarted` / `TurnEnded`.
    pub(crate) current_turn_number: std::cell::Cell<u64>,
    /// Recap rate-limit watermark (`main_turns` of last finished recap; `0` = none).
    pub(crate) last_recap_main_turn: std::cell::Cell<usize>,
    /// True while a recap model call is in flight (auto or manual). Prevents
    /// concurrent `spawn_local` recaps from racing watermark restore.
    pub(crate) recap_in_flight: std::cell::Cell<bool>,
    /// Bumped on each real user prompt (queue accept + turn start); in-flight
    /// recap suppresses emit if this changes before commit.
    pub(crate) recap_epoch: std::cell::Cell<u64>,
    /// True while THIS session has a prompt turn in flight (RAII-guarded in
    /// `handle_prompt`, like `tool_context.is_turn_active` — which is the
    /// agent-wide coordinator flag shared by all sessions and so unusable
    /// for per-session decisions). `Arc` so it can be re-checked inside the
    /// chat-state actor's `RepairHistory` handler.
    pub(crate) session_turn_active: Arc<std::sync::atomic::AtomicBool>,
    /// Per-turn barrier that orders the streamed message against the turn's
    /// tool calls.
    ///
    /// The sampler's events (text/thought chunks) are emitted by a separate
    /// drainer task (`handle_sampling_event`), while the turn loop emits the
    /// canonical client `ToolCall` notifications itself after
    /// `run_turn_via_sampler` returns. Both call `send_update`, which allocates
    /// the process-global, monotonically-increasing `eventId` AT CALL TIME (see
    /// `generate_event_id`). Because the two run as distinct tasks on the
    /// session `LocalSet`, the tool call's `send_update` could interleave
    /// BETWEEN two still-draining text chunks — allocating an `eventId` mid
    /// message and splitting the assistant text around the tool call on every
    /// attached client (the eventId order is what clients render in).
    ///
    /// To keep all of a turn's `eventId`s in stream order, `run_turn_via_sampler`
    /// installs a sender here before submitting and awaits the receiver after the
    /// response arrives; the drainer fires it the moment it processes the
    /// terminal `SamplingEvent::Completed` (every text/thought chunk has been
    /// `send_update`d by then). `None` between turns.
    pub(crate) turn_stream_drained: parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Handle to the per-session `sampler` actor.
    ///
    /// Live sessions get a real handle from `spawn_session_actor`;
    /// tests and other constructor sites use `SamplerHandle::noop()`.
    /// All inference flows through this handle.
    pub(crate) sampler_handle: sampler::SamplerHandle,
    /// Cached recipe for constructing this session's [`agent::Agent`].
    ///
    /// Populated once at session spawn and then reused by
    /// `handle_rebuild_agent_for_definition` to build a fresh `Agent`
    /// (system prompt, [`tools::bridge::ToolBridge`], tool
    /// registry, and tool name aliases) when the user selects another Agent.
    /// Session lifecycle policy is deliberately absent: an Agent switch must
    /// not alter TodoGate, compaction, permission, or provider state.
    ///
    /// See [`crate::session::agent_rebuild`] for the canonical-construction
    /// invariant.
    pub(crate) rebuild_spec: Arc<crate::session::agent_rebuild::AgentRebuildSpec>,
    /// Explicitly configured vision model for `read_file` image/PDF results.
    /// `None` leaves those images on the active session model path.
    pub(crate) image_description_model: parking_lot::RwLock<Option<String>>,
    /// One-shot title inference capability. `None` means the session already
    /// has a title or is a child session that never generates one.
    pub(crate) session_title_route:
        std::cell::RefCell<Option<crate::session::actor::summary::SessionTitleRoute>>,
    /// Cache auxiliary image outputs by content and prompt fingerprint.
    pub(crate) image_describe_cache: Arc<crate::session::image_describe::ImageDescribeCache>,
    /// Per-subagent exactly-once marker keyed by `subagent_id`; Goal usage is
    /// charged from the acknowledged child usage-ledger fold, never progress.
    pub(crate) subagent_token_records: parking_lot::Mutex<HashMap<String, SubagentTokenRecord>>,
    pub(crate) workspace_ops: workspace::WorkspaceOps,
    /// Layer-3 LazinessDetector: monotonic counter bumped whenever a
    /// fresh (non-synthetic) user prompt arrives at the actor.
    /// `maybe_fire_laziness_check` snapshots the value at start and
    /// polls for changes in its idle-wait loop.
    ///
    /// **vs. `tokio::sync::Notify`** (the original design):
    /// generation-counter snapshot+compare avoids the stored-permit
    /// hazard. A `notify_one()` emitted before the classifier spawns
    /// would cause the spawn-later `.notified()` arm to fire
    /// immediately, aborting the classifier on the very first idle
    /// period after any real turn. An `AtomicU64` has no such hazard.
    ///
    /// **vs. `tokio::sync::watch::Sender<u64>`** (the mirror design
    /// used for `ModelsManager::model_switch_watch`): single-consumer
    /// cardinality here. The only reader of `user_input_generation`
    /// is the per-actor laziness task's snapshot+compare; no
    /// main-loop subscriber needs a wake-on-change for user input
    /// (the prompt handler is itself the *producer*, in the same
    /// task). `tokio::sync::watch::Sender` is internally lock-bearing
    /// (an `RwLock<T>` per `tokio` source), so adopting it for this
    /// field would re-introduce a per-actor lock for a use case an
    /// `AtomicU64` already covers correctly. Model-switch differs
    /// because its main-loop arm DOES need a wakeup to zero the
    /// per-session nudge counter — the watch channel's `.changed()`
    /// is the right primitive there.
    pub(crate) user_input_generation: std::sync::atomic::AtomicU64,
    /// Session-scoped `--laziness-debug-log <path>`. When `Some`, the
    /// Layer-3 classifier fires after every turn end (bypassing the
    /// idle wait, the per-model enable gate, and the nudge cap), and
    /// the full outcome is appended as a JSONL line to this file.
    /// Observation-only — no nudges are ever injected when this is
    /// `Some`. `Arc<Path>` because the path is immutable after
    /// session spawn; concurrent appends rely on `O_APPEND`'s atomic
    /// guarantee for writes under `PIPE_BUF` (JSONL lines fit).
    pub(crate) laziness_debug_log: Option<std::sync::Arc<std::path::Path>>,
}
impl SessionActor {
    /// Get the signals handle for tracking session events.
    fn signals_handle(&self) -> SessionSignalsHandle {
        self.signals_handle.clone()
    }
    fn emit_event(&self, event: crate::session::events::Event) {
        self.events.emit(event);
    }
    async fn emit_turn_ended(
        &self,
        outcome: crate::session::events::TurnOutcomeLabel,
        terminal: chat_state::TurnTerminal,
        category: Option<crate::session::events::CancellationCategory>,
        context: Option<serde_json::Value>,
    ) -> Result<(), acp::Error> {
        self.events
            .emit_turn_ended(outcome, terminal, category, context)
            .await
            .map_err(|error| {
                acp::Error::internal_error()
                    .data(format!("turn terminal was not durably recorded: {error}"))
            })
    }
    /// Current model ID for structured tracing span attributes. Reads from chat_state_handle
    /// so it always reflects the latest model override — no stale cached field.
    /// Returns "unknown" if no sampling config is set.
    async fn current_model_id(&self) -> String {
        self.chat_state_handle
            .get_sampling_config()
            .await
            .map(|c| c.model)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "unknown".to_string())
    }
    /// Build a hook run context for dispatching hook events.
    fn session_id_string(&self) -> String {
        self.session_info.id.0.to_string()
    }
    /// Send a before-turn hook via the local workspace channel.
    /// Fire-and-forget — failures are logged but do not interrupt the turn.
    async fn send_before_turn_event(&self, payload: tool_protocol::turn_hook::BeforeTurnPayload) {
        self.workspace_ops
            .on_before_turn(&self.session_id_string(), &payload)
            .await;
    }
    /// Send an after-turn hook via the local workspace channel.
    /// Fire-and-forget — failures are logged but do not interrupt the turn.
    async fn send_after_turn_event(&self, payload: tool_protocol::turn_hook::AfterTurnPayload) {
        self.workspace_ops
            .on_after_turn(&self.session_id_string(), &payload)
            .await;
    }
    /// Compute the live command availability snapshot for this session.
    ///
    /// Convenience wrapper that fetches the toolset and delegates to
    /// `build_command_availability`. Use this on the inbound resolve
    /// path; the outbound advertise path enumerates tools once and
    /// shares the slice across both calls (see
    /// `send_available_commands_update`).
    async fn command_availability(&self) -> slash_commands::CommandAvailability {
        let tool_names = self.registered_tool_names().await;
        let has_workflow_runs = self.workflow_tracker().await.lock().has_public_runs();
        self.build_command_availability(&tool_names, has_workflow_runs)
    }
    /// Build the `CommandAvailability` snapshot from a precomputed slice
    /// of tool names plus the live session-scoped capability state.
    ///
    /// Single source of truth for the seven gate fields -- both
    /// `command_availability` (resolve path) and
    /// `send_available_commands_update` (advertise path) call this so
    /// the two paths can never drift.
    fn build_command_availability(
        &self,
        tool_names: &[String],
        has_workflow_runs: bool,
    ) -> slash_commands::CommandAvailability {
        use tools::implementations::memory::{MEMORY_GET_TOOL_NAME, MEMORY_SEARCH_TOOL_NAME};
        let memory_read_registered = tool_names
            .iter()
            .any(|n| n == MEMORY_SEARCH_TOOL_NAME || n == MEMORY_GET_TOOL_NAME);
        let goal = goal_support::goal_runtime_available_from_tools(self.goal_enabled, tool_names);
        slash_commands::CommandAvailability {
            memory: self.memory.is_enabled() && memory_read_registered,
            memory_configured: self.memory.backend_params.is_some(),
            scheduler: tool_names
                .iter()
                .any(|n| n == tools::implementations::grow_build::SCHEDULER_CREATE_TOOL_NAME),
            hooks: self.hooks.registry.borrow().is_some(),
            plugins: self.plugin_registry.borrow().is_some(),
            goal,
            workflows: tool_names
                .iter()
                .any(|n| n == tools::implementations::grow_build::workflow::WORKFLOW_TOOL_NAME),
            workflow_management: has_workflow_runs,
            workflow_behavior: self.behavior.lock().behavior() == tool_types::BehaviorId::Workflow,
        }
    }
    /// Names of every tool registered with the session's tool bridge.
    ///
    /// Async wrapper that fetches `tool_definitions()` and projects to
    /// the `function.name` field. Allocates one `Vec<String>` per call;
    /// callers that need both gating and the wire payload should call
    /// once and pass the slice to `build_command_availability`.
    async fn registered_tool_names(&self) -> Vec<String> {
        let bridge = self.agent.borrow().tool_bridge().clone();
        bridge
            .tool_definitions()
            .await
            .into_iter()
            .map(|td| td.function.name)
            .collect()
    }
    pub(crate) async fn workflow_tracker(
        &self,
    ) -> Arc<parking_lot::Mutex<crate::session::workflow::tracker::WorkflowTracker>> {
        self.workflow_manager.lock().await.tracker()
    }
    /// Send visible text output to the TUI from a slash command.
    ///
    /// Uses `AgentMessageChunk` so the text appears in the conversation
    /// scrollback. The session actor owns replay flushing; callers running in
    /// that actor must never enqueue a flush event and wait for the same actor
    /// to acknowledge it.
    async fn send_slash_command_output(&self, text: &str) {
        self.send_slash_command_output_with_meta(text, None).await;
    }
    async fn send_host_turn_slash_command_output(&self, text: &str) {
        let mut chunk_meta = serde_json::Map::new();
        chunk_meta.insert(
            crate::session::storage::HOST_TURN_META_KEY.into(),
            serde_json::json!(true),
        );
        self.send_slash_command_output_with_meta(text, Some(chunk_meta))
            .await;
    }
    async fn send_slash_command_output_with_meta(
        &self,
        text: &str,
        meta: Option<serde_json::Map<String, serde_json::Value>>,
    ) {
        self.send_update(
            acp::SessionUpdate::AgentMessageChunk(
                acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                    text.to_string(),
                )))
                .meta(meta),
            ),
            None,
        )
        .await;
    }
}
impl SessionActor {
    /// Owned handle to the active `ToolBridge`. Used by async methods
    /// that need to drop the `RefCell::Ref<Agent>` borrow before
    /// awaiting — `Arc::clone` is cheap, and an outstanding `Ref`
    /// across `.await` would panic if anything on the suspended path
    /// did `self.agent.borrow_mut()`.
    fn tool_bridge_handle(&self) -> Arc<tools::bridge::ToolBridge> {
        Arc::clone(self.agent.borrow().tool_bridge())
    }
}
#[cfg(test)]
mod tests;
/// Drop guard that records aggregate turn metrics on the current tracing span
struct TurnMetrics {
    turn_tool_count: u64,
    turn_model_calls: u64,
    span: tracing::Span,
}
impl TurnMetrics {
    fn new() -> Self {
        Self {
            turn_tool_count: 0,
            turn_model_calls: 0,
            span: tracing::Span::current(),
        }
    }
    fn record_model_response(&mut self, num_tool_calls: usize) {
        self.turn_model_calls += 1;
        self.turn_tool_count += num_tool_calls as u64;
    }
}
impl Drop for TurnMetrics {
    fn drop(&mut self) {
        self.span.record("turn_tool_count", self.turn_tool_count);
        self.span.record("turn_model_calls", self.turn_model_calls);
    }
}
