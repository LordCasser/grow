#![cfg_attr(rustfmt, rustfmt::skip)]
#![allow(unused_imports)]
use std::path::PathBuf;
use std::sync::OnceLock;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};
use tokio::sync::mpsc;
/// A `'static` reference to a value on a single-threaded `LocalSet`.
///
/// Encapsulates the raw-pointer pattern used when `spawn_local` tasks need
/// `&T` but the borrow checker requires `'static`. The pointer is valid as
/// long as:
///
/// 1. `T` is heap-allocated and never moved (e.g., behind `Rc` or owned by
///    the ACP connection for the process lifetime).
/// 2. All access happens on the **same** `LocalSet` thread (no `Send`).
/// 3. The `LocalRef` does not outlive the `LocalSet`.
///
/// These invariants are upheld by construction: `LocalRef` is `!Send`
/// (via `*const T`) and only used inside `spawn_local` closures on the
/// agent's `LocalSet`.
pub(crate) struct LocalRef<T> {
    ptr: *const T,
}
impl<T> LocalRef<T> {
    /// Create a `LocalRef` from a shared reference.
    ///
    /// # Safety contract (enforced by the caller, not by the type system)
    ///
    /// The referenced `T` must live for the entire duration of the `LocalSet`
    /// and must not be moved or deallocated while any `LocalRef` clone exists.
    pub(crate) fn new(val: &T) -> Self {
        Self { ptr: val as *const T }
    }
    /// Dereference back to `&T`.
    ///
    /// # Safety
    ///
    /// Safe because the caller of `new()` guarantees the pointee is alive
    /// and pinned, and `LocalRef` is `!Send` (only used on the same thread).
    pub(crate) fn get(&self) -> &T {
        unsafe { &*self.ptr }
    }
}
impl<T> Clone for LocalRef<T> {
    fn clone(&self) -> Self {
        Self { ptr: self.ptr }
    }
}
use agent_client_protocol::Client as _;
use agent_client_protocol::{self as acp, AuthenticateResponse};
use indexmap::IndexMap;
use tokio::sync::oneshot;
use xai_acp_lib::AcpAgentGatewaySender as GatewaySender;
use crate::agent::auth_method;
use crate::agent::config::{self, Config as AgentConfig, ModelEntry, resolve_credentials};
use crate::agent::folder_trust;
use crate::agent::models::{resolve_catalog_key, selectable_catalog_key_for_persisted};
use crate::agent::session_config;
use grow_sampling_types::{
    REASONING_EFFORT_META_KEY, ReasoningEffortOption, reasoning_effort_meta_value,
    supports_reasoning_effort_meta,
};
use crate::agent::update_chunk_merge;
use crate::auth::AuthManager;
use crate::extensions::notification::{SessionNotification, SessionUpdate};
use grow_diagnostics::session_ctx::log_event;
use grow_workspace::file_system::{AcpSessionFs, CodebaseIndexManager, LocalFs};
use grow_workspace::permission::{ClientType, PermissionEvent};
use crate::sampling::Client as OaiCompatClient;
use crate::sampling::error::map_sampling_err_to_acp;
use crate::session::mcp_servers::{McpMetaConfigMap, parse_mcp_meta_config};
use grow_sampler::SamplerConfig as SamplingConfig;
use crate::session::persistence::PersistenceHandle;
use crate::session::worktree::BackgroundCopyContext;
use crate::session::{
    ParsedPromptInfo, SCHEDULER_BACKGROUND_LOOPS_META_KEY, SessionCommand, SessionHandle,
    SessionLiveState, SessionThread, info::Info as SessionInfo, spawn_session_on_thread,
};
use crate::terminal::{AcpTerminalRunner, TerminalRunner};
use crate::tools::ToolContext;
use tokio_util::sync::CancellationToken;
use grow_paths::AbsPathBuf;
use grow_workspace::session::git::GitDiscoveryResult;
use xai_hunk_tracker::HunkTrackerActor;

fn parse_agent_profile_from_meta(meta: Option<&acp::Meta>) -> Option<grow_agent::AgentDefinition> {
    let value = meta?.get("agentProfile")?;
    if value.is_object() {
        return grow_agent::AgentDefinition::from_json(value)
            .map_err(|error| tracing::warn!(%error, "invalid ACP agentProfile object"))
            .ok();
    }
    value
        .as_str()
        .and_then(grow_agent::discovery::by_name)
}

fn parse_ask_user_question_from_meta(meta: Option<&acp::Meta>) -> Option<bool> {
    meta?.get("askUserQuestion")?.as_bool()
}

fn lookup_session_model(
    sessions: &HashMap<acp::SessionId, SessionHandle>,
    session_id: Option<&acp::SessionId>,
    default_model_id: &acp::ModelId,
) -> acp::ModelId {
    session_id
        .and_then(|id| sessions.get(id).map(|handle| handle.model_id.clone()))
        .unwrap_or_else(|| default_model_id.clone())
}

fn apply_yolo_mode_to_matching_sessions(
    sessions: &mut HashMap<acp::SessionId, SessionHandle>,
    sender_id: Option<&str>,
    yolo_mode: bool,
) -> usize {
    let mut updated = 0;
    for handle in sessions.values_mut() {
        let matches_sender = sender_id.is_none()
            || handle.origin_client.as_ref().map(|client| client.product.as_str()) == sender_id;
        if matches_sender {
            handle.yolo_mode = yolo_mode;
            let _ = handle
                .cmd_tx
                .send(SessionCommand::SetYoloMode { enabled: yolo_mode });
            updated += 1;
        }
    }
    updated
}
pub(crate) struct SessionSpawnOptions<'a> {
    pub session_info: SessionInfo,
    pub cwd: AbsPathBuf,
    pub mcp_servers: Vec<acp::McpServer>,
    pub initial_client_mcp_servers: Vec<acp::McpServer>,
    pub mcp_meta_config_map: McpMetaConfigMap,
    pub persistence: PersistenceHandle,
    pub chat_history: Vec<crate::sampling::ConversationItem>,
    pub rewind_points_file_path: Option<std::path::PathBuf>,
    pub initial_total_tokens: u64,
    pub origin_client: Option<crate::http::OriginClientInfo>,
    pub client_code_nav_enabled: bool,
    pub client_terminal: bool,
    pub client_fs_read: bool,
    pub client_fs_write: bool,
    pub preloaded_envrc: Option<std::collections::HashMap<String, String>>,
    pub persisted_signals: Option<crate::session::signals::SessionSignals>,
    pub persisted_behavior: Option<crate::session::behavior::BehaviorSnapshot>,
    pub persisted_goal_mode: Option<crate::session::goal_tracker::GoalOrchestration>,
    pub persisted_workflow_runs: Vec<
        crate::session::workflow::store::RestoredWorkflowRun,
    >,
    pub persisted_announcement_state: Option<
        crate::session::announcement_state::AnnouncementState,
    >,
    pub session_meta: Option<&'a acp::Meta>,
    pub managed_mcp_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Persisted Agent identity when reopening a session. New sessions leave
    /// this unset and resolve the global default independently of the model.
    pub persisted_agent_name: Option<&'a str>,
    pub session_model_id: acp::ModelId,
    pub session_yolo_mode: bool,
    pub session_auto_mode: bool,
    pub prompt_display_cwd: Option<String>,
}
/// `session/new` / `session/load` `_meta` key carrying per-session plugin roots.
pub(crate) const SESSION_PLUGIN_DIRS_META_KEY: &str = "pluginDirs";
/// `initialize` response `_meta` key advertising [`SESSION_PLUGIN_DIRS_META_KEY`] support.
pub(crate) const SESSION_PLUGIN_DIRS_CAPABILITY_KEY: &str = "grow/pluginDirs";
/// Per-session plugin roots from `session/new` / `session/load` `_meta.pluginDirs`,
/// loaded at CliOverride scope (always trusted) into this session's registry only.
/// Paths must be absolute (the SDKs resolve before sending); anything else is
/// warned and skipped.
pub(crate) fn parse_session_plugin_dirs(
    meta: Option<&acp::Meta>,
) -> Vec<std::path::PathBuf> {
    let Some(entries) = meta
        .and_then(|m| m.get(SESSION_PLUGIN_DIRS_META_KEY))
        .and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    let mut dirs = Vec::new();
    for entry in entries {
        let Some(raw) = entry.as_str() else {
            tracing::warn!(?entry, "pluginDirs entry is not a string; skipping");
            continue;
        };
        let path = std::path::PathBuf::from(raw);
        if !path.is_absolute() {
            tracing::warn!("pluginDirs entry is not absolute; skipping");
            continue;
        }
        let canonical = dunce::canonicalize(&path).unwrap_or(path);
        if !canonical.is_dir() {
            tracing::warn!("pluginDirs entry is not a directory; skipping");
            continue;
        }
        if !dirs.contains(&canonical) {
            dirs.push(canonical);
        }
    }
    dirs
}
/// `_meta.noReplay` → skip gateway replay (client already has the transcript).
fn parse_no_replay(meta: Option<&acp::Meta>) -> bool {
    meta.and_then(|m| m.get("noReplay")).and_then(|v| v.as_bool()).unwrap_or(false)
}
/// Insert `key`/`value` into a notification's `_meta`, creating the map if absent.
/// Used to stamp `grow/leaderClientId` onto replay notifications so the leader can
/// unicast them to the loading client only (see `forward_raw_replay_line`).
fn stamp_meta_value(meta: &mut Option<acp::Meta>, key: &str, value: &serde_json::Value) {
    meta.get_or_insert_with(acp::Meta::new).insert(key.to_string(), value.clone());
}
fn mark_as_replay(
    meta: &mut Option<acp::Meta>,
    persist_data: Option<&serde_json::Value>,
) {
    let is_replay = serde_json::json!(true);
    let obj = meta.get_or_insert_with(acp::Meta::new);
    obj.insert("isReplay".to_string(), is_replay);
    if let Some(persist) = persist_data {
        obj.insert("grow/persist".to_string(), persist.clone());
    }
}
/// Resolve a session's REQUESTED auto flag from `_meta`: an explicit `autoMode`
/// (or snake_case `auto_mode`) wins; when absent, fall back to the config default
/// with yolo taking precedence (yolo suppresses the default auto seed). Shared by
/// the new_session / load_session parse paths (the feature gate is enforced later
/// at the `set_auto_mode` seam) and unit-tested directly.
pub(crate) fn resolve_session_auto_mode(
    meta: Option<&acp::Meta>,
    default_auto_mode: bool,
    session_yolo_mode: bool,
) -> bool {
    meta.and_then(|m| m.get("autoMode").or_else(|| m.get("auto_mode")))
        .and_then(|v| v.as_bool())
        .unwrap_or(default_auto_mode && !session_yolo_mode)
}
/// Typed `_meta` payload for `PromptResponse`.
/// camelCase keys match the bot's `_META_TOKEN_KEY_MAP`.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PromptResponseMeta {
    pub session_id: String,
    pub request_id: String,
    pub prompt_id: String,
    pub total_tokens: u64,
    pub model_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_read_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
    /// Whole-prompt token usage (sibling token fields are last call only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<crate::extensions::notification::PromptUsage>,
    /// Cancellation category when the turn was terminated by the system
    /// (e.g. doom loop). `None` for normal completions and user cancels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancellation_category: Option<String>,
    /// What triggered a cancelled turn's cancel (`"send_now"`, `"ctrl_c"`,
    /// `"esc"`); surfaced as `cancelTrigger`. `None` for non-cancel completions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_trigger: Option<String>,
    /// Schema-validated `--json-schema` output. Delivered in `_meta` (not a
    /// side-channel notification) so the client reads it deterministically when
    /// the prompt RPC resolves. Absent unless requested and produced; on
    /// failure `structured_output_error` carries the message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output_error: Option<String>,
}
/// Inputs for [`build_prompt_response_meta`]. A struct (not positional args)
/// so call sites are self-documenting and adding a field can't silently
/// reorder an existing one.
pub(crate) struct PromptResponseMetaArgs<'a> {
    pub session_id: &'a str,
    pub prompt_id: &'a str,
    pub total_tokens: u64,
    pub model_id: &'a str,
    pub last_turn_usage: Option<&'a grow_sampling_types::TokenUsage>,
    pub prompt_usage: Option<crate::extensions::notification::PromptUsage>,
    pub cancellation_category: Option<String>,
    pub cancel_trigger: Option<String>,
    pub structured_output: Option<Result<serde_json::Value, String>>,
}
/// Build the `_meta` JSON for `PromptResponse`. Includes baseline
/// session/prompt/model identifiers plus optional per-turn token counts
/// from the most recent `TokenUsage`.
pub(crate) fn build_prompt_response_meta(
    args: PromptResponseMetaArgs<'_>,
) -> serde_json::Value {
    let PromptResponseMetaArgs {
        session_id,
        prompt_id,
        total_tokens,
        model_id,
        last_turn_usage,
        prompt_usage,
        cancellation_category,
        cancel_trigger,
        structured_output,
    } = args;
    let (structured_output, structured_output_error) = match structured_output {
        Some(Ok(value)) => (Some(value), None),
        Some(Err(error)) => (None, Some(error)),
        None => (None, None),
    };
    let meta = PromptResponseMeta {
        session_id: session_id.to_string(),
        request_id: prompt_id.to_string(),
        prompt_id: prompt_id.to_string(),
        total_tokens,
        model_id: model_id.to_string(),
        input_tokens: last_turn_usage.map(|u| u.prompt_tokens),
        output_tokens: last_turn_usage.map(|u| u.completion_tokens),
        cached_read_tokens: last_turn_usage.map(|u| u.cached_prompt_tokens),
        reasoning_tokens: last_turn_usage.map(|u| u.reasoning_tokens),
        usage: prompt_usage,
        cancellation_category,
        cancel_trigger,
        structured_output,
        structured_output_error,
    };
    serde_json::to_value(meta).expect("PromptResponseMeta is always serializable")
}
/// Typed payload for the `grow/settings/update` notification sent to pager
/// clients after remote settings settings are refreshed on `/new`.
///
/// Keeping this as a `#[derive(Serialize)]` struct gives compile-time
/// contract safety between the shell and the pager deserializer.
#[derive(serde::Serialize)]
struct SettingsUpdateNotification {
    show_resolved_model: Option<bool>,
    session_picker_grouped: Option<bool>,
    tips: Option<Vec<String>>,
    slash_command_tags: Option<std::collections::BTreeMap<String, String>>,
    /// Remote campaigns snapshot for the client's process-global campaign
    /// cache. `Some` whenever settings exist (empty means campaigns were
    /// withdrawn); `None` when the agent has no settings yet, which clients
    /// treat as "leave the cache alone". In leader mode this push is the only
    /// seam that seeds the TUI process, so a `/model` pick can record a remote
    /// campaign's dismissal even when the TUI's own startup prefetch missed.
    campaigns: Option<Vec<crate::util::config::CampaignOverride>>,
    auto_permission_mode_enabled: Option<bool>,
    /// Soft-default permission mode for the pager (post-auth / `/new` refresh).
    permission_mode: Option<String>,
    group_tool_verbs: Option<bool>,
    collapsed_edit_blocks: Option<bool>,
}
/// Reason why a client is not eligible to use codebase indexing.
///
/// Returned by [`MvpAgent::code_nav_eligibility`] when one of the policy
/// gates fails.  Used in `grow/code/status` responses and to generate
/// clear error messages on code-nav requests from ineligible clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeNavEligibility {
    /// Client type is not web (web-only for initial rollout).
    ClientNotWeb,
    /// Client did not advertise `grow/codeNavigation.enabled`.
    CapabilityNotAdvertised,
    /// `codebase_indexing` feature is disabled in config (or excluded by glob).
    DisabledByConfig,
    /// The cwd is not inside a git repository.
    NotGitRepo,
    /// `sessionId` is required for code navigation but was absent or refers to
    /// an unknown / evicted session.  Per-client capability cannot be determined
    /// without a valid session context.
    SessionRequired,
}
/// Interval between join-handle supervisor sweeps. A panicked/exited actor is
/// reaped within one tick. Kept small so reaping is prompt
/// without busy-spinning the single `LocalSet` thread.
const SESSION_SUPERVISOR_TICK: std::time::Duration = std::time::Duration::from_millis(
    200,
);
/// Upper bound on the `SessionHandle::is_busy` round-trip used by the
/// idle-unload decision (PR-2). Only consulted when no turn is running (so the
/// actor is between turns and responsive); on timeout we conservatively treat
/// the session as busy and keep it resident.
const IDLE_QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);
/// Per-session state freed on removal or idle-unload (but kept across a reload
/// rebuild); retained state instead survives an unload and is freed only at
/// removal.
#[derive(Default)]
struct ResidentResources {
    /// Strong ref pinning the code-nav index; the manager holds only a `Weak`.
    codebase_index: Option<std::sync::Arc<xai_codebase_graph::IndexManagerHandle>>,
    require_gateway: bool,
}
/// Per-session state that survives an idle-unload (so the session stays
/// resumable); freed only at `remove_session`. See [`ResidentResources`].
#[derive(Default)]
struct RetainedResources {
    turn_number: Option<u64>,
    dispatch_lock: Option<std::rc::Rc<tokio::sync::Mutex<()>>>,
    permission_event_receiver: Option<
        tokio::sync::mpsc::UnboundedReceiver<PermissionEvent>,
    >,
}
pub struct MvpAgent {
    /// LEADER-SAFE(per-session). Removed by `remove_session` / `sweep_dead_sessions`.
    pub(crate) sessions: RefCell<HashMap<acp::SessionId, SessionHandle>>,
    /// LEADER-SAFE(shared): `Send + Sync` mirror of per-session activity for the
    /// leader's auto-update checker, which cannot read the `!Send` maps. Expires
    /// when the actor exits. See [`crate::agent::activity::AgentActivity`].
    pub(crate) activity: crate::agent::activity::AgentActivity,
    /// LEADER-SAFE(per-session): in-flight `session/load` guards. Lets a racing
    /// `session/prompt` wait via [`Self::wait_for_in_flight_session_load`] instead
    /// of failing "unknown session id"; the RAII guard's drop wakes waiters.
    loading_sessions: RefCell<
        HashMap<acp::SessionId, tokio::sync::watch::Receiver<bool>>,
    >,
    /// LEADER-SAFE(per-session): reclaimed at `remove_session`. See [`RetainedResources`].
    retained_resources: RefCell<HashMap<acp::SessionId, RetainedResources>>,
    /// LEADER-SAFE(per-session): keyed by SessionId. Mirrors `sessions` lifecycle.
    session_threads: RefCell<HashMap<acp::SessionId, SessionThread>>,
    /// Title per resident session id, refreshed each `build_roster`. Lets the
    /// synchronous roster deltas reuse the title instead of emitting an empty
    /// one — `resident_roster_entry` can't read disk.
    resident_roster_titles: RefCell<HashMap<String, String>>,
    pub(crate) initialize_request: OnceLock<acp::InitializeRequest>,
    pub(crate) gateway: GatewaySender,
    /// Agent configuration. LEADER-SAFE(init-once): never mutated after construction.
    pub(crate) cfg: RefCell<AgentConfig>,
    /// Current auth method. LEADER-SAFE(shared): all clients share the same auth;
    /// last authenticate() call wins, which is correct (same user, same creds).
    /// Held as a shared live handle cloned into every running session so a
    /// mid-session `authenticate` (`/login`) is observed by each session's
    /// per-turn auth gate without re-spawning.
    pub(crate) auth_method_id: crate::agent::auth_method::SharedAuthMethodId,
    /// Global sampling config (API key + default base_url). LEADER-SAFE(shared):
    /// only api_key is written here (same for all clients). Per-session base_url
    /// is resolved at session creation time in `new_session` / `load_session`.
    pub(crate) sampling_config: RefCell<SamplingConfig>,
    pub(crate) auth_manager: Arc<AuthManager>,
    pub(crate) models_manager: crate::agent::models::ModelsManager,
    /// Single-flight guard for interactive login (device poll / loopback
    /// wait). Owns the active attempt's cancel token and its code/url
    /// channels; a new `authenticate` or `grow/auth/cancel` cancels the
    /// prior attempt.
    pub(crate) interactive_auth: crate::auth::single_flight::AuthSingleFlight,
    /// Client type. LEADER-SAFE(init-once): set once during `initialize` from
    /// `_meta.clientIdentifier` (injected by the IPC server in leader mode).
    ///
    /// **Known limitation (leader mode)**: in a session with multiple concurrent
    /// clients, the last `initialize` call wins and overwrites the global value.
    /// This means per-client diagnostics attribution (AB experiments, analytics,
    /// worktree-pool eligibility) uses the identity of whichever client most
    /// recently initialized — not the client that owns the current session.
    ///
    /// This is considered acceptable because `client_type` is used only for
    /// non-safety-critical diagnostics and experiment filtering.  Fully per-session
    /// attribution would require threading `clientIdentifier` from `_meta` through
    /// every session handler, which is deferred to future work.
    client_type: RefCell<ClientType>,
    /// Whether the current client advertised `grow/codeNavigation.enabled`.
    /// Updated on every `initialize()` call — same last-client-wins semantics
    /// as `client_type`.  Using `Cell<bool>` (not `RefCell`) so `.get()` is a
    /// plain copy with no borrow that could be held across an await point.
    code_nav_enabled: std::cell::Cell<bool>,
    /// Default YOLO mode - when true, sessions start with auto-approve enabled.
    /// Per-session YOLO tracking lives in SessionHandle.yolo_mode.
    default_yolo_mode: bool,
    default_auto_mode: bool,
    /// Memory system configuration (None when --experimental-memory not set).
    memory_config: Option<crate::config::MemoryConfig>,
    /// Optional channel to the leader's `ConfigFileWatcher` for dynamic
    /// per-cwd registration as new sessions open. Each
    /// successful session insert in `spawn_and_register_session` sends
    /// the session's cwd to the watcher task spawned in
    /// `agent/app.rs`, which calls
    /// [`crate::config::watcher::ConfigFileWatcher::watch_path`] (a
    /// **non-recursive** watch on `<cwd>/` and `<cwd>/.grow/`).
    ///
    /// `None` outside leader mode and in tests — the registration is a
    /// no-op in that case, which is fine: the existing per-extra-path
    /// loop already covers the leader's startup cwd.
    /// Plain `Option` (not `RefCell`) — this is written
    /// exactly once, by `set_config_watcher_path_tx(&mut self)` during
    /// leader construction while the agent is still uniquely owned, and
    /// only read thereafter. No interior mutability is required.
    pub(crate) config_watcher_path_tx: Option<
        tokio::sync::mpsc::UnboundedSender<std::path::PathBuf>,
    >,
    /// Buffering configuration. LEADER-SAFE(init-once): set once per connection
    /// during initialize from client capabilities, read when spawning sessions.
    /// In leader mode, the last client to initialize overwrites previous settings
    /// (same caveat as client_type — acceptable for non-safety-critical config).
    buffering_settings: RefCell<Option<update_chunk_merge::BufferingSettings>>,
    /// Context for managing background copy operations (e.g., copying ignored files)
    pub(crate) background_copy_context: BackgroundCopyContext,
    /// LEADER-SAFE(shared): agent-level code-nav index manager, keyed by cwd,
    /// no per-client state.
    codebase_indexes: Arc<parking_lot::Mutex<CodebaseIndexManager>>,
    /// LEADER-SAFE(per-session): reclaimed on removal / idle-unload. See [`ResidentResources`].
    resident_resources: RefCell<HashMap<acp::SessionId, ResidentResources>>,
    /// Worktree creation type (resolved: local config > remote > default Linked).
    pub(crate) worktree_type: crate::util::config::WorktreeType,
    /// Restore codebase state on worktree resume (resolved: local config > remote > default false).
    pub(crate) restore_code: bool,
    /// Managed MCP configs and gateway tool catalog; lazily fetched.
    managed_mcp_cache: crate::session::managed_mcp::ManagedMcpStateHandle,
    /// Agent-level MCP server state. LEADER-SAFE(shared): MCP servers are
    /// agent-scoped, not per-client.
    agent_mcp_state: std::sync::Arc<
        tokio::sync::Mutex<crate::session::mcp_servers::McpState>,
    >,
    /// Sessions whose persisted model was unavailable at `session/load` time
    /// with no same-family fallback, keyed by session id → the unavailable
    /// model id. Prompts to these sessions are blocked until either
    /// (a) the model reappears in the catalog — the catalog can be
    /// transiently degraded when a reconnect replays `session/load` (e.g.
    /// fetch still in flight after a leader restart), so the prompt path
    /// re-checks and self-heals — or (b) the user explicitly switches
    /// models via `set_session_model`. Released by `remove_session`.
    model_unavailable_sessions: RefCell<std::collections::HashMap<String, acp::ModelId>>,
    /// Unified sender for all subagent coordinator events.
    /// LEADER-SAFE(shared): channel is multi-producer, coordinator drains.
    subagent_event_tx: tokio::sync::mpsc::UnboundedSender<
        grow_tools::implementations::grow_build::task::types::SubagentEvent,
    >,
    /// Receiver for subagent events. Taken once by `start_subagent_coordinator()`.
    /// `None` after the coordinator drain task has been spawned.
    subagent_event_rx: RefCell<
        Option<
            tokio::sync::mpsc::UnboundedReceiver<
                grow_tools::implementations::grow_build::task::types::SubagentEvent,
            >,
        >,
    >,
    /// Shell-only presentation state; lifecycle lives in the channel actor.
    subagent_presentation: RefCell<crate::agent::subagent::SubagentPresentation>,
    /// Shared buffer for mid-turn monitor event notifications.
    /// Pushed by the `InjectNotification` handler when a turn is active and the
    /// notification has `Next` priority. Drained by the session turn loop
    /// (`inject_pending_monitor_events`) into a hidden synthetic user message.
    monitor_event_buffer: grow_tools::implementations::grow_build::monitor::types::MonitorEventBuffer,
    /// The process launch directory, captured once at construction so the
    /// deferred launch-dir init paths share one source of truth instead of each
    /// re-calling `std::env::current_dir()` (which could drift if the process
    /// cwd ever changes after startup).
    launch_cwd: PathBuf,
    /// Memoizes the single [`folder_trust::resolve_launch_dir_trust`] gather for
    /// the launch dir; see it for the dedup + TOCTOU contract.
    launch_dir_trust: std::cell::OnceCell<bool>,
    /// Shared plugin registry handle.
    pub(crate) plugin_registry_handle: grow_agent::plugins::SharedPluginRegistryHandle,
    /// One-shot guard for the lazy launch-dir population of
    /// `plugin_registry_handle`.
    ///
    /// Boot-time plugin discovery is deferred past ACP `initialize` (it walks
    /// cwd→git root plus user/marketplace dirs and stalled embedding clients' first
    /// `initialize`), so the shared snapshot starts empty. It is built once on
    /// the first session-creating call via [`Self::ensure_plugin_registry`];
    /// this flag keeps that to a single discovery walk.
    plugin_registry_initialized: std::cell::Cell<bool>,
    /// Single-flight guard for the proactive bundle sync background task.
    ///
    /// `maybe_sync_bundle_in_background` is invoked from each post-auth path
    /// (initialize, cached-token reauth, oidc) and a rapid reconnect can fire
    /// all three within the TTL window, giving us multiple concurrent
    /// `tokio::task::spawn_local` tasks racing to extract the tar archive,
    /// rewrite `manifest.json`, and prune stale files. The non-atomic
    /// per-file write/prune semantics in `bundle::extract_bundle_archive`
    /// make that race observable as a partially-written cache.
    ///
    /// We use an `Arc<AtomicBool>` so the spawned task can clear the flag
    /// on completion without re-borrowing `&self`. `Send` is required
    /// because the inner `sync_bundle_to_root` now uses `spawn_blocking`.
    bundle_sync_in_flight: Arc<std::sync::atomic::AtomicBool>,
    /// Local workspace ops, built lazily via [`Self::ensure_local_workspace_ops`].
    /// The agent never opens Computer Hub as a harness/client; remote cloud
    /// sandboxes are gateway-owned (`gateway_bridge` / `computer_sessions`).
    workspace_ops: RefCell<Option<grow_workspace::WorkspaceOps>>,
    /// Per-session coarse lifecycle state (residency + turn-state).
    /// Updated by `spawn_and_register_session` (→ `IdleResident`) and the
    /// join-handle supervisor on actor exit (→ `DeadFailed`) / explicit close
    /// (→ `Completed`). This is the roster's data source in PR-6; for now it
    /// gives the supervisor an observable demotion signal.
    /// LEADER-SAFE(per-session): keyed by SessionId.
    session_live_state: RefCell<HashMap<acp::SessionId, SessionLiveState>>,
    /// Idempotency guard: the join-handle supervisor task is spawned at most
    /// once (on the first `spawn_and_register_session`). See
    /// `ensure_session_supervisor`.
    supervisor_started: std::cell::Cell<bool>,
    /// Test-only spy recording every terminal roster delta `(session_id,
    /// final_state)` emitted by `record_roster_delta` (reap → `DeadFailed`,
    /// explicit close → `Completed`). Lets tests observe a terminal demotion
    /// even though the `session_live_state` entry is dropped on removal
    /// (the map is kept bounded).
    #[cfg(test)]
    roster_delta_spy: RefCell<Vec<(String, SessionLiveState)>>,
    /// Test-only counter of how many times the join-handle supervisor task was
    /// actually spawned. Asserts `ensure_session_supervisor` is idempotent.
    #[cfg(test)]
    supervisor_spawn_count: std::cell::Cell<usize>,
}
/// Spawn a thread to warm the shared async HTTP client (`OnceLock`-cached).
/// Loading TLS root certs is ~95ms; doing it here avoids a cold-start hit
/// on the first request. Idempotent.
pub fn warm_async_http_client() {
    std::thread::spawn(|| {
        let _timer = crate::instrumentation_timer!("startup.async_http_warmup");
        let _ = crate::http::shared_client();
    });
}
/// Read a string field from `session_meta` first, falling back to
/// `init_meta`. The session path bypasses the `initialize_request`
/// `OnceLock`, so a fresh client can supply `rules` / `systemPromptOverride`
/// even when the leader has been warmed by an earlier client.
fn read_session_or_init_meta_str<'a>(
    session_meta: Option<&'a acp::Meta>,
    init_meta: Option<&'a acp::Meta>,
    key: &str,
) -> Option<&'a str> {
    let read = |m: Option<&'a acp::Meta>| -> Option<&'a str> {
        m.and_then(|m| m.get(key)).and_then(|v| v.as_str())
    };
    read(session_meta).or_else(|| read(init_meta))
}
use grow_chat_state::conversation_util::replace_or_insert_system_head;
/// Non-empty `systemPromptOverride` from session meta (preferred) or init meta.
/// A blank string (empty or whitespace-only) is treated as "no override" so a
/// client cannot accidentally blank the system prompt.
fn system_prompt_override_from_meta<'a>(
    session_meta: Option<&'a acp::Meta>,
    init_meta: Option<&'a acp::Meta>,
) -> Option<&'a str> {
    read_session_or_init_meta_str(session_meta, init_meta, "systemPromptOverride")
        .filter(|s| !s.trim().is_empty())
}
/// Compose the system prompt for a *fresh* session: a full `systemPromptOverride`
/// verbatim, else the agent template with `_meta.rules` folded into
/// `<human_rules>`. Note: `rules` is applied at creation only — resumed sessions
/// sync `systemPromptOverride` (see `enqueue_replace_system_prompt_override`) but
/// not `rules`, by design.
fn build_spawn_system_prompt(
    session_meta: Option<&acp::Meta>,
    init_meta: Option<&acp::Meta>,
    agent_system_prompt: &str,
) -> String {
    if let Some(override_prompt) = system_prompt_override_from_meta(
        session_meta,
        init_meta,
    ) {
        override_prompt.to_owned()
    } else {
        let mut prompt = agent_system_prompt.to_owned();
        if let Some(rules) = read_session_or_init_meta_str(
            session_meta,
            init_meta,
            "rules",
        ) {
            prompt.push_str("\n\n<human_rules>\n");
            prompt.push_str(rules);
            prompt.push_str("\n</human_rules>");
        }
        prompt
    }
}
/// Enqueue a `ReplaceSystemPrompt` for a resident session actor. No-op when
/// the client sent no (non-empty) `systemPromptOverride`, or when the head
/// already matches (e.g. a cold load that pre-applied the override).
///
/// Note: only `systemPromptOverride` is synced on attach. `_meta.rules` is
/// folded into the prompt at session creation only (see
/// `build_spawn_system_prompt`); resumed sessions keep their original prompt
/// unless a full override is supplied. Updating `rules` mid-session is out of
/// scope by design.
fn enqueue_replace_system_prompt_override(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>,
    session_meta: Option<&acp::Meta>,
    init_meta: Option<&acp::Meta>,
) {
    let Some(override_prompt) = system_prompt_override_from_meta(session_meta, init_meta)
    else {
        return;
    };
    let _ = cmd_tx
        .send(crate::session::SessionCommand::ReplaceSystemPrompt {
            system_prompt: override_prompt.to_owned(),
        });
}
/// Warn that a `ValidateType` arrived for an evicted/unknown parent session,
/// so ops can diagnose "Unknown subagent type" errors for project agents.
pub(crate) fn warn_on_missing_parent_session_for_validate_type(
    parent_session_id: &str,
    parent_session_present: bool,
) {
    if !parent_session_present {
        tracing::warn!(
            parent_session_id,
            "ValidateType received for unknown parent session — \
             validating against built-ins only",
        );
    }
}
/// Parse an env var as a JSON object. Returns `None` if unset or not a valid JSON object.
pub(crate) fn parse_json_object_env(var: &str) -> Option<serde_json::Value> {
    let val = std::env::var(var).ok()?;
    match serde_json::from_str::<serde_json::Value>(&val) {
        Ok(v) if v.is_object() => Some(v),
        Ok(_) => {
            tracing::warn!("{var} is not a JSON object, ignoring");
            None
        }
        Err(e) => {
            tracing::warn!("{var} is invalid JSON: {e}");
            None
        }
    }
}
#[derive(Debug, Default, serde::Deserialize)]
struct AuthRequestMeta {
    #[serde(default)]
    headless: bool,
    #[serde(default)]
    reauth: bool,
    /// `--oauth`: force loopback. The only transport override sent over ACP
    /// (loopback is the default; device is opt-in via env/config).
    #[serde(default)]
    use_oauth: bool,
    /// When true, skip cached tokens and force the interactive browser login
    /// flow. Used by the `/login` slash command for mid-session re-auth.
    /// Unlike `reauth`, this does NOT clear existing credentials — if the
    /// user abandons the browser flow, the current session continues.
    #[serde(default)]
    force_interactive: bool,
    /// Pager auth `request_seq` for this attempt. Scopes `grow/auth/cancel`
    /// so a delayed cancel cannot tear down a successor login.
    #[serde(default)]
    request_seq: Option<u64>,
}
impl AuthRequestMeta {
    /// `--oauth` → force loopback; otherwise default (loopback).
    fn login_override(&self) -> crate::auth::LoginTransportOverride {
        if self.use_oauth {
            crate::auth::LoginTransportOverride::ForceLoopback
        } else {
            crate::auth::LoginTransportOverride::None
        }
    }
    fn from_json(meta: Option<&acp::Meta>) -> Self {
        meta.cloned()
            .and_then(|value| {
                serde_json::from_value(serde_json::Value::Object(value)).ok()
            })
            .unwrap_or_default()
    }
}
/// Inject standard proxy headers into an `extra_headers` map.
///
/// Every authenticated request to cli-chat-proxy (web search, image gen, and
/// any future tools that go through the proxy) must carry these headers.
/// Centralising them here means new tool code paths only need one call instead
/// of remembering which headers the proxy expects.
///
/// Headers injected:
///  - `x-grow-client-version` -- required by the proxy's version-gate check.
///    Uses `client_version` when provided, otherwise falls back to cli-chat-proxy
///    compile-time `CARGO_PKG_VERSION`.
///  - `X-Grow-Token-Auth` / `x-authenticateresponse` -- required by the
///    cli-chat-proxy auth middleware when the `base_url` is a known proxy URL.
///  - optional extra access header -- only set when the corresponding key is
///    `Some` *and* the `base_url` points at a matching non-production host
///    (requires the optional non-production feature).
///
/// Existing entries are never overwritten so callers can pre-set a value.
fn inject_proxy_headers(
    headers: &mut indexmap::IndexMap<String, String>,
    client_version: Option<&str>,
    alpha_test_key: Option<&str>,
    base_url: &str,
) {
    headers
        .entry("x-grow-client-version".to_string())
        .or_insert_with(|| {
            client_version
                .map(String::from)
                .unwrap_or_else(|| grow_version::VERSION.to_string())
        });
    headers
        .entry("x-grow-client-identifier".to_string())
        .or_insert_with(crate::http::process_client_identifier);
    if crate::util::is_cli_chat_proxy_url(base_url) {
        headers
            .entry("X-Grow-Token-Auth".to_string())
            .or_insert_with(|| "grow-cli".to_string());
        headers
            .entry("x-authenticateresponse".to_string())
            .or_insert_with(|| "authenticate-response".to_string());
        headers
            .entry(crate::http::CLIENT_MODE_HEADER.to_string())
            .or_insert_with(|| crate::http::process_client_mode().to_string());
    }
    let _ = (alpha_test_key, base_url);
}
fn resolve_inference_idle_timeout_secs(
    models: &indexmap::IndexMap<String, crate::agent::config::ModelEntry>,
    model: &str,
    remote_settings: Option<&crate::util::config::RemoteSettings>,
) -> u64 {
    let per_model = models
        .get(model)
        .or_else(|| models.values().find(|entry| entry.info.model == model))
        .and_then(|entry| entry.info.inference_idle_timeout_secs);
    let remote = remote_settings.and_then(|s| s.inference_idle_timeout_secs);
    per_model.or(remote).unwrap_or(600).max(10)
}
/// Parse the client-advertised `grow/hunkTracker.mode` string. Case-insensitive
/// and trimmed. Absent/blank/`off`/`disabled` => `None`; unknown => `AllDirty`.
fn resolve_hunk_tracking_mode(
    mode_str: Option<&str>,
) -> Option<xai_hunk_tracker::TrackingMode> {
    let mode = mode_str.map(str::trim)?;
    if mode.is_empty() || mode.eq_ignore_ascii_case("off")
        || mode.eq_ignore_ascii_case("disabled")
    {
        return None;
    }
    Some(
        serde_json::from_value(serde_json::Value::String(mode.to_ascii_lowercase()))
            .unwrap_or(xai_hunk_tracker::TrackingMode::AllDirty),
    )
}
/// Session wiring derived from the resolved tracking mode. Disabling the tracker
/// (`actor_mode == None`) turns off the actor, the per-event forward, and the
/// LOC sink together, so the disable path can't be left half-wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HunkTrackingPlan {
    /// `Some` → spawn the actor in this mode; `None` → use `noop()`, no actor.
    actor_mode: Option<xai_hunk_tracker::TrackingMode>,
}
impl HunkTrackingPlan {
    /// Gate for the fs-notify forward sites (via `ToolContext.hunk_tracking_enabled`)
    /// and LOC-sink eligibility.
    fn enabled(&self) -> bool {
        self.actor_mode.is_some()
    }
}
fn plan_hunk_tracking(mode_str: Option<&str>) -> HunkTrackingPlan {
    HunkTrackingPlan {
        actor_mode: resolve_hunk_tracking_mode(mode_str),
    }
}
/// RAII marker for an in-flight `session/load` (see
/// [`MvpAgent::begin_session_load`]). Holding the guard keeps the session id
/// in `MvpAgent::loading_sessions`; dropping it removes the marker and wakes
/// every [`MvpAgent::wait_for_in_flight_session_load`] waiter (the held
/// watch sender drops with the guard, closing the channel).
pub(crate) struct SessionLoadGuard<'a> {
    agent: &'a MvpAgent,
    session_id: acp::SessionId,
    rx: tokio::sync::watch::Receiver<bool>,
    /// Dropped with the guard — closes the watch channel, waking waiters.
    _tx: tokio::sync::watch::Sender<bool>,
}
impl Drop for SessionLoadGuard<'_> {
    fn drop(&mut self) {
        let mut map = self.agent.loading_sessions.borrow_mut();
        if map.get(&self.session_id).is_some_and(|rx| rx.same_channel(&self.rx)) {
            map.remove(&self.session_id);
        }
    }
}
mod code_nav;
mod session_lifecycle;
mod subagent_coordinator;
mod agent_ops;
mod acp_agent;
pub(crate) use session_lifecycle::RegistrySnapshot;
pub(super) use super::ext_parsers;
/// Emit the `auth.lifecycle` login span with optional user id and error
/// category. Named `auth.lifecycle` (not `auth`) to avoid colliding with the
/// pre-existing per-request `AuthManager::auth()` `#[instrument]` span.
fn emit_login_span(
    success: bool,
    auth_method: &str,
    user_id: Option<&str>,
    error_category: Option<&str>,
) {
    let span = tracing::info_span!(
        "auth.lifecycle",
        action = "login",
        success,
        auth_method,
        user_id = tracing::field::Empty,
        error_category = tracing::field::Empty,
    );
    if let Some(uid) = user_id
        .filter(|u| !u.is_empty() && !u.eq_ignore_ascii_case("unknown"))
    {
        span.record("user_id", uid);
    }
    if let Some(ec) = error_category {
        span.record("error_category", ec);
    }
    span.in_scope(|| {});
}
/// Metadata captured from a replayed `task_backgrounded` entry.
pub(crate) struct OrphanedTask {
    task_id: String,
    command: String,
    cwd: String,
}
impl MvpAgent {
    /// Forward one raw JSONL replay line and collect its completion receiver.
    ///
    /// Dispatches by on-disk method name:
    /// - ACP updates (`"session/update"`) → typed `SessionNotification` for correct
    ///   TUI dispatch (direct dispatch preserves Rust types, not method strings).
    /// - Grow updates (`"_grow/session/update"`) → `ExtNotification`.
    ///
    /// When `mark_replay` is true, the notification is tagged with
    /// `_meta.isReplay: true` so the client knows it's historical data.
    /// Cursor-based reconnects set this to false for events after the cursor
    /// so the client processes them as live updates.
    fn forward_raw_replay_line(
        &self,
        line: &str,
        persist_data: Option<&serde_json::Value>,
        target_client_id: Option<&serde_json::Value>,
        completions: &mut Vec<
            tokio::sync::oneshot::Receiver<xai_acp_lib::AcpResult<()>>,
        >,
        mark_replay: bool,
        pending_tool_calls: &mut std::collections::HashMap<
            acp::ToolCallId,
            acp::ToolCall,
        >,
    ) {
        use crate::session::storage::RawLinePeek;
        let env = match serde_json::from_str::<RawLinePeek<'_>>(line) {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(?e, "replay: skipping unparseable JSONL line");
                return;
            }
        };
        let method = env.method.unwrap_or("session/update");
        let Some(raw_params) = env.params else {
            tracing::debug!("replay: skipping JSONL line with no params");
            return;
        };
        let is_xai = method == "_grow/session/update";
        if is_xai {
            if target_client_id.is_none() && !mark_replay {
                if let Ok(owned) = serde_json::value::RawValue::from_string(
                    raw_params.get().to_owned(),
                ) {
                    completions
                        .push(
                            self
                                .gateway
                                .forward_with_completion(
                                    acp::ExtNotification::new(
                                        "grow/session/update",
                                        std::sync::Arc::from(owned),
                                    ),
                                ),
                        );
                }
            } else {
                let Ok(mut params) = serde_json::from_str::<
                    serde_json::Value,
                >(raw_params.get()) else {
                    tracing::debug!("replay: skipping Grow update with unparseable params");
                    return;
                };
                if let Some(obj) = params.as_object_mut() {
                    let meta = obj
                        .entry("_meta")
                        .or_insert_with(|| serde_json::json!({}));
                    if let Some(m) = meta.as_object_mut() {
                        if mark_replay {
                            m.insert("isReplay".to_string(), serde_json::json!(true));
                        }
                        if let Some(pd) = persist_data {
                            m.insert("grow/persist".to_string(), pd.clone());
                        }
                        if let Some(tid) = target_client_id {
                            m.insert("grow/leaderClientId".to_string(), tid.clone());
                        }
                    }
                }
                if let Ok(raw_val) = serde_json::value::to_raw_value(&params) {
                    completions
                        .push(
                            self
                                .gateway
                                .forward_with_completion(
                                    acp::ExtNotification::new(
                                        "grow/session/update",
                                        std::sync::Arc::from(raw_val),
                                    ),
                                ),
                        );
                }
            }
        } else {
            let Ok(mut notification) = serde_json::from_str::<
                acp::SessionNotification,
            >(raw_params.get()) else {
                tracing::debug!("replay: skipping ACP update with unparseable params");
                return;
            };
            match &mut notification.update {
                acp::SessionUpdate::ToolCall(tc) => {
                    let is_pre_completed = matches!(
                        tc.status,
                        acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
                    );
                    if is_pre_completed {} else {
                        pending_tool_calls.insert(tc.tool_call_id.clone(), tc.clone());
                        return;
                    }
                }
                acp::SessionUpdate::ToolCallUpdate(u) => {
                    match u.fields.status {
                        Some(acp::ToolCallStatus::Completed)
                        | Some(acp::ToolCallStatus::Failed) => {
                            if let Some(mut base) = pending_tool_calls
                                .remove(&u.tool_call_id)
                            {
                                base.update(std::mem::take(&mut u.fields));
                                notification.update = acp::SessionUpdate::ToolCall(base);
                            }
                        }
                        None => {
                            if let Some(base) = pending_tool_calls
                                .get_mut(&u.tool_call_id)
                            {
                                base.update(std::mem::take(&mut u.fields));
                            }
                            return;
                        }
                        _ => return,
                    }
                }
                _ => {}
            }
            if mark_replay {
                mark_as_replay(&mut notification.meta, persist_data);
            }
            if let Some(tid) = target_client_id {
                stamp_meta_value(&mut notification.meta, "grow/leaderClientId", tid);
            }
            completions.push(self.gateway.forward_with_completion(notification));
        }
    }
    /// Replay updates from disk and drain completions.
    /// Returns `(initial_total_tokens, end_offset)`.
    pub(super) async fn replay_session_updates(
        &self,
        session_id: &acp::SessionId,
        cwd: &AbsPathBuf,
        updates_file_path: &Option<PathBuf>,
        persist_data: Option<&serde_json::Value>,
        target_client_id: Option<&serde_json::Value>,
        cursor: Option<&str>,
    ) -> Result<(u64, u64, Vec<(String, String)>), acp::Error> {
        let mut replay_timer = crate::instrumentation_timer!("session.load_session_replay");
        replay_timer.with_field("session_id", session_id.0.as_ref());
        replay_timer.with_field("cwd", cwd.as_str());
        let Some(updates_path) = updates_file_path.clone() else {
            tracing::warn!(session_id = %session_id.0, "replay: no updates file path");
            return Ok((0, 0, Vec::new()));
        };
        let file_size = std::fs::metadata(&updates_path).map(|m| m.len()).unwrap_or(0);
        let raw_contents = match std::fs::read_to_string(&updates_path) {
            Ok(s) if !s.is_empty() => s,
            _ => return Ok((0, 0, Vec::new())),
        };
        let end_offset = raw_contents.len() as u64;
        let mut prepared = {
            let _timer = crate::instrumentation_timer!("session.replay.read_and_filter");
            crate::session::storage::prepare_replay_lines(&raw_contents, cursor)
        };
        let unfinished_subagents = std::mem::take(&mut prepared.unfinished_subagents);
        if cursor.is_some() {
            let sending = prepared.lines.len();
            if prepared.mark_replay {
                tracing::warn!(
                    session_id = %session_id.0,
                    "replay: cursor not found, falling back to full replay"
                );
            } else {
                tracing::info!(
                    session_id = %session_id.0,
                    skipped = prepared.total_live - sending,
                    remaining = sending,
                    "replay: cursor found, skipping events"
                );
            }
        }
        let last_tokens = prepared.last_tokens;
        let mark_replay = prepared.mark_replay;
        if let Some(max_seq) = prepared.max_event_seq {
            crate::util::event_id::ensure_event_counter_at_least(max_seq + 1);
        }
        let lines_to_send = prepared.lines;
        let updates_count = lines_to_send.len() as u64;
        let mut completions = Vec::with_capacity(lines_to_send.len());
        {
            let _timer = crate::instrumentation_timer!("session.replay.forward_updates");
            let mut pending_tool_calls = std::collections::HashMap::new();
            for line in &lines_to_send {
                self.forward_raw_replay_line(
                    line,
                    persist_data,
                    target_client_id,
                    &mut completions,
                    mark_replay,
                    &mut pending_tool_calls,
                );
            }
        }
        if updates_count > 0 && completions.is_empty() {
            tracing::warn!(
                updates_count,
                "Replay sent updates but collected 0 completions — \
                 forward_raw_replay_line must use gateway.forward_with_completion(). \
                 See: session/load notification ordering bug."
            );
        }
        {
            let _timer = crate::instrumentation_timer!("session.replay.drain_completions");
            for rx in completions {
                let _ = rx.await;
            }
        }
        tracing::info!(
            session_id = %session_id.0,
            updates_count,
            end_offset,
            file_size,
            "replay: completed"
        );
        replay_timer.with_field("updates_count", updates_count);
        Ok((last_tokens, end_offset, unfinished_subagents))
    }
    /// Enqueue replay notifications for updates appended after `from_offset`.
    /// Returns completion receivers; callers open the gate then drain.
    /// Intentionally sync (not async) so no prompt-task progress before gate flip.
    ///
    /// When `mark_replay` is false (cursor-based reconnect), delta events are
    /// forwarded without `_meta.isReplay` since they are truly new events the
    /// client has not seen.
    pub(super) fn replay_session_updates_from_offset_enqueue(
        &self,
        session_id: &acp::SessionId,
        updates_file_path: &Option<PathBuf>,
        from_offset: u64,
        persist_data: Option<&serde_json::Value>,
        target_client_id: Option<&serde_json::Value>,
        mark_replay: bool,
    ) -> Vec<tokio::sync::oneshot::Receiver<xai_acp_lib::AcpResult<()>>> {
        use std::io::{Read, Seek, SeekFrom};
        let Some(updates_path) = updates_file_path.clone() else {
            return Vec::new();
        };
        let mut file = match std::fs::File::open(&updates_path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        if file.seek(SeekFrom::Start(from_offset)).is_err() {
            return Vec::new();
        }
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_err() || contents.is_empty() {
            return Vec::new();
        }
        let live_lines = crate::session::storage::filter_delta_replay_lines(&contents);
        let delta_count = live_lines.len();
        let mut completions = Vec::with_capacity(live_lines.len());
        let mut pending_tool_calls = std::collections::HashMap::new();
        for line in &live_lines {
            self.forward_raw_replay_line(
                line,
                persist_data,
                target_client_id,
                &mut completions,
                mark_replay,
                &mut pending_tool_calls,
            );
        }
        if delta_count > 0 && completions.is_empty() {
            tracing::warn!(
                delta_count,
                "Delta replay sent updates but collected 0 completions — \
                 forward_raw_replay_line must use gateway.forward_with_completion(). \
                 See: session/load notification ordering bug."
            );
        }
        if delta_count > 0 {
            tracing::info!(
                session_id = %session_id.0,
                delta_count,
                from_offset,
                "Delta replay enqueued updates (drain pending)"
            );
        }
        completions
    }
    /// Scan persisted updates for `task_backgrounded` entries that have no
    /// matching `task_completed`. Applies rewind dead-branch filtering so
    /// tasks from rewound branches are not included.
    pub(super) fn find_orphaned_background_tasks(
        updates_file_path: &Option<PathBuf>,
    ) -> Vec<OrphanedTask> {
        use crate::session::wire_tags::{TASK_BACKGROUNDED, TASK_COMPLETED};
        let Some(updates_path) = updates_file_path else {
            return Vec::new();
        };
        let contents = match std::fs::read_to_string(updates_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let all_lines: Vec<&str> = contents
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        let live_lines = crate::session::storage::filter_rewind_lines(all_lines);
        let mut pending = std::collections::HashMap::<String, OrphanedTask>::new();
        for line in live_lines {
            if !line.contains(&*TASK_BACKGROUNDED) && !line.contains(&*TASK_COMPLETED) {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let update = &v["params"]["update"];
            match update["sessionUpdate"].as_str() {
                Some(tag) if tag == *TASK_BACKGROUNDED => {
                    if let Some(id) = update["task_id"].as_str() {
                        pending
                            .insert(
                                id.to_string(),
                                OrphanedTask {
                                    task_id: id.to_string(),
                                    command: update["command"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_string(),
                                    cwd: update["cwd"].as_str().unwrap_or_default().to_string(),
                                },
                            );
                    }
                }
                Some(tag) if tag == *TASK_COMPLETED => {
                    if let Some(id) = update["task_snapshot"]["task_id"].as_str() {
                        pending.remove(id);
                    }
                }
                _ => {}
            }
        }
        pending.into_values().collect()
    }
    /// Emit `task_completed` for background tasks that were replayed as
    /// "Running" but whose processes no longer exist (cold session load).
    /// Returns completion receivers so the caller can drain them before
    /// returning LoadSessionResponse.
    pub(super) fn reconcile_stale_background_tasks(
        &self,
        session_id: &acp::SessionId,
        updates_file_path: &Option<PathBuf>,
    ) -> Vec<tokio::sync::oneshot::Receiver<xai_acp_lib::AcpResult<()>>> {
        let orphaned = Self::find_orphaned_background_tasks(updates_file_path);
        if orphaned.is_empty() {
            return Vec::new();
        }
        if self.sessions.borrow().get(session_id).is_some() {
            return Vec::new();
        }
        let mut completions = Vec::with_capacity(orphaned.len());
        for task in &orphaned {
            let snapshot = grow_tools::types::TaskSnapshot {
                task_id: task.task_id.clone(),
                command: task.command.clone(),
                display_command: None,
                cwd: task.cwd.clone(),
                start_time: std::time::SystemTime::now(),
                end_time: Some(std::time::SystemTime::now()),
                output: String::new(),
                output_file: std::path::PathBuf::new(),
                truncated: false,
                exit_code: None,
                signal: Some("session_restart".to_string()),
                completed: true,
                kind: grow_tools::computer::types::TaskKind::Bash,
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: None,
                description: None,
                is_backgrounded: true,
            };
            let notification = crate::extensions::notification::SessionNotification {
                session_id: session_id.clone(),
                update: crate::extensions::notification::SessionUpdate::TaskCompleted {
                    task_snapshot: snapshot,
                    will_wake: false,
                },
                meta: None,
            };
            if let Ok(params) = serde_json::to_value(&notification)
                .and_then(|v| serde_json::value::to_raw_value(&v))
            {
                completions
                    .push(
                        self
                            .gateway
                            .forward_with_completion(
                                acp::ExtNotification::new(
                                    "grow/task_completed",
                                    params.into(),
                                ),
                            ),
                    );
            }
        }
        if !completions.is_empty() {
            tracing::info!(
                session_id = %session_id.0,
                stale_count = completions.len(),
                "Emitted task_completed for stale background tasks"
            );
        }
        completions
    }
    /// Extracts initial_total_tokens by scanning only the tail of the updates file.
    /// Avoids loading and deserializing all updates when replay is skipped (noReplay).
    pub(super) fn extract_initial_tokens_from_updates(
        updates_file_path: &Option<PathBuf>,
    ) -> u64 {
        use std::io::{Read, Seek, SeekFrom};
        let Some(updates_path) = updates_file_path else {
            return 0;
        };
        let mut file = match std::fs::File::open(updates_path) {
            Ok(f) => f,
            Err(_) => return 0u64,
        };
        let file_len = match file.metadata() {
            Ok(m) => m.len(),
            Err(_) => return 0,
        };
        const TAIL_SIZE: u64 = 64 * 1024;
        let start_pos = file_len.saturating_sub(TAIL_SIZE);
        if file.seek(SeekFrom::Start(start_pos)).is_err() {
            return 0;
        }
        let mut buf = String::new();
        if file.read_to_string(&mut buf).is_err() {
            return 0;
        }
        let result = buf
            .lines()
            .rev()
            .filter(|line| !line.trim().is_empty())
            .find_map(|line| {
                let value: serde_json::Value = serde_json::from_str(line).ok()?;
                value.get("params")?.get("meta")?.get("totalTokens")?.as_u64()
            })
            .unwrap_or(0);
        if result == 0 {
            tracing::warn!(
                path = %updates_path.display(),
                "extract_initial_tokens: no totalTokens found in updates tail, \
                 token tracking will rely on conversation estimate until first model response"
            );
        }
        result
    }
    pub(crate) fn auth_response_with_meta(&self) -> AuthenticateResponse {
        let show_resolved_model = self
            .cfg
            .borrow()
            .remote_settings
            .as_ref()
            .and_then(|settings| settings.show_resolved_model);
        let meta = self.auth_manager.current().map(|auth| {
            let auth_meta = crate::auth::AuthMeta {
                email: auth.email.clone(),
                auth_mode: Some(format!("{:?}", auth.auth_mode)),
                team_id: auth.team_id.clone(),
                team_name: auth.team_name.clone(),
                is_zdr: auth.is_zdr_team(),
                team_role: auth.team_role.clone(),
                show_resolved_model,
            };
            serde_json::to_value(auth_meta)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default()
        });
        AuthenticateResponse::new().meta(meta)
    }
    /// Resolve current auto-GC policy and run it on the blocking pool.
    pub(super) fn spawn_auto_worktree_gc(&self) {
        let auto_gc_policy = self.cfg.borrow().resolve_worktree_auto_gc();
        tokio::task::spawn_blocking(move || {
            let opts = xai_fast_worktree::AutoGcOptions::from_resolved(auto_gc_policy);
            if let Err(e) = xai_fast_worktree::WorktreeDb::open_default()
                .and_then(|db| xai_fast_worktree::maybe_auto_gc(&db, &opts))
            {
                tracing::warn!(error = %e, "auto worktree gc failed");
            }
        });
    }
    /// Fire-and-forget `grow/settings/update` from the current remote snapshot.
    pub(super) fn emit_settings_update_notification(&self) {
        let payload = {
            let cfg = self.cfg.borrow();
            let rs = cfg.remote_settings.as_ref();
            SettingsUpdateNotification {
                show_resolved_model: rs.and_then(|s| s.show_resolved_model),
                session_picker_grouped: rs.and_then(|s| s.session_picker_grouped),
                tips: rs.and_then(|s| s.tips.clone()),
                slash_command_tags: rs.and_then(|s| s.slash_command_tags.clone()),
                campaigns: rs.map(|s| s.campaigns.clone()),
                auto_permission_mode_enabled: crate::util::config::remote_auto_mode_enabled(
                    rs,
                ),
                permission_mode: rs.and_then(|s| s.permission_mode.clone()),
                group_tool_verbs: rs.and_then(|s| s.group_tool_verbs),
                collapsed_edit_blocks: rs.and_then(|s| s.collapsed_edit_blocks),
            }
        };
        if let Ok(params) = serde_json::value::to_raw_value(&payload) {
            self.gateway
                .forward_fire_and_forget(
                    acp::ExtNotification::new("grow/settings/update", params.into()),
                );
        }
    }
    /// Fan out `RefreshSkillBaseline` to each provided sender.
    pub(super) fn broadcast_refresh_skill_baseline(
        senders: Vec<tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>>,
    ) {
        for tx in senders {
            let _ = tx.send(crate::session::SessionCommand::RefreshSkillBaseline);
        }
    }
    /// Snapshot live session senders and broadcast `RefreshSkillBaseline`.
    pub(super) fn refresh_skill_baseline_for_all_sessions(&self) {
        let senders = self
            .sessions
            .borrow()
            .values()
            .map(|h| h.cmd_tx.clone())
            .collect();
        Self::broadcast_refresh_skill_baseline(senders);
    }
    /// Eagerly fan out the current on-disk plugin registry to every live
    /// session so each adopts a cwd-correct snapshot (hooks + MCP + skills +
    /// client slash-command catalog) — the same refresh the session where the
    /// plugin changed already gets. Mirrors the MCP fan-out in
    /// `handle_plugins_reload`, extended to the whole registry. Each session
    /// gets its own `build_for_cwd` result because project-scoped plugins
    /// differ by working directory. `skip` avoids redundant work on a session
    /// that just self-updated (the originating session of a per-session
    /// reload). Subagents are skipped by the receiving actor.
    pub(crate) fn broadcast_plugin_registry_to_sessions(
        &self,
        skip: Option<&acp::SessionId>,
    ) {
        let targets: Vec<
            (
                std::path::PathBuf,
                tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>,
            ),
        > = self
            .sessions
            .borrow()
            .iter()
            .filter_map(|(sid, h)| {
                if skip == Some(sid) {
                    return None;
                }
                Some((std::path::PathBuf::from(&h.info.cwd), h.cmd_tx.clone()))
            })
            .collect();
        let remote_settings = self.cfg.borrow().remote_settings.clone();
        for (cwd, cmd_tx) in targets {
            let project_trusted = folder_trust::resolve_and_record(
                cwd.as_path(),
                remote_settings.as_ref(),
                false,
            );
            let disk_cfg = crate::config::resolve_effective_plugins_config(cwd.as_path())
                .to_discovery_config();
            let registry = self
                .plugin_registry_handle
                .build_for_cwd(cwd.as_path(), &disk_cfg, &[], project_trusted);
            let _ = cmd_tx
                .send(crate::session::SessionCommand::ReloadPlugins {
                    registry,
                });
        }
    }
    /// Spawn a best-effort bundle sync. Re-fires on every call site (init,
    /// cached_token, service.example.com/oidc); the cheap pre-checks below absorb repeats
    /// so reconnects are cheap.
    ///
    /// Pre-spawn gating order (cheapest first, all synchronous):
    /// 1. Auth gate — avoid spawning a no-op task on every init.
    /// 2. Freshness check — skip the sender snapshot + spawn entirely on
    ///    cache hits, which is the steady-state on every reconnect.
    /// 3. Single-flight guard — if a previous sync is still in flight (e.g.,
    ///    initialize + cached_token + oidc fired in quick succession before
    ///    the first sync's tar extract finished), drop this call to avoid
    ///    racing concurrent extracts that would interleave per-file writes
    ///    against `~/.grow/bundled/` and the manifest.
    pub(crate) fn maybe_sync_bundle_in_background(&self, force: bool) {
        use crate::extensions::bundle::{
            BUNDLE_SYNC_TTL, bundle_cache_is_fresh, has_bundle_credentials,
            maybe_sync_bundle_to_root,
        };
        use std::sync::atomic::Ordering;
        let am = self.auth_manager.clone();
        let deployment_key = self.deployment_key();
        if !has_bundle_credentials(Some(&am), deployment_key.as_deref()) {
            return;
        }
        let root = crate::bundle::bundled_root();
        if !force && bundle_cache_is_fresh(&root, BUNDLE_SYNC_TTL) {
            tracing::debug!("proactive bundle sync skipped pre-spawn: cache is fresh");
            return;
        }
        let in_flight = self.bundle_sync_in_flight.clone();
        if in_flight
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            tracing::debug!("proactive bundle sync skipped: another sync is already in flight");
            return;
        }
        let proxy_base_url = self.cli_chat_proxy_base_url();
        let alpha_test_key = self.alpha_test_key();
        let senders: Vec<
            tokio::sync::mpsc::UnboundedSender<crate::session::SessionCommand>,
        > = self.sessions.borrow().values().map(|h| h.cmd_tx.clone()).collect();
        tokio::task::spawn_local(async move {
            let result = maybe_sync_bundle_to_root(
                    &root,
                    &proxy_base_url,
                    Some(&am),
                    deployment_key.as_deref(),
                    alpha_test_key.as_deref(),
                    force,
                    BUNDLE_SYNC_TTL,
                )
                .await;
            in_flight.store(false, Ordering::Release);
            match result {
                Ok(Some(res)) => {
                    tracing::info!(
                        version = %res.version,
                        personas = res.personas_count,
                        roles = res.roles_count,
                        agents = res.agents_count,
                        skills = res.skills_count,
                        "proactive bundle sync complete"
                    );
                    Self::broadcast_refresh_skill_baseline(senders);
                }
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(error = %err, "proactive bundle sync failed");
                }
            }
        });
    }
}
#[cfg(test)]
mod tests;
#[cfg(test)]
mod prompt_response_meta_tests;
