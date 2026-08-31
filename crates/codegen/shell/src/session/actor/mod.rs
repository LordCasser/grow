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
use acp_transport::protocol as acp;
use agent::AgentDefinition;
use agent::prompt::skills::SkillsConfig;
use agent_client_protocol::schema::v1::ContentBlock;
use futures_util::FutureExt;
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
use tokio_util::sync::CancellationToken;
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
use slash_exec::HOST_COMMAND_INVOCATION;
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
mod laziness_classifier;
pub(crate) use laziness_classifier::*;
mod notification_drain;
use notification_drain::*;
mod extensions;
use extensions::*;
mod memory_dream;
use memory_dream::*;
pub(crate) mod goal_support;
pub(crate) use goal_support::*;
mod hook_dispatch;
use hook_dispatch::*;
mod stop_gate;
pub use stop_gate::MAX_STOP_HOOK_CONTINUATIONS_PER_TURN;
mod coordination;
mod idle_arbitration;
mod input_admission;
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
    /// Human input identities atomically consumed by this turn. Empty for all
    /// synthetic and host-owned internal turns.
    pub(crate) input_ids: Vec<String>,
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
    /// Presentation identity for a Shell-owned slash command. This survives
    /// idle scheduling without becoming part of the command's prompt blocks.
    pub(crate) host_command: Option<crate::session::HostCommandInvocation>,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminationState {
    Open,
    Graceful,
    Fatal,
}

impl TerminationState {
    pub(crate) fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    pub(crate) fn request(&mut self, requested: Self) {
        if matches!(requested, Self::Fatal) || matches!(self, Self::Open) {
            *self = requested;
        }
    }
}

struct AdmissionState {
    /// The sole owner of foreground execution. Goal's future continuation
    /// right is not foreground work and therefore cannot block user admission.
    pub(crate) foreground: ForegroundState,
    /// Actor-owned admission latch. Once termination begins, detached idle
    /// producers may finish an already-owned foreground transaction, but no
    /// new prompt, compaction, notification, Goal continuation, or Control
    /// transaction may be admitted.
    pub(crate) termination: TerminationState,
    /// One coalescing manual-compaction request admitted during a running turn.
    pub(crate) pending_manual_compact: Option<Option<String>>,
    /// Controls accepted while a step owns its immutable model-facing state.
    /// Sampling and Agent are latest-wins desired-state domains; ordered
    /// lifecycle mutations keep their causal admission sequence. The turn
    /// drains the resulting snapshot after StepEnded and before the next
    /// sample; an idle session drains it before admitting new foreground.
    pending_step_controls: PendingStepControls,
    /// Control currently being prepared or durably applied. A newer desired
    /// revision may coexist while an older Agent preparation finishes; UI
    /// snapshots always prefer the newer pending revision.
    applying_step_control: Option<StepControlProjection>,
    /// Monotonic Behavior control revision. Behavior retains its dedicated
    /// admission/confirmation state machine but shares the typed projection
    /// protocol with Sampling and Agent.
    behavior_control_revision: u64,
    /// Newest Behavior request not yet claimed by the dedicated worker.
    pending_behavior_control: Option<PendingBehaviorSelection>,
    /// Behavior request currently owned by the dedicated worker. A newer
    /// pending revision may coexist while capability or ownership checks run.
    applying_behavior_control: Option<StepControlProjection>,
    /// Exactly one local worker drains Behavior desired state. Keeping this
    /// outside the main mailbox allows later requests to supersede an older
    /// target while capability/ownership checks are still preparing.
    behavior_control_worker_active: bool,
    /// The Behavior worker claimed an otherwise-idle foreground before it was
    /// detached from the mailbox. This is the runtime admission fence that
    /// prevents a later prompt, compaction, notification, Goal continuation,
    /// or Sampling/Agent drain from overtaking the earlier Behavior request.
    behavior_control_foreground_claimed: bool,
    /// Per-client desired-state high-water marks. Actor-local revisions order
    /// accepted requests; these tokens reject transport reordering before it
    /// can manufacture the wrong local order.
    control_intents: std::collections::HashMap<
        (crate::extensions::notification::ControlDomain, String),
        ControlIntentReceipt,
    >,
    /// A durable lifecycle/Behavior preemption committed while the current
    /// foreground producer still owns the turn. No subsequent Step may start
    /// until that exact turn reaches its terminal.
    terminal_preemption_pending: bool,
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

impl AdmissionState {
    fn restore_terminal_control_intent(
        receipts: &mut std::collections::HashMap<
            (crate::extensions::notification::ControlDomain, String),
            ControlIntentReceipt,
        >,
        domain: crate::extensions::notification::ControlDomain,
        intent: &crate::session::ControlIntent,
        terminal: ControlIntentTerminal,
    ) -> Result<(), String> {
        let key = (domain, intent.client_id.clone());
        let token = (intent.generation, intent.sequence);
        match receipts.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(ControlIntentReceipt {
                    token,
                    lifecycle: ControlIntentLifecycle::Terminal(terminal),
                });
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry.get().token < token {
                    entry.insert(ControlIntentReceipt {
                        token,
                        lifecycle: ControlIntentLifecycle::Terminal(terminal),
                    });
                } else if entry.get().token == token {
                    let ControlIntentLifecycle::Terminal(existing) = &entry.get().lifecycle else {
                        return Err(
                            "persisted control receipt conflicts with an in-flight intent".into(),
                        );
                    };
                    if existing.phase != terminal.phase || existing.target != terminal.target {
                        return Err(format!(
                            "persisted {:?} control receipt has conflicting terminal facts for client `{}` generation {} sequence {}",
                            domain, intent.client_id, intent.generation, intent.sequence
                        ));
                    }
                    if terminal.ui_terminal_durable && !existing.ui_terminal_durable {
                        entry.insert(ControlIntentReceipt {
                            token,
                            lifecycle: ControlIntentLifecycle::Terminal(terminal),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn admit_control_intent(
        &mut self,
        domain: crate::extensions::notification::ControlDomain,
        intent: Option<&crate::session::ControlIntent>,
    ) -> ControlIntentAdmission {
        let Some(intent) = intent else {
            return ControlIntentAdmission::New;
        };
        let key = (domain, intent.client_id.clone());
        let candidate = (intent.generation, intent.sequence);
        if let Some(current) = self.control_intents.get(&key) {
            if current.token > candidate {
                return ControlIntentAdmission::Older;
            }
            if current.token == candidate {
                return match &current.lifecycle {
                    ControlIntentLifecycle::InFlight => ControlIntentAdmission::DuplicateInFlight,
                    ControlIntentLifecycle::Terminal(terminal) => {
                        ControlIntentAdmission::ExactTerminal(terminal.clone())
                    }
                };
            }
        }
        self.control_intents.insert(
            key,
            ControlIntentReceipt {
                token: candidate,
                lifecycle: ControlIntentLifecycle::InFlight,
            },
        );
        ControlIntentAdmission::New
    }

    fn mark_control_intent_terminal(
        &mut self,
        domain: crate::extensions::notification::ControlDomain,
        intent: Option<&crate::session::ControlIntent>,
        terminal: ControlIntentTerminal,
    ) {
        let Some(intent) = intent else {
            return;
        };
        let key = (domain, intent.client_id.clone());
        let token = (intent.generation, intent.sequence);
        if let Some(receipt) = self.control_intents.get_mut(&key)
            && receipt.token == token
        {
            receipt.lifecycle = ControlIntentLifecycle::Terminal(terminal);
        }
    }

    fn mark_control_terminal_ui_durable(
        &mut self,
        domain: crate::extensions::notification::ControlDomain,
        intent: &crate::session::ControlIntent,
    ) {
        let key = (domain, intent.client_id.clone());
        let token = (intent.generation, intent.sequence);
        if let Some(ControlIntentReceipt {
            token: current,
            lifecycle: ControlIntentLifecycle::Terminal(terminal),
        }) = self.control_intents.get_mut(&key)
            && *current == token
        {
            terminal.ui_terminal_durable = true;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ControlIntentLifecycle {
    InFlight,
    Terminal(ControlIntentTerminal),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControlIntentReceipt {
    token: (u64, u64),
    lifecycle: ControlIntentLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ControlIntentAdmission {
    New,
    DuplicateInFlight,
    ExactTerminal(ControlIntentTerminal),
    Older,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControlIntentTerminal {
    phase: crate::extensions::notification::ControlPhase,
    target: crate::extensions::notification::ControlTarget,
    message: Option<String>,
    ui_terminal_durable: bool,
}

struct PendingModelReload {
    catalog: std::sync::Arc<crate::agent::models::PublishedModelCatalog>,
    responders: Vec<tokio::sync::oneshot::Sender<Result<(), acp::Error>>>,
}

struct PendingModelSelection {
    route: crate::agent::models::PublishedSessionRoute,
    catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
    respond_to: tokio::sync::oneshot::Sender<
        Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
    >,
    intent: Option<crate::session::ControlIntent>,
}

struct PendingAgentSelection {
    preparation: std::rc::Rc<AgentPreparation>,
    respond_to:
        tokio::sync::oneshot::Sender<Result<crate::session::DesiredStateOutcome<()>, acp::Error>>,
    intent: Option<crate::session::ControlIntent>,
}

struct PendingBehaviorSelection {
    session_mode: acp::SessionModeId,
    revision: u64,
    confirmation_owner: Option<String>,
    responds_to: tokio::sync::oneshot::Sender<
        Result<crate::session::behavior::BehaviorChangeOutcome, acp::Error>,
    >,
    intent: Option<crate::session::ControlIntent>,
}

enum PendingGoalDefinitionMutation {
    Edit {
        objective: String,
        /// `None` preserves the budget already attached to the Goal.
        token_budget: Option<i64>,
    },
    Budget {
        /// `None` explicitly removes the limit.
        token_budget: Option<i64>,
    },
}

struct PendingGoalDefinitionControl {
    /// Stable lifecycle identity captured at command admission. Definition
    /// revisions are intentionally not captured: multiple edits admitted in
    /// one step compose in FIFO order against this same long-lived Goal.
    goal_id: String,
    mutation: PendingGoalDefinitionMutation,
    /// Present only for an idle admission, where the caller may safely wait
    /// for the immediately drained durable result. Active-turn commands return
    /// as scheduled instead of blocking the actor mailbox until StepEnded.
    responds_to: Option<tokio::sync::oneshot::Sender<Result<bool, String>>>,
}

struct AgentPreparation {
    target_name: String,
    result: std::cell::RefCell<Option<Result<agent::Agent, acp::Error>>>,
    superseded: std::cell::Cell<bool>,
    abort: std::cell::RefCell<Option<tokio::task::AbortHandle>>,
    ready: tokio::sync::Notify,
}

impl AgentPreparation {
    fn ready(target_name: String, result: Result<agent::Agent, acp::Error>) -> std::rc::Rc<Self> {
        std::rc::Rc::new(Self {
            target_name,
            result: std::cell::RefCell::new(Some(result)),
            superseded: std::cell::Cell::new(false),
            abort: std::cell::RefCell::new(None),
            ready: tokio::sync::Notify::new(),
        })
    }

    fn start(
        rebuild_spec: std::sync::Arc<crate::session::agent_rebuild::AgentRebuildSpec>,
        definition: agent::AgentDefinition,
        session_id: String,
    ) -> std::rc::Rc<Self> {
        let new_agent_name = definition.selector_identity();
        let preparation = std::rc::Rc::new(Self {
            target_name: new_agent_name.clone(),
            result: std::cell::RefCell::new(None),
            superseded: std::cell::Cell::new(false),
            abort: std::cell::RefCell::new(None),
            ready: tokio::sync::Notify::new(),
        });
        let output = std::rc::Rc::clone(&preparation);
        let task = tokio::task::spawn_local(async move {
            let result = std::panic::AssertUnwindSafe(rebuild_spec.build_agent(definition))
                .catch_unwind()
                .await
                .map_err(|_| {
                    tracing::error!(
                        %session_id,
                        new_agent_type = %new_agent_name,
                        "Agent preparation panicked"
                    );
                    acp::Error::internal_error().data(format!(
                        "rebuild_agent: preparation panicked for agent_type={new_agent_name}"
                    ))
                })
                .and_then(|result| {
                    result.map_err(|error| {
                        tracing::error!(
                            %session_id,
                            new_agent_type = %new_agent_name,
                            %error,
                            "Agent preparation failed"
                        );
                        acp::Error::internal_error().data(format!(
                            "rebuild_agent: build failed for agent_type={new_agent_name}: {error}"
                        ))
                    })
                });
            *output.result.borrow_mut() = Some(result);
            output.ready.notify_waiters();
        });
        *preparation.abort.borrow_mut() = Some(task.abort_handle());
        preparation
    }

    /// `false` means a newer desired Agent revision replaced this candidate.
    async fn wait_ready(&self) -> bool {
        loop {
            let ready = self.ready.notified();
            if self.superseded.get() {
                return false;
            }
            if self.result.borrow().is_some() {
                return true;
            }
            ready.await;
        }
    }

    async fn wait_superseded(&self) {
        loop {
            let changed = self.ready.notified();
            if self.superseded.get() {
                return;
            }
            changed.await;
        }
    }

    fn mark_superseded(&self) {
        self.superseded.set(true);
        if let Some(abort) = self.abort.borrow_mut().take() {
            abort.abort();
        }
        self.ready.notify_waiters();
    }

    fn has_agent(&self) -> bool {
        matches!(&*self.result.borrow(), Some(Ok(_)))
    }

    fn target_name(&self) -> &str {
        &self.target_name
    }

    fn take(&self) -> Result<agent::Agent, acp::Error> {
        self.result
            .borrow_mut()
            .take()
            .expect("Agent preparation must be ready and consumed exactly once")
    }
}

impl Drop for AgentPreparation {
    fn drop(&mut self) {
        if let Some(abort) = self.abort.get_mut().take() {
            abort.abort();
        }
    }
}

/// A concrete control claimed for application at a causal boundary.
enum PendingStepControl {
    ModelReload(PendingModelReload),
    ModelSelection(PendingModelSelection),
    AgentSelection(PendingAgentSelection),
    GoalDefinition(PendingGoalDefinitionControl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingStepControlKey {
    Ordered { sequence: u64 },
    Sampling { sequence: u64, revision: u64 },
    Agent { sequence: u64, revision: u64 },
}

struct SequencedStepControl<T> {
    sequence: u64,
    revision: u64,
    value: T,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StepControlProjection {
    revision: u64,
    target: crate::extensions::notification::ControlTarget,
    intent: Option<crate::session::ControlIntent>,
}

/// Immutable admission horizon captured atomically with `StepEnded`.
/// Controls admitted after this sequence belong to the next Step even when
/// they arrive before its `StepStarted` event is appended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StepControlBoundary {
    cutoff: Option<u64>,
    /// A desired-state domain that existed at `StepEnded` stays eligible for
    /// this boundary until one revision durably settles. A newer revision may
    /// replace an in-flight preparation and inherit the same boundary; a
    /// domain first admitted after `StepEnded` must wait for the next one.
    sampling_eligible: bool,
    agent_eligible: bool,
}

impl StepControlBoundary {
    fn close_domain(&mut self, key: PendingStepControlKey) {
        match key {
            PendingStepControlKey::Sampling { .. } => self.sampling_eligible = false,
            PendingStepControlKey::Agent { .. } => self.agent_eligible = false,
            PendingStepControlKey::Ordered { .. } => {}
        }
    }
}

/// Server-authoritative desired state for the next causal Step boundary.
///
/// Sampling and Agent each have exactly one replaceable slot. Goal-definition
/// changes and catalog reloads are lifecycle mutations, not user-facing
/// desired-state domains, so they remain ordered. A global admission sequence
/// provides deterministic application order between the surviving targets
/// and those ordered mutations without retaining superseded controls.
#[derive(Default)]
struct PendingStepControls {
    next_sequence: u64,
    sampling_revision: u64,
    agent_revision: u64,
    sampling: Option<SequencedStepControl<PendingModelSelection>>,
    agent: Option<SequencedStepControl<PendingAgentSelection>>,
    ordered: VecDeque<SequencedStepControl<PendingStepControl>>,
}

impl PendingStepControls {
    fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }

    fn is_empty(&self) -> bool {
        self.sampling.is_none() && self.agent.is_none() && self.ordered.is_empty()
    }

    fn revision(&self, domain: crate::extensions::notification::ControlDomain) -> u64 {
        match domain {
            crate::extensions::notification::ControlDomain::Sampling => self.sampling_revision,
            crate::extensions::notification::ControlDomain::Agent => self.agent_revision,
            crate::extensions::notification::ControlDomain::Behavior => 0,
        }
    }

    fn reserve_terminal_replay_revision(
        &mut self,
        domain: crate::extensions::notification::ControlDomain,
    ) -> u64 {
        match domain {
            crate::extensions::notification::ControlDomain::Sampling => {
                self.sampling_revision = self.sampling_revision.saturating_add(1);
                self.sampling_revision
            }
            crate::extensions::notification::ControlDomain::Agent => {
                self.agent_revision = self.agent_revision.saturating_add(1);
                self.agent_revision
            }
            crate::extensions::notification::ControlDomain::Behavior => {
                unreachable!("Behavior revisions are owned by AdmissionState")
            }
        }
    }

    fn len(&self) -> usize {
        usize::from(self.sampling.is_some())
            + usize::from(self.agent.is_some())
            + self.ordered.len()
    }

    fn desired_sampling_model_id(&self) -> Option<crate::agent::models::ModelId> {
        self.sampling
            .as_ref()
            .map(|pending| pending.value.route.model_id.clone())
    }

    fn admit_sampling(
        &mut self,
        route: crate::agent::models::PublishedSessionRoute,
        catalog: Option<std::sync::Arc<crate::agent::models::PublishedModelCatalog>>,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<crate::agent::models::ModelId>, acp::Error>,
        >,
    ) -> (u64, Option<(u64, PendingModelSelection)>) {
        self.sampling_revision = self.sampling_revision.saturating_add(1);
        let revision = self.sampling_revision;
        let sequence = self.next_sequence();
        let superseded = self
            .sampling
            .take()
            .map(|pending| (pending.revision, pending.value));
        self.sampling = Some(SequencedStepControl {
            sequence,
            revision,
            value: PendingModelSelection {
                route,
                catalog,
                respond_to: responds_to,
                intent,
            },
        });
        (revision, superseded)
    }

    fn admit_agent(
        &mut self,
        preparation: std::rc::Rc<AgentPreparation>,
        intent: Option<crate::session::ControlIntent>,
        responds_to: tokio::sync::oneshot::Sender<
            Result<crate::session::DesiredStateOutcome<()>, acp::Error>,
        >,
    ) -> (u64, Option<(u64, PendingAgentSelection)>) {
        self.agent_revision = self.agent_revision.saturating_add(1);
        let revision = self.agent_revision;
        let sequence = self.next_sequence();
        let superseded = if let Some(pending) = self.agent.take() {
            pending.value.preparation.mark_superseded();
            Some((pending.revision, pending.value))
        } else {
            None
        };
        self.agent = Some(SequencedStepControl {
            sequence,
            revision,
            value: PendingAgentSelection {
                preparation,
                respond_to: responds_to,
                intent,
            },
        });
        (revision, superseded)
    }

    fn admit_model_reload(
        &mut self,
        catalog: std::sync::Arc<crate::agent::models::PublishedModelCatalog>,
        responds_to: tokio::sync::oneshot::Sender<Result<(), acp::Error>>,
    ) {
        let last_sequence = [
            self.sampling.as_ref().map(|pending| pending.sequence),
            self.agent.as_ref().map(|pending| pending.sequence),
            self.ordered.back().map(|pending| pending.sequence),
        ]
        .into_iter()
        .flatten()
        .max();
        if self.ordered.back().is_some_and(|pending| {
            Some(pending.sequence) == last_sequence
                && matches!(pending.value, PendingStepControl::ModelReload(_))
        }) {
            let PendingStepControl::ModelReload(pending) =
                &mut self.ordered.back_mut().expect("checked above").value
            else {
                unreachable!("checked model reload variant")
            };
            pending.catalog = catalog;
            pending.responders.push(responds_to);
            return;
        }
        let sequence = self.next_sequence();
        self.ordered.push_back(SequencedStepControl {
            sequence,
            revision: 0,
            value: PendingStepControl::ModelReload(PendingModelReload {
                catalog,
                responders: vec![responds_to],
            }),
        });
    }

    fn admit_goal_definition(&mut self, pending: PendingGoalDefinitionControl) {
        let sequence = self.next_sequence();
        self.ordered.push_back(SequencedStepControl {
            sequence,
            revision: 0,
            value: PendingStepControl::GoalDefinition(pending),
        });
    }

    fn next_key(&self) -> Option<PendingStepControlKey> {
        let ordered = self
            .ordered
            .front()
            .map(|pending| PendingStepControlKey::Ordered {
                sequence: pending.sequence,
            });
        let sampling = self
            .sampling
            .as_ref()
            .map(|pending| PendingStepControlKey::Sampling {
                sequence: pending.sequence,
                revision: pending.revision,
            });
        let agent = self
            .agent
            .as_ref()
            .map(|pending| PendingStepControlKey::Agent {
                sequence: pending.sequence,
                revision: pending.revision,
            });
        [ordered, sampling, agent]
            .into_iter()
            .flatten()
            .min_by_key(|key| match key {
                PendingStepControlKey::Ordered { sequence }
                | PendingStepControlKey::Sampling { sequence, .. }
                | PendingStepControlKey::Agent { sequence, .. } => *sequence,
            })
    }

    fn boundary(&self) -> StepControlBoundary {
        StepControlBoundary {
            cutoff: self.next_sequence.checked_sub(1),
            sampling_eligible: self.sampling.is_some(),
            agent_eligible: self.agent.is_some(),
        }
    }

    fn next_key_at_boundary(&self, boundary: StepControlBoundary) -> Option<PendingStepControlKey> {
        let ordered = boundary.cutoff.and_then(|cutoff| {
            self.ordered
                .front()
                .filter(|pending| pending.sequence <= cutoff)
                .map(|pending| PendingStepControlKey::Ordered {
                    sequence: pending.sequence,
                })
        });
        let sampling = boundary.sampling_eligible.then(|| {
            self.sampling
                .as_ref()
                .map(|pending| PendingStepControlKey::Sampling {
                    sequence: pending.sequence,
                    revision: pending.revision,
                })
        });
        let agent = boundary.agent_eligible.then(|| {
            self.agent
                .as_ref()
                .map(|pending| PendingStepControlKey::Agent {
                    sequence: pending.sequence,
                    revision: pending.revision,
                })
        });
        [ordered, sampling.flatten(), agent.flatten()]
            .into_iter()
            .flatten()
            .min_by_key(|key| match key {
                PendingStepControlKey::Ordered { sequence }
                | PendingStepControlKey::Sampling { sequence, .. }
                | PendingStepControlKey::Agent { sequence, .. } => *sequence,
            })
    }

    fn agent_preparation(
        &self,
        key: PendingStepControlKey,
    ) -> Option<std::rc::Rc<AgentPreparation>> {
        let PendingStepControlKey::Agent { sequence, revision } = key else {
            return None;
        };
        self.agent
            .as_ref()
            .filter(|pending| pending.sequence == sequence && pending.revision == revision)
            .map(|pending| std::rc::Rc::clone(&pending.value.preparation))
    }

    fn projection(&self, key: PendingStepControlKey) -> Option<StepControlProjection> {
        match key {
            PendingStepControlKey::Sampling { sequence, revision } => self
                .sampling
                .as_ref()
                .filter(|pending| pending.sequence == sequence && pending.revision == revision)
                .map(|pending| StepControlProjection {
                    revision,
                    target: crate::extensions::notification::ControlTarget::Sampling {
                        model_id: pending.value.route.model_id.0.to_string(),
                        reasoning_effort: pending
                            .value
                            .route
                            .sampling_config
                            .reasoning_effort
                            .map(|effort| effort.to_string()),
                    },
                    intent: pending.value.intent.clone(),
                }),
            PendingStepControlKey::Agent { sequence, revision } => self
                .agent
                .as_ref()
                .filter(|pending| pending.sequence == sequence && pending.revision == revision)
                .map(|pending| StepControlProjection {
                    revision,
                    target: crate::extensions::notification::ControlTarget::Agent {
                        agent_name: pending.value.preparation.target_name().to_owned(),
                    },
                    intent: pending.value.intent.clone(),
                }),
            PendingStepControlKey::Ordered { .. } => None,
        }
    }

    fn domain_projection(
        &self,
        domain: crate::extensions::notification::ControlDomain,
    ) -> Option<StepControlProjection> {
        match domain {
            crate::extensions::notification::ControlDomain::Sampling => {
                self.sampling.as_ref().and_then(|pending| {
                    self.projection(PendingStepControlKey::Sampling {
                        sequence: pending.sequence,
                        revision: pending.revision,
                    })
                })
            }
            crate::extensions::notification::ControlDomain::Agent => {
                self.agent.as_ref().and_then(|pending| {
                    self.projection(PendingStepControlKey::Agent {
                        sequence: pending.sequence,
                        revision: pending.revision,
                    })
                })
            }
            crate::extensions::notification::ControlDomain::Behavior => None,
        }
    }

    fn take(&mut self, key: PendingStepControlKey) -> Option<PendingStepControl> {
        match key {
            PendingStepControlKey::Ordered { sequence } => self
                .ordered
                .front()
                .is_some_and(|pending| pending.sequence == sequence)
                .then(|| self.ordered.pop_front().expect("checked above").value),
            PendingStepControlKey::Sampling { sequence, revision } => self
                .sampling
                .as_ref()
                .is_some_and(|pending| pending.sequence == sequence && pending.revision == revision)
                .then(|| {
                    PendingStepControl::ModelSelection(
                        self.sampling.take().expect("checked above").value,
                    )
                }),
            PendingStepControlKey::Agent { sequence, revision } => self
                .agent
                .as_ref()
                .is_some_and(|pending| pending.sequence == sequence && pending.revision == revision)
                .then(|| {
                    PendingStepControl::AgentSelection(
                        self.agent.take().expect("checked above").value,
                    )
                }),
        }
    }

    fn cancel_goal_definitions(&mut self, goal_id: &str, message: &str) {
        self.ordered.retain_mut(|pending| {
            let PendingStepControl::GoalDefinition(goal) = &mut pending.value else {
                return true;
            };
            if goal.goal_id != goal_id {
                return true;
            }
            if let Some(respond_to) = goal.responds_to.take() {
                let _ = respond_to.send(Err(message.to_owned()));
            }
            false
        });
    }

    fn cancel_for_shutdown(&mut self) {
        let error = || acp::Error::internal_error().data("session is shutting down");
        if let Some(pending) = self.sampling.take() {
            let _ = pending.value.respond_to.send(Err(error()));
        }
        if let Some(pending) = self.agent.take() {
            pending.value.preparation.mark_superseded();
            let _ = pending.value.respond_to.send(Err(error()));
        }
        for pending in self.ordered.drain(..) {
            match pending.value {
                PendingStepControl::ModelReload(pending) => {
                    for respond_to in pending.responders {
                        let _ = respond_to.send(Err(error()));
                    }
                }
                PendingStepControl::GoalDefinition(mut pending) => {
                    if let Some(respond_to) = pending.responds_to.take() {
                        let _ = respond_to.send(Err("session is shutting down".to_string()));
                    }
                }
                PendingStepControl::ModelSelection(_) | PendingStepControl::AgentSelection(_) => {
                    unreachable!("replaceable controls never enter the ordered queue")
                }
            }
        }
    }
}

pub(crate) enum ForegroundState {
    Idle,
    /// An idle model/effort/Agent control is being durably applied. This
    /// fences every idle consumer while allowing the control implementation
    /// to release admission locks before calling back into capability/runtime
    /// refresh paths.
    ApplyingControl,
    RegularTurn(AgentTask),
    /// The runner has returned, but its canonical Timeline terminal has not
    /// completed the durable barrier yet.  This is still foreground
    /// ownership: admitting another turn here would let the successor's
    /// `TurnStarted` overtake its predecessor's `TurnEnded`.
    Settling {
        prompt_id: String,
        origin: crate::session::PromptOrigin,
        turn_kind: crate::session::TurnKind,
    },
    Compaction,
}

impl ForegroundState {
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }

    pub(crate) fn regular(&self) -> Option<&AgentTask> {
        match self {
            Self::RegularTurn(task) => Some(task),
            Self::Idle | Self::ApplyingControl | Self::Settling { .. } | Self::Compaction => None,
        }
    }

    pub(crate) fn regular_mut(&mut self) -> Option<&mut AgentTask> {
        match self {
            Self::RegularTurn(task) => Some(task),
            Self::Idle | Self::ApplyingControl | Self::Settling { .. } | Self::Compaction => None,
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

    /// Transfer the regular runner's payload to settlement while retaining a
    /// non-idle foreground fence until the durable terminal is committed.
    pub(crate) fn begin_settling(&mut self) -> Option<AgentTask> {
        match std::mem::replace(self, Self::Idle) {
            Self::RegularTurn(task) => {
                let prompt_id = task.prompt_id.clone();
                *self = Self::Settling {
                    prompt_id,
                    origin: task.origin.clone(),
                    turn_kind: task.turn_kind,
                };
                Some(task)
            }
            other => {
                *self = other;
                None
            }
        }
    }

    pub(crate) fn begin_terminalization(&mut self, prompt_id: &str) -> bool {
        matches!(self.regular(), Some(task) if task.prompt_id == prompt_id)
            && self.begin_settling().is_some()
    }

    pub(crate) fn settling_identity(
        &self,
        prompt_id: &str,
    ) -> Option<(crate::session::PromptOrigin, crate::session::TurnKind)> {
        match self {
            Self::Settling {
                prompt_id: active,
                origin,
                turn_kind,
            } if active == prompt_id => Some((origin.clone(), *turn_kind)),
            _ => None,
        }
    }

    /// Structured identity for the exact prompt across both producer-owned
    /// and durable-terminal settlement phases.
    pub(crate) fn identity(
        &self,
        prompt_id: &str,
    ) -> Option<(crate::session::PromptOrigin, crate::session::TurnKind)> {
        match self {
            Self::RegularTurn(task) if task.prompt_id == prompt_id => {
                Some((task.origin.clone(), task.turn_kind))
            }
            Self::Settling {
                prompt_id: active,
                origin,
                turn_kind,
            } if active == prompt_id => Some((origin.clone(), *turn_kind)),
            Self::Idle
            | Self::ApplyingControl
            | Self::RegularTurn(_)
            | Self::Settling { .. }
            | Self::Compaction => None,
        }
    }

    pub(crate) fn finish_settling(&mut self, prompt_id: &str) -> bool {
        if matches!(self, Self::Settling { prompt_id: active, .. } if active == prompt_id) {
            *self = Self::Idle;
            true
        } else {
            false
        }
    }
}

impl AdmissionState {
    /// Whether `prompt_id` still owns the live regular-turn admission epoch.
    /// Step and provider admission both use this predicate while holding the
    /// step-control gate, so termination and terminal preemption cannot split
    /// those two boundaries into subtly different policies.
    pub(crate) fn can_continue_regular_turn(&self, prompt_id: &str) -> bool {
        self.termination.is_open()
            && !self.terminal_preemption_pending
            && matches!(
                self.foreground.regular(),
                Some(task) if task.prompt_id == prompt_id
            )
    }

    /// Prompt id of the in-flight regular turn, if any. Foreground ownership
    /// and the FIFO share this lock, so completion and queue mutations compare
    /// against one race-free identity.
    pub(crate) fn running_prompt_id(&self) -> Option<&str> {
        match &self.foreground {
            ForegroundState::RegularTurn(task) => Some(task.prompt_id.as_str()),
            ForegroundState::Settling { prompt_id, .. } => Some(prompt_id.as_str()),
            ForegroundState::Idle
            | ForegroundState::ApplyingControl
            | ForegroundState::Compaction => None,
        }
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
    state.termination.is_open()
        && state.foreground.is_idle()
        && state.pending_inputs.is_empty()
        && !state.notifications_suppressed
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
        || !state.pending_step_controls.is_empty()
        || state.applying_step_control.is_some()
        || state.behavior_control_worker_active
        || state.pending_behavior_control.is_some()
        || state.applying_behavior_control.is_some()
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
    /// Explicit continuation semantics after a successful dispatch. This is
    /// frozen from the validated typed input; post-flight must not infer it
    /// from the broader tool kind.
    success_control: Option<ControlDisposition>,
    /// Exact Plan snapshot that a successful complete/cancel call must retire.
    /// The transition is committed post-dispatch so tool failure can never
    /// leave durable Behavior state ahead of its visible result.
    plan_exit_on_success: Option<crate::session::behavior::BehaviorSnapshot>,
}

/// Exhaustive result of preflighting one provider tool call. A resolved call
/// has already emitted its user-visible outcome and must never be dispatched.
#[derive(Debug)]
pub(crate) enum ToolPreflight {
    Dispatch(PreparedToolCall),
    Resolved {
        loop_result: ToolLoop,
        /// Observe-only lifecycle that is causally downstream of the
        /// undispatched tool's durable terminal fact.  PermissionDenied is
        /// the current producer: emitting it during preflight would point at
        /// an open Tool and violate the Timeline's post-tool contract.
        post_terminal_hook: Option<DeferredObserveHook>,
    },
}

impl ToolPreflight {
    fn resolved(loop_result: ToolLoop) -> Self {
        Self::Resolved {
            loop_result,
            post_terminal_hook: None,
        }
    }

    fn resolved_with_post_terminal_hook(
        loop_result: ToolLoop,
        post_terminal_hook: DeferredObserveHook,
    ) -> Self {
        Self::Resolved {
            loop_result,
            post_terminal_hook: Some(post_terminal_hook),
        }
    }
}

#[derive(Debug)]
pub(crate) struct DeferredObserveHook {
    event: ::hooks::event::HookEventName,
    cause: chat_state::HookCause,
    payload: ::hooks::event::HookPayload,
    prompt_id: Option<String>,
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
/// One memoized model's auth state, keyed by canonical `provider/model` id; see
/// [`SessionActor::model_auth_memo`] for the invalidation contract.
#[derive(Clone)]
pub(crate) struct ModelAuthMemo {
    pub(crate) catalog_model_id: String,
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
    /// Monotonic identity of the file + client handler configuration. Every
    /// occurrence captures this together with its cloned handler plan, so a
    /// mid-dispatch reload cannot change membership or ordering.
    generation: std::cell::Cell<u64>,
    resolved_workspace_root: String,
    vcs_kind: workspace::session::git::VcsKind,
    load_errors: std::cell::RefCell<Vec<String>>,
}

/// Phase 3: Post-flight handling after dispatch (inline in execute_tool_calls for now).
pub(crate) struct SessionActor {
    pub(crate) session_info: SessionInfo,
    /// Unique authority epoch for actor-local desired-state revisions.
    pub(crate) control_epoch: String,
    #[cfg(test)]
    pub(crate) test_session_dir_guard: Option<tempfile::TempDir>,
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
    /// Serializes immutable input artifacts with admission and orphan sweep.
    pub(crate) input_artifact_gate: TokioMutex<()>,
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
    /// Prevents turn cancellation from dropping a step-boundary route
    /// transition halfway through its durable commit and live-state swap.
    /// The turn remains abortable everywhere else.
    step_control_gate: TokioMutex<()>,
    /// Serializes every root Goal read-modify-persist transaction. Goal token
    /// settlement can arrive from primary sampling, Sideband work, or child
    /// mailboxes while lifecycle commands mutate the same durable snapshot;
    /// the in-memory rollback path is only sound when those transactions are
    /// linearized.
    goal_transaction_gate: TokioMutex<()>,
    /// Notification transport: gateway, persistence channel, replay buffer.
    pub(crate) notifications: NotificationSender,
    pub(crate) permissions: PermissionHandle,
    pub(crate) tool_context: ToolContext,
    /// Read-deny glob patterns, resolved once at construction and
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
    /// Session-scoped cancellation authority for auxiliary model ledgers.
    /// Sideband durability retries are exact and may otherwise wait forever
    /// after the session has entered teardown.
    pub(crate) sideband_cancel: tokio_util::sync::CancellationToken,
    /// SessionEnd memory/dream work runs only after ordinary auxiliary owners
    /// have been cancelled and drained, so it receives an isolated token that
    /// cannot accidentally resurrect the normal Sideband epoch.
    pub(crate) finalizer_sideband_cancel: tokio_util::sync::CancellationToken,
    /// Drop recovery uses an epoch owned by the Session rather than an
    /// untracked token. Fatal/final teardown revokes it and drains the
    /// associated activity permits before crossing the persistence barrier.
    pub(crate) sideband_repair_cancel: tokio_util::sync::CancellationToken,
    /// Cancellation epoch for append-only UI facts. Provider Sidebands stop
    /// at graceful-shutdown entry, while control terminals, permission audit,
    /// and child lifecycle notices remain valid until their producers drain.
    /// Keeping those epochs distinct prevents shutdown from discarding the
    /// final user-visible state before the persistence frontier.
    pub(crate) durable_ui_cancel: tokio_util::sync::CancellationToken,
    /// Once fatal/final persistence begins, Drop recovery must not create an
    /// independent post-barrier Sideband writer.
    pub(crate) sideband_fail_stop: Arc<std::sync::atomic::AtomicBool>,
    /// Linearizes Sideband activity acquisition with fail-stop publication so
    /// no nested writer can appear after the final activity-idle observation.
    pub(crate) sideband_admission_gate: tokio::sync::Mutex<()>,
    /// Finite work detached from foreground admission (title/recap/auxiliary
    /// model calls, memory timers, manual compaction). Teardown closes this
    /// admission authority and drains every already-issued permit before the
    /// Goal window or final persistence barrier is closed.
    pub(crate) session_activities: SessionActivityTracker,
    /// Accepted coordination work is actor-owned: one active inquiry and a
    /// bounded FIFO of waiting inquiries for this target Session.
    coordination_inquiries: std::cell::RefCell<VecDeque<coordination::QueuedCoordinationInquiry>>,
    coordination_inquiry_active: std::cell::Cell<bool>,
    /// Long-lived session services are explicit owners rather than detached
    /// LocalSet tasks. Teardown closes their ingress, then joins them before
    /// dismantling persistence and permission authorities.
    mcp_dispatcher_worker: TaskSlot<()>,
    mcp_initialization_worker: TaskSlot<()>,
    project_discovery_worker: TaskSlot<()>,
    fs_watch_handle: std::cell::RefCell<Option<crate::session::fs_watch::FsWatchHandle>>,
    /// Shared stop epoch for the remaining long-lived Session services. Each
    /// service also has an explicit join owner below; the token requests a
    /// cooperative stop before teardown applies its bounded join policy.
    background_service_shutdown: CancellationToken,
    user_question_worker: TaskSlot<()>,
    context_recall_worker: TaskSlot<()>,
    notification_reconciliation_worker: TaskSlot<()>,
    memory_reindex_worker: TaskSlot<()>,
    /// Actor-owned handles for the two detached control drains. Retaining the
    /// JoinHandles makes shutdown a bounded join instead of polling mutable
    /// flags forever, and preserves panic results for fail-closed teardown.
    step_control_worker: TaskSlot<Result<(), String>>,
    behavior_control_worker: TaskSlot<Result<(), String>>,
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
    /// Actor-committed Agent identity shared with every `SessionHandle` clone.
    pub(crate) agent_profile: crate::session::handle::SessionAgentProfile,
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
    pub(crate) permission_audit_bridge:
        parking_lot::Mutex<Option<tokio::task::JoinHandle<Result<(), String>>>>,
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
    /// Single actor-committed catalog/provider route shared with handle-side
    /// readers. Every update replaces the complete snapshot atomically.
    pub(crate) model_route: crate::session::handle::SessionModelRoute,
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
    /// Session-owned asynchronous Goal admission preparation. Keeping its
    /// JoinHandle in a TaskSlot makes shutdown cancellation explicit while
    /// preparation stays off the mailbox path.
    goal_drive: TaskSlot<()>,
    /// Durable long-lived Goal state. Idle continuation authority is runtime
    /// state and is deliberately not persisted in this value.
    pub(crate) goal_tracker: Arc<parking_lot::Mutex<crate::session::goal_tracker::GoalTracker>>,
    /// Root-owned activity window and accounting ingress shared with all
    /// descendant model runtimes. This is separate from delegation ownership:
    /// every model call settled while the Goal is Active is chargeable.
    pub(crate) goal_usage_window: goal_support::GoalUsageWindow,
    /// Background tasks (and monitors) that originated during
    /// a Goal turn, including surviving tasks reparented from delegated
    /// children. Their late
    /// auto-wake completions are dropped by [`Self::maybe_drain_notifications`]
    /// regardless of the goal's current status, so a leftover dev/verification
    /// server that completes after the run ended (Blocked / paused / cleared)
    /// cannot wake the idle parent. Reset only when a new Goal starts; clearing
    /// the old Goal must not erase ownership of work that is still running.
    pub(crate) goal_turn_task_ids:
        parking_lot::Mutex<std::collections::HashMap<String, (String, u64)>>,
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
    /// Session-owned Workflow ingress worker. Teardown closes admission,
    /// wakes this worker to reject queued envelopes, and joins it before
    /// draining Workflow executors.
    pub(crate) workflow_worker: TaskSlot<()>,
    pub(crate) workflow_service_shutdown: CancellationToken,
    /// Background-computed user-message prefix, injected before the first prompt.
    pub(crate) deferred_prefix: TaskSlot<String>,
    /// Re-parked Plan approval is an open-ended reverse request. Retaining its
    /// owner lets teardown abort and join it before the final Control/Timeline
    /// persistence barrier.
    restored_plan_approval: TaskSlot<()>,
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
    /// Plugin registry snapshot for this session. Updated on `/plugins reload`
    /// and shared with new-Workflow admission so one live registry authority
    /// feeds both Agent rebuilds and Run-scoped catalog freezing.
    pub(crate) plugin_registry: crate::session::workflow::tracker::SharedWorkflowPluginRegistry,
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
    /// Sole shutdown/join owner for the sampler actor.
    sampler_owner: std::cell::RefCell<Option<sampler::SamplerOwner>>,
    /// Drains sampler events and is joined after sampler shutdown closes the channel.
    sampler_event_drainer: TaskSlot<()>,
    /// Cached recipe for constructing this session's [`agent::Agent`].
    ///
    /// Populated once at session spawn and then reused by the next-step Agent
    /// selection path to build a fresh `Agent`
    /// (system prompt, [`tools::bridge::ToolBridge`], tool
    /// registry, and tool name aliases) when the user selects another Agent.
    /// Session lifecycle policy is deliberately absent: an Agent switch must
    /// not alter compaction, permission, or provider state.
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
    ) -> Result<(), chat_state::TimelineWriteError> {
        self.events
            .emit_turn_ended(outcome, terminal, category, context)
            .await
    }
    /// Current canonical `provider/model` identity. The provider-facing wire
    /// model lives only in `SessionModelRoute::sampling_config`; catalog and
    /// credential lookups must never use it as an identity.
    fn current_catalog_model_id(&self) -> String {
        self.model_route.snapshot().model_id.0.to_string()
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
        let has_workflow_runs = self.workflow_tracker().await.lock().has_runs();
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
            plugins: self.plugin_registry.read().is_some(),
            goal,
            // Workflow is a Shell-owned control-plane capability. Keep slash
            // command availability aligned with the runtime gate even when a
            // selected Agent definition has no authored Workflow tool.
            workflows: self.background_workflows_enabled
                && !self.startup_hints.is_subagent
                && !self.workflow_service_shutdown.is_cancelled(),
            workflow_management: has_workflow_runs,
            workflow_behavior: self.behavior.lock().behavior() == tool_types::BehaviorId::Workflow,
        }
    }
    /// Names of every tool registered with the session's tool bridge.
    ///
    /// Async wrapper that fetches `tool_definitions_builtins_only()` and
    /// projects to the `function.name` field. Allocates one `Vec<String>` per call;
    /// callers that need both gating and the wire payload should call
    /// once and pass the slice to `build_command_availability`.
    /// Capability projection for Shell-owned runtimes and slash commands.
    /// Remote MCP tools are callable data-plane extensions, not authority to
    /// impersonate Goal, Workflow, Scheduler, or memory runtimes by name.
    async fn registered_tool_names(&self) -> Vec<String> {
        let bridge = self.agent.borrow().tool_bridge().clone();
        bridge
            .tool_definitions_builtins_only()
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
    /// Append durable, UI-only output for a Shell-owned command or lifecycle
    /// control. It never enters ChatState and never masquerades as assistant
    /// prose. The outer notification metadata supplies the immutable event id;
    /// `invocation_id` groups multiple terminal rows from one command.
    async fn send_host_turn_slash_command_output(&self, text: &str) {
        self.send_host_turn_slash_command_notice(
            crate::extensions::notification::UiNoticeTone::Info,
            text,
            None,
        )
        .await;
    }

    async fn send_host_turn_slash_command_notice(
        &self,
        tone: crate::extensions::notification::UiNoticeTone,
        message: &str,
        details: Option<String>,
    ) {
        let invocation = HOST_COMMAND_INVOCATION
            .try_with(Clone::clone)
            .unwrap_or_else(|_| crate::session::HostCommandInvocation {
                command: "session-control".to_string(),
                description: "Session lifecycle control".to_string(),
                invocation_id: format!("session-control-{}", uuid::Uuid::now_v7()),
            });
        self.send_ui_notice(crate::extensions::notification::UiNotice {
            correlation_id: invocation.invocation_id,
            category: crate::extensions::notification::UiNoticeCategory::Command,
            subject: Some(invocation.command),
            description: Some(invocation.description),
            message: message.to_string(),
            tone,
            details,
        })
        .await;
    }

    async fn send_lifecycle_notice(
        &self,
        subject: &str,
        tone: crate::extensions::notification::UiNoticeTone,
        message: &str,
        details: Option<String>,
    ) {
        self.send_ui_notice(crate::extensions::notification::UiNotice {
            correlation_id: format!("lifecycle-{subject}-{}", uuid::Uuid::now_v7()),
            category: crate::extensions::notification::UiNoticeCategory::Lifecycle,
            subject: Some(subject.to_string()),
            description: None,
            message: message.to_string(),
            tone,
            details,
        })
        .await;
    }

    async fn send_ui_notice(&self, notice: crate::extensions::notification::UiNotice) {
        if let Err(error) = self.persist_ui_notice(notice).await {
            tracing::warn!(
                session_id = %self.session_info.id.0,
                error = %error,
                "failed to persist Shell command output",
            );
        }
    }

    async fn persist_ui_notice(
        &self,
        notice: crate::extensions::notification::UiNotice,
    ) -> Result<(), String> {
        let update = crate::extensions::notification::SessionUpdate::UiNotice(notice);
        self.send_grow_passive_notification(update.clone(), update)
            .await
            .map_err(|error| error.to_string())
    }

    async fn send_host_turn_slash_command_success(&self, message: &str) {
        self.send_host_turn_slash_command_notice(
            crate::extensions::notification::UiNoticeTone::Success,
            message,
            None,
        )
        .await;
    }

    async fn send_host_turn_slash_command_warning(&self, message: &str) {
        self.send_host_turn_slash_command_notice(
            crate::extensions::notification::UiNoticeTone::Warning,
            message,
            None,
        )
        .await;
    }

    async fn send_host_turn_slash_command_error(&self, message: &str, details: impl Into<String>) {
        self.send_host_turn_slash_command_notice(
            crate::extensions::notification::UiNoticeTone::Error,
            message,
            Some(details.into()),
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
