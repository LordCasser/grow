//! Agent business types.
//!
//! Pure data types for agent session management. No UI or rendering logic.
//! The view-model that combines these with UI state is [`super::agent_view::AgentView`].
use crate::acp::meta::NotificationMeta;
use crate::acp::model_state::ModelState;
use crate::acp::tracker::{AcpUpdateTracker, TurnActivity};
use crate::scrollback::EntryId;
use crate::scrollback::state::ScrollbackState;
use acp_transport::AcpAgentTx;
use agent_client_protocol as acp;
use shell::sampling::types::ReasoningEffort;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use tools::implementations::grow_build::workflow::WORKFLOW_TOOL_NAME;
/// Unique local identifier for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AgentId(pub usize);
/// Whether a queue entry is a regular prompt or a slash command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueEntryKind {
    /// Regular user prompt — sent via `PromptRequest`.
    Prompt,
    /// Slash command (e.g., `/compact`) — dispatched as `ExtRequest` or local action.
    Command,
    /// Direct bash command — bypasses agent loop, executed by shell directly.
    BashCommand,
}
impl QueueEntryKind {
    /// Short, stable label for diagnostics / profiling logs.
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Command => "command",
            Self::BashCommand => "bash_command",
        }
    }
}
/// An entry waiting in the queue to be sent to the agent.
///
/// Each entry gets a monotonically increasing `id` for stable tracking
/// (e.g., when editing a queued prompt whose positional index shifts as
/// earlier prompts drain). The user-facing display uses the 1-based
/// positional index (`#1`, `#2`, …), never the internal `id`.
#[derive(Debug, Clone)]
pub struct QueuedPrompt {
    /// Monotonic ID, unique within this agent's session. Never reused.
    pub id: u64,
    /// The prompt text (or command text, e.g. "/compact").
    pub text: String,
    /// Whether this is a prompt or a slash command.
    pub kind: QueueEntryKind,
    /// Optional separate payload for the wire. When `Some`, this is sent
    /// instead of `text`. Used for skill injection where the display
    /// shows `/commit args` but the wire carries the skill XML content.
    pub wire_blocks: Option<Vec<acp::ContentBlock>>,
    /// Images attached to this prompt. Drained from `PromptWidget` at
    /// submission time. Preserved across queue text edits.
    pub images: Vec<crate::prompt_images::PastedImage>,
    /// Whether this prompt should display as a skill invocation (teal accent).
    /// Only meaningful when `wire_blocks` is `Some`.
    pub display_as_skill: bool,
    /// Recognized slash-token byte ranges into `text`, captured from the
    /// composer at submit time; empty = no token styling.
    pub skill_token_ranges: Vec<std::ops::Range<usize>>,
    /// All chip elements captured from the textarea at send time.
    /// Threaded into `InFlightPrompt` so rewind restores collapsed chips.
    pub chip_elements: Vec<ChipElement>,
    /// Combined-turn display segments (len ≥ 2); drain paints one bubble each.
    pub combined_texts: Vec<String>,
}
impl QueuedPrompt {
    /// Base row with every optional field at its default. Sites needing
    /// more use struct-update syntax (`QueuedPrompt { wire_blocks: …,
    /// ..QueuedPrompt::plain(id, text, kind) }`) so adding a field is a
    /// one-site change.
    pub fn plain(id: u64, text: impl Into<String>, kind: QueueEntryKind) -> Self {
        Self {
            id,
            text: text.into(),
            kind,
            wire_blocks: None,
            images: Vec::new(),
            display_as_skill: false,
            skill_token_ranges: Vec::new(),
            chip_elements: Vec::new(),
            combined_texts: Vec::new(),
        }
    }
    /// Whether the wire payload is exactly the display text.
    ///
    /// `true` for plain rows (no `wire_blocks`) and for raw skill slash rows
    /// (`/find-session args` — a single Text block equal to `text`, expanded
    /// shell-side at delivery), so interjecting `text` loses nothing. `false`
    /// when the payload was expanded client-side (for example `/loop`):
    /// interjecting those by `text` would drop the expansion, and by payload
    /// would render the raw instruction.
    pub fn wire_matches_display(&self) -> bool {
        match self.wire_blocks.as_deref() {
            None => true,
            Some([acp::ContentBlock::Text(t)]) => t.text == self.text,
            Some(_) => false,
        }
    }
}
/// A command that is sent to the agent and tracked in the state machine.
///
/// These are distinct from UI-local slash commands (like `/theme`, `/help`)
/// which execute immediately without going through the queue or agent.
///
/// Each variant carries the data needed for execution and display.
/// Using an enum instead of a String gives us:
/// - Type safety (can't misspell command names)
/// - Variant-specific data (e.g., `/model` would carry target model)
/// - Proper rendering per command type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentCommand {
    /// `/compact` — compact conversation history.
    Compact,
    /// Creating a git worktree (from the welcome screen `w` action).
    CreateWorktree,
    /// Resuming a session in a worktree (worktree + code restore).
    RestoreWorktree,
    /// Restoring code in same directory (non-worktree `--restore-code`).
    RestoreCode,
    /// Forking the current session into a peer (no-worktree path).
    /// Drives the spinner shown on the placeholder agent while the
    /// `grow/session/fork` request is in flight.
    ForkSession,
}
impl AgentCommand {
    /// Human-readable label for the status line (e.g., "Compacting").
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Compact => "Compacting",
            Self::CreateWorktree => "Creating worktree",
            Self::RestoreWorktree => "Restoring session in worktree",
            Self::RestoreCode => "Restoring code",
            Self::ForkSession => "Forking session",
        }
    }
    /// The raw command text (e.g., "/compact").
    pub fn command_text(&self) -> &'static str {
        match self {
            Self::Compact => "/compact",
            Self::CreateWorktree => "worktree",
            Self::RestoreWorktree => "worktree",
            Self::RestoreCode => "restore",
            Self::ForkSession => "fork",
        }
    }
}
/// Maximum in-memory stdout per background task (10 MB).
pub const BG_TASK_MAX_STDOUT: usize = 10 * 1024 * 1024;
/// How long to wait for a kill response before auto-clearing `pending_kill`
/// so the user can retry. Applied to both bg tasks and subagents.
pub const PENDING_KILL_TIMEOUT_SECS: u64 = 10;
/// Status of a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgTaskStatus {
    /// Currently running.
    Running,
    /// Completed successfully (exit 0).
    Done,
    /// Failed (non-zero exit, signal, timeout, OOM, etc.).
    Failed,
}
/// Central state for a single background task.
///
/// Stored in `AgentSession::bg_tasks` keyed by `task_id`.
/// Both the scrollback `BgTaskBlock` and the bg task pane read from this.
#[derive(Debug, Clone)]
pub struct BgTaskState {
    pub task_id: String,
    pub tool_call_id: String,
    pub command: String,
    pub description: Option<String>,
    pub cwd: String,
    pub output_file: String,
    pub status: BgTaskStatus,
    pub start_time: SystemTime,
    pub end_time: Option<SystemTime>,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
    /// Accumulated stdout (full cumulative buffer from shell, max BG_TASK_MAX_STDOUT).
    ///
    /// Mutate via [`BgTaskState::set_stdout`] / [`BgTaskState::append_stdout`]
    /// so `stdout_line_count` and `truncated` stay in sync.
    pub stdout: String,
    /// Cached `stdout.lines().count()`. Kept in sync by [`Self::set_stdout`]
    /// and [`Self::append_stdout`] so the tasks-pane overlay doesn't have to
    /// memchr-scan up to `BG_TASK_MAX_STDOUT` bytes per visible task per
    /// render frame.
    pub stdout_line_count: usize,
    /// Whether the rolling buffer has dropped data. Either the shell-side
    /// `BashOutput.truncated` flag was set when the chunk arrived, or the
    /// TUI itself trimmed the buffer to stay under `BG_TASK_MAX_STDOUT` (in
    /// `set_stdout` / `append_stdout`). Once `true`, stays `true` — the
    /// real line count is at least `stdout_line_count`, hence the `(N+)`
    /// badge.
    pub truncated: bool,
    /// Kill request sent, awaiting task_completed.
    pub pending_kill: bool,
    /// When the kill request was sent. Used to auto-clear `pending_kill`
    /// after a timeout so the user can retry if the response is lost.
    pub kill_requested_at: Option<Instant>,
    /// Scrollback entry ID for the "Task started" block (for finish_running).
    pub scrollback_entry_id: Option<crate::scrollback::entry::EntryId>,
    /// True when this background task is a monitor (the `monitor` tool). The
    /// tasks pane renders monitors with a blue "Monitor" tag + neutral text
    /// (mirroring scheduled `/loop` rows) instead of bash-highlighting the
    /// command. Set from the `monitor_description` field of the
    /// `TaskBackgrounded` notification.
    pub is_monitor: bool,
    /// True when this task was restored from a `session/load` replay
    /// (`_meta.isReplay`) rather than started live in this client. Restored
    /// tasks are historical context: the tasks pane must not auto-open for
    /// them (on a cold resume they are dead and reconciled away within the
    /// same load; on a warm reconnect they are ambient, not new activity).
    pub restored_from_replay: bool,
}
impl BgTaskState {
    /// Elapsed duration (from start to end, or start to now if running).
    pub fn elapsed(&self) -> Duration {
        let end = self.end_time.unwrap_or_else(SystemTime::now);
        end.duration_since(self.start_time)
            .unwrap_or(Duration::ZERO)
    }

    pub fn elapsed_at(&self, now: SystemTime) -> Duration {
        self.end_time
            .unwrap_or(now)
            .duration_since(self.start_time)
            .unwrap_or(Duration::ZERO)
    }
    /// Replace `stdout` with `new_stdout`.
    ///
    /// If `new_stdout` exceeds `BG_TASK_MAX_STDOUT`, keeps the head (snapped
    /// to the nearest char boundary so UTF-8 stays valid) and sets
    /// `truncated = true` — TUI-side dropping is treated the same as
    /// shell-side dropping for badge purposes. Always refreshes
    /// `stdout_line_count`.
    pub fn set_stdout(&mut self, new_stdout: String) {
        if new_stdout.len() <= BG_TASK_MAX_STDOUT {
            self.stdout = new_stdout;
        } else {
            let end =
                crate::render::line_utils::floor_char_boundary(&new_stdout, BG_TASK_MAX_STDOUT);
            self.stdout = new_stdout[..end].to_string();
            self.truncated = true;
        }
        self.stdout_line_count = self.stdout.lines().count();
    }
    /// Append `chunk` to `stdout`, inserting a `\n` separator first if the
    /// buffer is non-empty.
    ///
    /// If the resulting buffer exceeds `BG_TASK_MAX_STDOUT`, trims the head
    /// (snapped to the next char boundary) and sets `truncated = true`.
    /// Always refreshes `stdout_line_count`.
    pub fn append_stdout(&mut self, chunk: &str) {
        if !self.stdout.is_empty() {
            self.stdout.push('\n');
        }
        self.stdout.push_str(chunk);
        if self.stdout.len() > BG_TASK_MAX_STDOUT {
            let want_start = self.stdout.len() - BG_TASK_MAX_STDOUT;
            let mut start = want_start;
            while start < self.stdout.len() && !self.stdout.is_char_boundary(start) {
                start += 1;
            }
            self.stdout = self.stdout[start..].to_string();
            self.truncated = true;
        }
        self.stdout_line_count = self.stdout.lines().count();
    }
}
/// State for a scheduled (loop) task, displayed in the tasks pane.
#[derive(Debug, Clone)]
pub struct ScheduledTaskInfo {
    pub task_id: String,
    pub prompt: String,
    pub human_schedule: String,
    pub created_at: std::time::Instant,
    pub next_fire_at: Option<String>,
    /// Tag shown in the tasks pane (e.g. "loop", "check").
    pub tag: String,
    pub last_subagent_id: Option<String>,
}
/// Parsed status of the session's long-lived Goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalDisplayStatus {
    Active,
    Paused,
    Blocked,
    UsageLimited,
    BudgetLimited,
    Complete,
}
impl GoalDisplayStatus {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "blocked" => Self::Blocked,
            "usage_limited" => Self::UsageLimited,
            "budget_limited" => Self::BudgetLimited,
            "complete" => Self::Complete,
            _ => return None,
        })
    }

    pub fn stopped_label(&self) -> &'static str {
        match self {
            Self::Paused => "Paused",
            Self::Blocked => "Blocked",
            Self::UsageLimited => "Usage limited",
            Self::Active | Self::BudgetLimited | Self::Complete => "",
        }
    }

    pub fn uses_warning_chip(&self) -> bool {
        matches!(self, Self::Paused | Self::Blocked | Self::UsageLimited)
    }
}
/// Display projection of the durable Goal snapshot.
#[derive(Debug, Clone)]
pub struct GoalDisplayState {
    pub goal_id: String,
    pub objective: String,
    pub status: GoalDisplayStatus,
    pub token_budget: Option<i64>,
    pub tokens_used: i64,
    pub elapsed_ms: u64,
    pub created_at: String,
    pub updated_at: String,
    pub status_message: Option<String>,
    /// Wall-clock instant when this state was last updated from a GoalUpdated
    /// notification. Used to compute local elapsed delta between notifications
    /// so the pager can tick elapsed_ms at render frequency.
    pub received_at: std::time::Instant,
    /// Monotonic floor for the displayed elapsed time, carried across
    /// `GoalUpdated` rebuilds (seeded in `acp_handler` from the prior state).
    /// Without it the timer ticks backward when a notification's authoritative
    /// base is below the value the pager already extrapolated to. See
    /// [`Self::live_elapsed_ms`].
    pub elapsed_floor_ms: u64,
}
/// An explicit Goal-interrupt decision submitted from the Goal panel. Kept so
/// a stuck-turn retry (second Esc/Ctrl+C while `TurnCancelling`) replays the
/// exact same intent instead of falling back to a default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InterruptIntent {
    /// Whether the cancel carries "pause the Goal" (User reason). Only the
    /// Goal panel's "Pause goal" choice sets this.
    pub pause_goal: bool,
    /// Whether running subagents are stopped with the turn.
    pub cancel_subagents: bool,
}

impl GoalDisplayState {
    /// Minimal state for tests that only need a present goal (e.g. occluder
    /// gating); field values are representative, not load-bearing.
    #[cfg(test)]
    pub(crate) fn test_stub() -> Self {
        Self {
            goal_id: "g-test".into(),
            objective: "test goal".into(),
            status: GoalDisplayStatus::Active,
            token_budget: None,
            tokens_used: 0,
            elapsed_ms: 0,
            created_at: "now".into(),
            updated_at: "now".into(),
            status_message: None,
            received_at: std::time::Instant::now(),
            elapsed_floor_ms: 0,
        }
    }
    /// Return elapsed_ms adjusted with local wall-clock delta since the last
    /// GoalUpdated notification. This makes the timer tick smoothly at render
    /// frequency without requiring the shell to emit notifications every second.
    pub fn live_elapsed_ms_at(&self, now: std::time::Instant) -> u64 {
        let live = if self.status == GoalDisplayStatus::Active {
            self.elapsed_ms
                .saturating_add(now.saturating_duration_since(self.received_at).as_millis() as u64)
        } else {
            self.elapsed_ms
        };
        live.max(self.elapsed_floor_ms)
    }

    pub fn live_elapsed_ms(&self) -> u64 {
        self.live_elapsed_ms_at(std::time::Instant::now())
    }
}
/// What the agent is currently doing.
///
/// Enforces mutual exclusivity: the agent is either idle, running a turn,
/// or running a command — never two at once.
#[derive(Debug, Clone, Default)]
pub enum AgentState {
    /// Nothing happening. Queue can drain.
    #[default]
    Idle,
    /// Prompt RPC was sent, but the server has not yet confirmed whether it is
    /// queued or owns the foreground. This is busy/cancellable, but it is not
    /// an inference turn and must not render as an LLM response.
    TurnSubmitting,
    /// A prompt turn is in progress.
    TurnRunning,
    /// A turn cancel has been sent; waiting for PromptResponse.
    TurnCancelling,
    /// A slash command is in flight.
    CommandRunning {
        command: AgentCommand,
        started_at: Instant,
    },
    /// A command cancel has been sent (future use).
    CommandCancelling { command: AgentCommand },
}
impl AgentState {
    /// Nothing is happening — safe to drain queue or start commands.
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
    /// A prompt turn is actively running (not cancelling).
    pub fn is_turn_running(&self) -> bool {
        matches!(self, Self::TurnSubmitting | Self::TurnRunning)
    }
    pub fn is_turn_submitting(&self) -> bool {
        matches!(self, Self::TurnSubmitting)
    }
    /// Whether a durable terminal signal may finalize the current turn.
    ///
    /// Contract (single finalizer, see `turn_completion`): `TurnRunning` and
    /// `TurnCancelling` are terminal; `TurnSubmitting` is NOT — while the
    /// prompt RPC is still in flight the server has not confirmed the
    /// foreground, so a durable `TurnCompleted` cannot be trusted to end the
    /// right turn and is Ignored. Recovery from a lost submission is the
    /// prompt-status watchdog's job (Phase C). The driver's own
    /// `PromptResponse` rail is exempt: it is the RPC's terminal and may
    /// finalize from `TurnSubmitting`.
    pub fn is_terminal_turn(&self) -> bool {
        matches!(self, Self::TurnRunning | Self::TurnCancelling)
    }
    /// Manual `/compact` is in flight (stoppable via session/cancel).
    pub fn is_compact_running(&self) -> bool {
        matches!(
            self,
            Self::CommandRunning {
                command: AgentCommand::Compact,
                ..
            }
        )
    }
    /// Either a turn or command cancel is in progress.
    pub fn is_cancelling(&self) -> bool {
        matches!(self, Self::TurnCancelling | Self::CommandCancelling { .. })
    }
    /// Agent is busy (turn or command) — queue should not drain.
    pub fn is_busy(&self) -> bool {
        !self.is_idle()
    }
    /// The command currently in flight, if any.
    pub fn command_in_flight(&self) -> Option<&AgentCommand> {
        match self {
            Self::CommandRunning { command, .. } | Self::CommandCancelling { command } => {
                Some(command)
            }
            _ => None,
        }
    }
}
/// Extra metadata a late `PromptResponse` contributes for an
/// already-finalized turn (see [`AgentSession::finalized_pr_meta`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct FinalizedPrMeta {
    /// Typed token usage from the late response (`PromptResponse.usage`).
    pub(crate) usage: Option<agent_client_protocol::Usage>,
    /// `_meta.structuredOutput` / `_meta.structuredOutputError` from the late
    /// response: `Ok(value)` when the model produced schema-validated output,
    /// `Err(message)` when the shell reported a structured-output failure.
    pub(crate) structured_output: Option<Result<serde_json::Value, String>>,
    /// `Err` text when the late RPC resolved as an error (the durable rail's
    /// `agent_result` may be coarser or absent).
    pub(crate) error: Option<String>,
}
/// Per-agent business logic (ACP session, models, state).
///
/// External code should use the facade methods (`handle_update`,
/// `start_turn`, `finish_turn`, `turn_activity`) instead of accessing
/// the tracker directly.
pub struct AgentSession {
    pub id: AgentId,
    pub acp_tx: AcpAgentTx,
    pub session_id: Option<acp::SessionId>,
    pub models: ModelState,
    pub state: AgentState,
    pub cwd: PathBuf,
    /// Whether this session is running inside a git worktree.
    pub is_worktree: bool,
    /// `AgentId` of the parent session if this session was created via
    /// `/fork`. Display-only (status bar, future agent picker grouping);
    /// navigation does not consult it -- the session picker is the
    /// source of truth for navigation history.
    pub forked_from: Option<AgentId>,
    /// Prompts waiting to be sent. Drained front-to-back when
    /// `state` becomes [`AgentState::Idle`].
    pub pending_prompts: VecDeque<QueuedPrompt>,
    /// Next monotonic ID for [`QueuedPrompt`].
    pub(crate) next_queue_id: u64,
    /// Canonical permission mode for this session.
    pub(crate) permission_mode: shell::util::config::PermissionMode,
    /// Prompt history for the current session, fetched from ACP
    /// (`grow/prompt_history` scoped via `session_id`). Most-recent-first.
    /// Fetched on session create/load; prompts sent in this session are
    /// additionally front-inserted locally on send.
    pub prompt_history: Vec<String>,
    /// True until the session's startup/load `grow/prompt_history` fetch completes.
    pub prompt_history_loading: bool,
    /// Session is currently replaying historical updates from `session/load`.
    /// Used to suppress live-style redraw/render work until the load completes.
    pub loading_replay: bool,
    /// Last `--restore-code` outcome's `degree`, parsed from
    /// `_meta.codeRestore.degree` (non-worktree path) or `restoreDegree`
    /// (worktree path). Forward-compat hook: the field is set by both
    /// dispatch handlers but no rendering path consumes it yet — the
    /// type-safety anchor for the wire shape lives in
    /// [`crate::app::effects`]'s parser tests + the deserialise tests in
    /// `ResumeSessionInWorktreeResponse`. Adding a rendering consumer is
    /// out of scope for now.
    pub restore_degree: Option<workspace::session::git::RestoreDegree>,
    /// Set when a rate-limit `RetryState::Exhausted` fires, so the subsequent
    /// `TurnFailed` from the RPC error path can be suppressed (the retry
    /// handler already displayed a user-friendly message). Cleared on `finish_turn`.
    pub rate_limited: bool,
    /// Set when a `RetryState::Failed` with `error_type == "encrypted_content_mismatch"`
    /// fires, so the subsequent `TurnFailed` can be suppressed (the retry handler
    /// already displayed a user-friendly message). Cleared on `finish_turn`.
    pub model_incompatible: bool,
    pub(crate) tracker: AcpUpdateTracker,
    /// ACP-advertised slash commands. Seeded from `InitializeResponse.meta`,
    /// updated by `AvailableCommandsUpdate`. The prompt-side registry syncs
    /// when the generation counter changes.
    pub available_commands: Vec<acp::AvailableCommand>,
    /// Generation counter for `available_commands`. Bumped on every update
    /// (even if the list is identical). Prompt-side compares its synced
    /// generation to detect changes.
    ///
    /// - Bootstrap (from connection): starts at 1 so prompt-side (starting at 0)
    ///   triggers an initial sync.
    /// - Test/placeholder: starts at 0 (no initial sync needed).
    pub available_commands_generation: u64,
    /// Names of tools the agent has registered. `None` until the shell
    /// advertises a list via `AvailableCommandsUpdate.meta.tools`.
    /// `Some(_)` enables tool-gating in the slash registry; `None` keeps
    /// every command visible (avoids bootstrap flicker).
    pub available_tools: Option<HashSet<String>>,
    /// Whether a `/model` switch is in flight. Dims the status-bar model name
    /// and holds the queue drain (`maybe_drain_queue`) so a queued prompt isn't
    /// sent on the old harness mid-switch. Cleared on
    /// `SwitchModelComplete`, or by `begin_session_reload` when a reconnect
    /// drops the in-flight RPC — else a lost completion jams the queue forever.
    pub model_switch_pending: bool,
    /// Model the user chose this session via `/model` / the model picker, or
    /// the last successfully applied live remote `ModelChanged` (leader-mode
    /// fan-out). Survives reconnect (`begin_session_reload` does **not** clear
    /// it). History-replay silent-revert of a prior choice is suppressed on the
    /// shell side via `ReconnectState::user_selected_model`; the pager still
    /// applies live remote switches and updates this field to match.
    pub user_model_preference: Option<acp::ModelId>,
    /// `/model X [effort]` issued before the session was ready, applied on SessionCreated.
    pub deferred_model_switch: Option<(acp::ModelId, Option<ReasoningEffort>)>,
    /// Whether the confirmed Behavior is Plan. Derived only from
    /// `CurrentModeUpdate`; tool titles never change it.
    pub(crate) plan_mode_active: bool,
    /// Confirmed user-facing Behavior. Permission policy is tracked separately.
    pub(crate) behavior_mode: tools::types::BehaviorId,
    /// Optimistic Plan projection set immediately by a Behavior selection.
    /// Cleared to `None` when `detect_plan_mode_change()` confirms real state.
    /// Selectors use `plan_mode_pending.unwrap_or(plan_mode_active)` so the UI
    /// remains responsive while waiting for ACP confirmation.
    pub(crate) plan_mode_pending: Option<bool>,
    /// Optimistic Behavior selection awaiting `CurrentModeUpdate`.
    pub(crate) behavior_mode_pending: Option<tools::types::BehaviorId>,
    /// Current phase reported by the plan-mode runtime, when one is active.
    pub(crate) plan_phase: Option<String>,
    /// Session mode to apply once this agent's ACP session exists. Set when
    /// the agent is spawned from the dashboard with `/plan` active (the
    /// session does not exist yet, so the mode can't be sent immediately).
    /// Consumed in the `SessionCreated` / `WorktreeSessionCreated` handlers,
    /// mirroring `AgentSession.deferred_model_switch`.
    pub(crate) deferred_session_mode: Option<tools::types::BehaviorId>,
    /// Central bg task state, keyed by task_id.
    pub bg_tasks: BTreeMap<String, BgTaskState>,
    /// Correlation map: tool_call_id → task_id.
    /// Used to route stdout chunks (which arrive keyed by tool_call_id) to the
    /// correct bg task in `bg_tasks`.
    pub bg_tool_call_to_task: HashMap<String, String>,
    /// Active scheduled tasks, keyed by task_id.
    pub scheduled_tasks: HashMap<String, ScheduledTaskInfo>,
    /// Plain-text prompt currently in flight, captured at send time and
    /// cleared as soon as the server emits any activity (chunk, tool call,
    /// retry, etc.). Used by `do_cancel_turn` to "rewind" a prompt back to
    /// the input box if the user cancels before any response arrives.
    /// `None` for skill-injected prompts (cannot be reversed) and bash/cron.
    pub in_flight_prompt: Option<InFlightPrompt>,
    /// `in_flight_prompt` is cleared on compact start so cancel cannot rewind.
    pub compact_held_prompt: Option<InFlightPrompt>,
    /// Stable id for the prompt currently in flight. Generated client-side
    /// at `Effect::SendPrompt` time and threaded through `PromptRequest._meta`
    /// to the agent, which echoes it back on every `SessionNotification` and
    /// `PromptResponse` it produces for that prompt.
    ///
    /// The acp_handler uses this to discriminate chunks for the active turn
    /// from chunks belonging to a turn the user already rewound: any update
    /// whose `meta.promptId` is set and doesn't match this id is silently
    /// dropped. `None` between turns.
    pub current_prompt_id: Option<String>,
    /// Per-agent mirror of the server-authoritative shared prompt queue
    /// (`AppView::shared_prompt_queues[sid]`), kept in sync by
    /// `handle_queue_changed` and the immediate-send path. The queue pane
    /// renders the union of this and the local `pending_prompts`; edit handlers
    /// read it to route remove/reorder by origin. Empty unless a plain prompt
    /// was queued server-side while a turn was running.
    pub(crate) shared_queue: Vec<crate::app::prompt_queue::QueueEntryWire>,
    /// True when this session was opened via `session/load` (session picker
    /// resume, `/resume`, or a leader dashboard roster attach) rather than
    /// created locally — i.e. this client is *viewing* a session it did not
    /// start. While set, the ACP gate adopts the prompt id of incoming live
    /// `session/update` deltas (the driver's turn) instead of dropping them,
    /// so the viewer renders the in-flight (and subsequent) turns live. This
    /// must NOT be applied to a locally-created driver, whose post-rewind
    /// stale-chunk drop semantics rely on the strict prompt-id match. Cleared
    /// in `maybe_drain_queue` the moment this client sends its own prompt
    /// ("takes the wheel"), and re-derived per turn from
    /// [`Self::self_originated_prompt_ids`] in the ACP gate / turn-start shim:
    /// a client that has driven a turn can still go on to VIEW a turn another
    /// client drives (e.g. a `/loop` cron, or a plain prompt typed in another
    /// pane), so this flag is no longer a one-way latch.
    pub(crate) attached_as_viewer: bool,
    /// Prompt ids of turns THIS client originated (sent to the agent as the
    /// turn driver). The ACP gate consults this to keep `attached_as_viewer`
    /// per-turn accurate: a prompt id present here is this client's own turn
    /// (drive it — and drop a stale post-rewind chunk on a mismatch), while one
    /// that is absent is another client's (or a server-initiated) turn (adopt +
    /// render it as a viewer). Without this, the flag latched false after the
    /// first local prompt and the gate dropped every later turn a different
    /// pane drove. Bounded FIFO — only recent ids matter (a stale chunk arrives
    /// right after its turn ends).
    pub(crate) self_originated_prompt_ids: VecDeque<String>,
    /// Prompt ids rewound by this client, bounded to recent turns.
    pub(crate) rewound_prompt_ids: VecDeque<String>,
    /// Highwater of the largest `eventId` counter applied to this session's
    /// scrollback (see `acp::meta::NotificationMeta::event_seq`). Incoming
    /// `session/update`s with a counter `<=` this are duplicates (replay/live
    /// overlap, a re-emit after the reconnect gate, or duplicate routing) and
    /// are dropped so each event renders exactly once. `None` until the first
    /// `eventId`-bearing update.
    ///
    /// ACP stream only — the Grow stream keeps its own highwater
    /// ([`Self::last_applied_grow_event_seq`]) because the two streams are not
    /// delivered in one id order: ACP lines ride the agent's FIFO event
    /// pipeline while Grow lines are emitted direct-to-gateway, so a fresh Grow
    /// id arriving ahead of queued lower-id ACP chunks must not make the chunks
    /// look stale (silent live-text loss).
    pub(crate) last_applied_event_seq: Option<u64>,
    /// Grow-stream sibling of [`Self::last_applied_event_seq`] (see there for
    /// why the highwaters are split). Same drop rule, replay-exempt.
    pub(crate) last_applied_grow_event_seq: Option<u64>,
    /// Raw `eventId` of the most recent update APPLIED to this root session —
    /// replay or live, on both the ACP and Grow paths; dropped updates (dedup,
    /// promptId gate, unexpected replay) don't move it. Sent as `_meta.cursor`
    /// on a reconnect `session/load` so the agent replays only the post-cursor
    /// tail. Why the full string: see
    /// [`crate::acp::meta::NotificationMeta::event_id`].
    pub(crate) last_seen_event_id: Option<String>,
    /// Unexpected-replay drops since the last reload window opened. Gates the
    /// drop log to one `warn!` per incident (a late replay is one line per
    /// event — thousands for a large transcript).
    pub(crate) unexpected_replay_drops: u32,
    /// Prompt ids whose durable `TurnCompleted` terminal arrived during THIS
    /// load's replay window (`loading_replay`). The running turn is not adopted
    /// until replay finishes, so a terminal seen mid-replay can't be finalized
    /// yet — it is recorded here and consulted by
    /// `AgentView::should_adopt_running_prompt` so the post-replay adoption
    /// skips a turn that already ended (otherwise the viewer re-strands on
    /// "Waiting…"). Reset at the start of every load so it never leaks across
    /// loads.
    pub(crate) replayed_terminal_prompts: HashSet<String>,
    /// Prompt id of the turn THIS client's terminal finalizer already
    /// committed (the first-wins winner). A late `PromptResponse` whose pid
    /// matches only merges its extra metadata ([`Self::finalized_pr_meta`]) —
    /// it must not finish the turn, push a second marker, drain the queue,
    /// or re-run the adoption handoff (all of that ran exactly once when the
    /// finalizer won). Cleared at the next real turn start
    /// (`AgentView::start_turn_boundary`) and at every replay-window entry, so
    /// a stale pid can never merge into a newer turn.
    pub(crate) finalized_prompt: Option<String>,
    /// Extra metadata a late `PromptResponse` contributed for an
    /// already-finalized turn ([`Self::finalized_prompt`] matched): the
    /// durable rail's terminal cannot carry token usage, structured output,
    /// or the RPC's own error text, so the late response merges them here.
    /// There is no live consumer yet — this is the retention point that makes
    /// the late-PR merge observable and keeps the data from being dropped by
    /// the race.
    pub(crate) finalized_pr_meta: Option<FinalizedPrMeta>,
    /// Whether this session was created via the `/new` slash command.
    /// Checked in the `SessionCreated` handler to decide whether to show
    /// the `/agents` discoverability tip. `false` for sessions created
    /// by `/resume`, welcome-screen picker, `/fork`, or worktree flows.
    pub created_via_new: bool,
}
/// Captured state for a prompt that has been sent but not yet acknowledged
/// by any server activity. See `AgentSession::in_flight_prompt`.
#[derive(Debug, Clone)]
pub struct InFlightPrompt {
    pub text: String,
    pub images: Vec<crate::prompt_images::PastedImage>,
    /// Primary (last) user-prompt block for restore/cancel.
    pub scrollback_entry: EntryId,
    /// Earlier segment blocks for a combined multi-bubble turn (oldest first).
    pub combined_scrollback_entries: Vec<EntryId>,
    /// All chip elements (paste blocks, @-file refs, image chips) that were
    /// active in the textarea at send time. Restored on rewind so collapsed
    /// chips render correctly instead of raw text.
    pub chip_elements: Vec<ChipElement>,
}
/// Snapshot of a textarea chip element for rewind restore.
/// Covers paste blocks, @-file refs, and image chips.
#[derive(Debug, Clone)]
pub struct ChipElement {
    pub range: std::ops::Range<usize>,
    pub kind: ratatui_textarea::ElementKind,
    pub display: Option<ratatui::text::Line<'static>>,
}
/// Names of Shell-owned Workflow runtime commands used as capability signals.
/// Public management is Behavior-gated, while private Deep Research remains a
/// stable bootstrap signal that the workflow runtime is configured.
const WORKFLOW_RUN_COMMAND_NAME: &str = "workflow-run";
const DEEP_RESEARCH_COMMAND_NAME: &str = "deep-research";
impl AgentSession {
    /// Construct a session with the state shared by every lifecycle entry point.
    ///
    /// Lifecycle-specific facts (fork ancestry, replay, worktree membership,
    /// and bootstrap metadata) are applied by the named methods or direct
    /// domain updates at the call site after construction.
    pub(crate) fn new(
        id: AgentId,
        acp_tx: AcpAgentTx,
        session_id: Option<acp::SessionId>,
        models: ModelState,
        cwd: PathBuf,
        permission_mode: shell::util::config::PermissionMode,
    ) -> Self {
        Self {
            id,
            acp_tx,
            session_id,
            models,
            state: AgentState::Idle,
            cwd,
            is_worktree: false,
            forked_from: None,
            pending_prompts: VecDeque::new(),
            next_queue_id: 0,
            permission_mode,
            prompt_history: Vec::new(),
            prompt_history_loading: false,
            loading_replay: false,
            restore_degree: None,
            rate_limited: false,
            model_incompatible: false,
            tracker: AcpUpdateTracker::new(),
            available_commands: Vec::new(),
            available_commands_generation: 0,
            available_tools: None,
            model_switch_pending: false,
            user_model_preference: None,
            deferred_model_switch: None,
            plan_mode_active: false,
            behavior_mode: tools::types::BehaviorId::Normal,
            plan_mode_pending: None,
            behavior_mode_pending: None,
            plan_phase: None,
            deferred_session_mode: None,
            bg_tasks: BTreeMap::new(),
            bg_tool_call_to_task: HashMap::new(),
            scheduled_tasks: HashMap::new(),
            in_flight_prompt: None,
            compact_held_prompt: None,
            current_prompt_id: None,
            shared_queue: Vec::new(),
            attached_as_viewer: false,
            self_originated_prompt_ids: VecDeque::new(),
            rewound_prompt_ids: VecDeque::new(),
            last_applied_event_seq: None,
            last_applied_grow_event_seq: None,
            last_seen_event_id: None,
            unexpected_replay_drops: 0,
            replayed_terminal_prompts: HashSet::new(),
            finalized_prompt: None,
            finalized_pr_meta: None,
            created_via_new: false,
        }
    }

    pub(crate) fn mark_forked_from(&mut self, parent_id: AgentId) {
        self.forked_from = Some(parent_id);
    }

    pub(crate) fn set_worktree(&mut self, is_worktree: bool) {
        self.is_worktree = is_worktree;
    }

    pub(crate) fn begin_replay(&mut self) {
        self.prompt_history_loading = true;
        self.loading_replay = true;
    }

    pub(crate) fn mark_created_via_new(&mut self) {
        self.created_via_new = true;
    }

    pub fn permission_mode(&self) -> shell::util::config::PermissionMode {
        self.permission_mode
    }
    /// Whether always-approve is active.
    pub fn is_always_approve(&self) -> bool {
        self.permission_mode.is_always_approve()
    }
    /// Whether Auto (LLM classifier) mode is active. Prefer this over direct
    /// field access. Mutually exclusive with `is_always_approve()` (always-approve wins).
    pub fn is_auto(&self) -> bool {
        self.permission_mode.is_auto()
    }
    /// Test-only setter for the canonical session mode.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_permission_mode_for_test(
        &mut self,
        mode: shell::util::config::PermissionMode,
    ) {
        self.permission_mode = mode;
    }
    /// Bootstrap-only Workflow support heuristic used before the Shell's first
    /// structured `BehaviorAvailability` projection arrives.
    ///
    /// Signals (any true → available):
    /// 1. `available_tools` is `Some(_)` and contains the `workflow` tool.
    /// 2. A Workflow runtime slash command is advertised. During bootstrap
    ///    (`available_tools` is still `None`) private `deep-research` is the
    ///    stable signal; public `workflow-run` appears only in Workflow Behavior.
    /// 3. `has_workflow_runs` — workflows stay selectable while a run is known
    ///    to the pager (running or history).
    ///
    /// Callers must stop consulting these signals as soon as the structured
    /// projection exists;
    /// [`AgentView::behavior_supported`](crate::app::agent_view::AgentView::behavior_supported)
    /// owns that switch.
    pub(crate) fn bootstrap_workflow_support(
        available_tools: Option<&HashSet<String>>,
        available_commands: &[acp::AvailableCommand],
        has_workflow_runs: bool,
    ) -> bool {
        let has_workflow_tool =
            available_tools.is_some_and(|tools| tools.contains(WORKFLOW_TOOL_NAME));
        let has_workflow_command = available_commands.iter().any(|c| {
            c.name == WORKFLOW_TOOL_NAME
                || c.name == WORKFLOW_RUN_COMMAND_NAME
                || c.name == DEEP_RESEARCH_COMMAND_NAME
        });
        has_workflow_tool || has_workflow_command || has_workflow_runs
    }
    /// Process an ACP session update. Returns true if scrollback was modified.
    pub fn handle_update(
        &mut self,
        update: acp::SessionUpdate,
        meta: &NotificationMeta,
        scrollback: &mut ScrollbackState,
    ) -> bool {
        self.tracker.set_session_cwd(&self.cwd);
        self.tracker.handle_update(update, meta, scrollback)
    }
    /// Start a new turn: set state to TurnRunning, prepare tracker.
    ///
    /// Called by `maybe_drain_queue` when a prompt is being sent.
    pub fn start_turn(&mut self, scrollback: &mut ScrollbackState) {
        self.tracker.finish_turn(scrollback);
        self.compact_held_prompt = None;
        self.tracker.set_session_cwd(&self.cwd);
        self.state = AgentState::TurnRunning;
        self.in_flight_prompt = None;
    }
    /// Finish the current turn: cleanup tracker, set state to Idle.
    ///
    /// Called when `PromptResponse` is received.
    pub fn finish_turn(&mut self, scrollback: &mut ScrollbackState) {
        self.tracker.finish_turn(scrollback);
        self.state = AgentState::Idle;
        self.rate_limited = false;
        self.model_incompatible = false;
        self.in_flight_prompt = None;
        self.compact_held_prompt = None;
        self.current_prompt_id = None;
    }
    /// Whether any background task is still running (vs. completed/failed).
    /// Used to defer the automatic away-recap: a running task can wake the
    /// agent (auto-wake on completion), so we don't pre-generate a recap while
    /// one is live and could change the session out from under it.
    pub fn has_running_bg_tasks(&self) -> bool {
        self.bg_tasks
            .values()
            .any(|t| t.status == BgTaskStatus::Running)
    }
    /// Begin cancellation without committing a terminal transition.
    ///
    /// Streaming content and tool activity remain owned by the prompt until an
    /// exact-id PromptResponse or durable TurnCompleted wins finalization. This
    /// prevents cancellation intent from closing the tracker before the
    /// authoritative terminal (and from closing it a second time when that
    /// terminal arrives).
    pub fn cancel_turn(&mut self, _scrollback: &mut ScrollbackState) {
        if self.state.is_turn_running() {
            self.state = AgentState::TurnCancelling;
        }
    }
    /// Current activity within a running turn (for turn status line display).
    ///
    /// Returns `None` when not in `TurnRunning` state.
    pub fn turn_activity(&self) -> Option<TurnActivity> {
        if matches!(self.state, AgentState::TurnRunning) {
            self.tracker.activity()
        } else {
            None
        }
    }
    /// Set a compaction-related activity override on the tracker.
    ///
    /// Called from ACP handler when compaction ExtNotifications arrive.
    pub fn set_compaction_activity(&mut self, activity: Option<TurnActivity>) {
        self.tracker.set_compaction_activity(activity);
    }
    pub fn defer_compaction(
        &mut self,
        tokens_before: u64,
        estimate_after: u64,
        elapsed_ms: Option<i64>,
    ) {
        self.tracker
            .defer_compaction(tokens_before, estimate_after, elapsed_ms);
    }
    pub fn note_context_used(&mut self, used: u64) {
        self.tracker.note_context_used(used);
    }
    /// Set a retry-related activity override on the tracker.
    ///
    /// Called from ACP handler when `RetryState::Retrying` arrives.
    /// Auto-cleared when normal streaming data resumes.
    pub fn set_retry_activity(&mut self, activity: Option<TurnActivity>) {
        self.tracker.set_retry_activity(activity);
    }
    /// Start a slash command (e.g., /compact).
    pub fn start_command(&mut self, command: AgentCommand) {
        self.state = AgentState::CommandRunning {
            command,
            started_at: Instant::now(),
        };
    }
    /// Finish a running command, return to Idle.
    pub fn finish_command(&mut self) {
        self.state = AgentState::Idle;
    }
    /// Mark an in-flight `/compact` as cancelling (waiting for CompactComplete).
    pub fn cancel_compact_command(&mut self) {
        if let AgentState::CommandRunning {
            command: AgentCommand::Compact,
            ..
        } = &self.state
        {
            self.state = AgentState::CommandCancelling {
                command: AgentCommand::Compact,
            };
        }
    }
    /// Push a prompt onto the back of the queue. Returns the assigned ID.
    pub fn enqueue_prompt(&mut self, text: String) -> u64 {
        self.enqueue_entry(text, QueueEntryKind::Prompt)
    }
    /// Push a plain prompt carrying the composer's recognized slash-token
    /// ranges (mid-text skill highlighting in the scrollback echo).
    pub fn enqueue_prompt_with_skill_tokens(
        &mut self,
        text: String,
        skill_token_ranges: Vec<std::ops::Range<usize>>,
    ) -> u64 {
        self.enqueue_entry_at(text, QueueEntryKind::Prompt, false, skill_token_ranges)
    }
    /// Push a prompt onto the **front** of the queue. Returns the assigned ID.
    ///
    /// Sibling of [`enqueue_prompt`](Self::enqueue_prompt) -- same defaults,
    /// but `push_front` instead of `push_back`. Used by the `/fork` flow to
    /// inject the user's directive ahead of any prompts the user typed
    /// during the placeholder window so the directive runs first.
    pub fn enqueue_prompt_front(&mut self, text: String) -> u64 {
        self.enqueue_entry_at(text, QueueEntryKind::Prompt, true, Vec::new())
    }
    /// Requeue a failed plain prompt without dropping its attachments.
    pub fn enqueue_in_flight_prompt_front(&mut self, prompt: InFlightPrompt) -> u64 {
        let id = self.next_queue_id;
        self.next_queue_id += 1;
        self.pending_prompts.push_front(QueuedPrompt {
            images: prompt.images,
            chip_elements: prompt.chip_elements,
            ..QueuedPrompt::plain(id, prompt.text, QueueEntryKind::Prompt)
        });
        id
    }
    /// Push a slash command onto the back of the queue. Returns the assigned ID.
    pub fn enqueue_command(&mut self, text: String) -> u64 {
        self.enqueue_entry(text, QueueEntryKind::Command)
    }
    /// Push a direct bash command onto the back of the queue. Returns the assigned ID.
    pub fn enqueue_bash_command(&mut self, text: String) -> u64 {
        self.enqueue_entry(text, QueueEntryKind::BashCommand)
    }
    /// Push an entry with the given kind onto the back of the queue.
    pub fn enqueue_entry(&mut self, text: String, kind: QueueEntryKind) -> u64 {
        self.enqueue_entry_at(text, kind, false, Vec::new())
    }
    /// Internal: push an entry with the given kind onto the front (`front == true`)
    /// or back (`front == false`) of the queue. Single source of truth for the
    /// `QueuedPrompt` defaults shared by `enqueue_entry` and `enqueue_prompt_front`.
    fn enqueue_entry_at(
        &mut self,
        text: String,
        kind: QueueEntryKind,
        front: bool,
        skill_token_ranges: Vec<std::ops::Range<usize>>,
    ) -> u64 {
        let id = self.next_queue_id;
        self.next_queue_id += 1;
        let entry = QueuedPrompt {
            skill_token_ranges,
            ..QueuedPrompt::plain(id, text, kind)
        };
        if front {
            self.pending_prompts.push_front(entry);
        } else {
            self.pending_prompts.push_back(entry);
        }
        id
    }
    /// Pop the front prompt from the queue (next to send).
    pub fn dequeue_prompt(&mut self) -> Option<QueuedPrompt> {
        self.pending_prompts.pop_front()
    }
    /// Pop the front entry, merging consecutive plain `Prompt` followers via
    /// [`prompt_queue::combine_prefix_len`]. `editing_id` is held out of the
    /// merge (composer draft must not vanish). Front may keep images.
    pub fn dequeue_combined_prompt(&mut self, editing_id: Option<u64>) -> Option<QueuedPrompt> {
        use prompt_queue::{CombineGate, TEXT_SEPARATOR, combine_prefix_len, join_texts};
        if self.pending_prompts.is_empty() {
            return None;
        }
        let skip_id = editing_id.map(|id| id.to_string());
        let skip_refs: Vec<&str> = skip_id.iter().map(String::as_str).collect();
        let id_strings: Vec<String> = self
            .pending_prompts
            .iter()
            .map(|p| p.id.to_string())
            .collect();
        let gates: Vec<CombineGate<'_>> = self
            .pending_prompts
            .iter()
            .zip(id_strings.iter())
            .map(|(p, id)| CombineGate {
                id: id.as_str(),
                is_plain_prompt: p.kind == QueueEntryKind::Prompt,
                is_synthetic: false,
                is_expanded_skill: !p.wire_matches_display(),
                is_bash: p.kind == QueueEntryKind::BashCommand,
                has_images: !p.images.is_empty(),
                text: p.text.as_str(),
            })
            .collect();
        let n = combine_prefix_len(gates, &skip_refs).max(1);
        let mut merged = self.pending_prompts.pop_front()?;
        if n == 1 {
            return Some(merged);
        }
        let mut segments = vec![merged.text.clone()];
        for _ in 1..n {
            let next = self
                .pending_prompts
                .pop_front()
                .expect("prefix length checked");
            let shift =
                join_texts(segments.iter().map(String::as_str)).len() + TEXT_SEPARATOR.len();
            segments.push(next.text.clone());
            merged
                .chip_elements
                .extend(next.chip_elements.into_iter().map(|c| ChipElement {
                    range: (c.range.start + shift)..(c.range.end + shift),
                    kind: c.kind,
                    display: c.display,
                }));
            merged.wire_blocks = None;
            merged.display_as_skill = false;
        }
        merged.text = join_texts(segments.iter().map(String::as_str));
        merged.skill_token_ranges.clear();
        merged.combined_texts = segments;
        Some(merged)
    }
    /// Number of prompts currently queued.
    pub fn queue_len(&self) -> usize {
        self.pending_prompts.len()
    }
    /// Find the 0-based positional index of a prompt by its stable ID.
    pub fn queue_position(&self, id: u64) -> Option<usize> {
        self.pending_prompts.iter().position(|p| p.id == id)
    }
    /// Swap a prompt with its neighbor above (toward front of queue).
    pub fn swap_prompt_up(&mut self, id: u64) {
        if let Some(pos) = self.queue_position(id)
            && pos > 0
        {
            self.pending_prompts.swap(pos, pos - 1);
        }
    }
    /// Swap a prompt with its neighbor below (toward back of queue).
    pub fn swap_prompt_down(&mut self, id: u64) {
        if let Some(pos) = self.queue_position(id)
            && pos + 1 < self.pending_prompts.len()
        {
            self.pending_prompts.swap(pos, pos + 1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_session() -> AgentSession {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        AgentSession::new(
            AgentId(0),
            tx,
            None,
            ModelState::default(),
            PathBuf::from("/tmp"),
            shell::util::config::PermissionMode::Ask,
        )
    }
    #[test]
    fn workflows_available_true_when_workflow_tool_advertised() {
        // (a) Tool-level signal: `AvailableCommandsUpdate.meta.tools` contains
        // the `workflow` tool, with no advertised commands or runs.
        let tools = HashSet::from([WORKFLOW_TOOL_NAME.to_string()]);
        assert!(AgentSession::bootstrap_workflow_support(
            Some(&tools),
            &[],
            false
        ));
    }

    #[test]
    fn workflows_available_true_via_behavior_gated_workflow_run_command() {
        // A live Workflow Behavior snapshot advertises its management command.
        let cmds = [acp::AvailableCommand::new(
            WORKFLOW_RUN_COMMAND_NAME.to_string(),
            "run a workflow".to_string(),
        )];
        assert!(AgentSession::bootstrap_workflow_support(None, &cmds, false));
    }

    #[test]
    fn workflows_available_true_via_private_runtime_bootstrap_signal() {
        let cmds = [acp::AvailableCommand::new(
            DEEP_RESEARCH_COMMAND_NAME.to_string(),
            "private research runtime".to_string(),
        )];
        assert!(AgentSession::bootstrap_workflow_support(None, &cmds, false));
    }

    #[test]
    fn workflows_available_true_via_workflow_command() {
        // The literal `workflow` command name also counts (previously the only
        // pager-side signal alongside runs).
        let cmds = [acp::AvailableCommand::new(
            WORKFLOW_TOOL_NAME.to_string(),
            "workflows".to_string(),
        )];
        assert!(AgentSession::bootstrap_workflow_support(
            Some(&HashSet::new()),
            &cmds,
            false
        ));
    }

    #[test]
    fn workflows_available_false_without_any_signal() {
        // (c) No tool, no command, no runs → unavailable. Covers both the
        // not-yet-received (None) and empty-toolset (Some(&[])) cases.
        assert!(!AgentSession::bootstrap_workflow_support(None, &[], false));
        assert!(!AgentSession::bootstrap_workflow_support(
            Some(&HashSet::new()),
            &[],
            false
        ));
    }

    #[test]
    fn workflows_available_true_when_workflow_runs_exist() {
        // (d) Runs-only signal: workflows stay selectable while a run exists,
        // regardless of tool/command advertisement.
        assert!(AgentSession::bootstrap_workflow_support(None, &[], true));
    }

    #[test]
    fn goal_display_status_parse_known_values() {
        assert_eq!(
            GoalDisplayStatus::parse("active"),
            Some(GoalDisplayStatus::Active)
        );
        assert_eq!(
            GoalDisplayStatus::parse("paused"),
            Some(GoalDisplayStatus::Paused)
        );
        assert_eq!(
            GoalDisplayStatus::parse("blocked"),
            Some(GoalDisplayStatus::Blocked)
        );
        assert_eq!(
            GoalDisplayStatus::parse("budget_limited"),
            Some(GoalDisplayStatus::BudgetLimited)
        );
        assert_eq!(
            GoalDisplayStatus::parse("complete"),
            Some(GoalDisplayStatus::Complete)
        );
        assert_eq!(GoalDisplayStatus::parse("user_paused"), None);
        assert_eq!(GoalDisplayStatus::parse("unknown"), None);
    }

    #[test]
    fn stopped_label_is_consistent_across_renderers() {
        assert_eq!(GoalDisplayStatus::Paused.stopped_label(), "Paused");
        assert_eq!(GoalDisplayStatus::Blocked.stopped_label(), "Blocked");
        assert_eq!(
            GoalDisplayStatus::UsageLimited.stopped_label(),
            "Usage limited"
        );
        assert_eq!(GoalDisplayStatus::Active.stopped_label(), "");
        assert_eq!(GoalDisplayStatus::BudgetLimited.stopped_label(), "");
        assert_eq!(GoalDisplayStatus::Complete.stopped_label(), "");
        assert!(GoalDisplayStatus::Paused.uses_warning_chip());
        assert!(GoalDisplayStatus::Blocked.uses_warning_chip());
        assert!(GoalDisplayStatus::UsageLimited.uses_warning_chip());
        assert!(!GoalDisplayStatus::Active.uses_warning_chip());
        assert!(!GoalDisplayStatus::BudgetLimited.uses_warning_chip());
        assert!(!GoalDisplayStatus::Complete.uses_warning_chip());
    }
    #[test]
    fn enqueue_assigns_monotonic_ids() {
        let mut s = test_session();
        let id0 = s.enqueue_prompt("first".into());
        let id1 = s.enqueue_prompt("second".into());
        let id2 = s.enqueue_prompt("third".into());
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(s.queue_len(), 3);
    }
    #[test]
    fn dequeue_returns_fifo_order() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.enqueue_prompt("second".into());
        let p = s.dequeue_prompt().unwrap();
        assert_eq!(p.text, "first");
        assert_eq!(p.id, 0);
        let p = s.dequeue_prompt().unwrap();
        assert_eq!(p.text, "second");
        assert_eq!(p.id, 1);
        assert!(s.dequeue_prompt().is_none());
    }
    #[test]
    fn queue_position_tracks_by_id() {
        let mut s = test_session();
        let _id0 = s.enqueue_prompt("a".into());
        let id1 = s.enqueue_prompt("b".into());
        let id2 = s.enqueue_prompt("c".into());
        assert_eq!(s.queue_position(id1), Some(1));
        assert_eq!(s.queue_position(id2), Some(2));
        s.dequeue_prompt();
        assert_eq!(s.queue_position(id1), Some(0));
        assert_eq!(s.queue_position(id2), Some(1));
    }
    #[test]
    fn queue_position_returns_none_for_drained() {
        let mut s = test_session();
        let id0 = s.enqueue_prompt("gone".into());
        s.dequeue_prompt();
        assert_eq!(s.queue_position(id0), None);
    }
    #[test]
    fn swap_prompt_up() {
        let mut s = test_session();
        let id_a = s.enqueue_prompt("a".into());
        let id_b = s.enqueue_prompt("b".into());
        let id_c = s.enqueue_prompt("c".into());
        s.swap_prompt_up(id_b);
        assert_eq!(s.pending_prompts[0].id, id_b);
        assert_eq!(s.pending_prompts[1].id, id_a);
        assert_eq!(s.pending_prompts[2].id, id_c);
        s.swap_prompt_up(id_b);
        assert_eq!(s.pending_prompts[0].id, id_b);
    }
    #[test]
    fn swap_prompt_down() {
        let mut s = test_session();
        let id_a = s.enqueue_prompt("a".into());
        let id_b = s.enqueue_prompt("b".into());
        let id_c = s.enqueue_prompt("c".into());
        s.swap_prompt_down(id_b);
        assert_eq!(s.pending_prompts[0].id, id_a);
        assert_eq!(s.pending_prompts[1].id, id_c);
        assert_eq!(s.pending_prompts[2].id, id_b);
        s.swap_prompt_down(id_b);
        assert_eq!(s.pending_prompts[2].id, id_b);
    }
    #[test]
    fn ids_never_reuse_after_drain() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.dequeue_prompt();
        let id = s.enqueue_prompt("second".into());
        assert_eq!(id, 1);
    }
    #[test]
    fn enqueue_bash_command_stores_bash_kind() {
        let mut s = test_session();
        s.enqueue_bash_command("ls -la".into());
        assert_eq!(s.queue_len(), 1);
        let entry = s.dequeue_prompt().unwrap();
        assert_eq!(entry.text, "ls -la");
        assert_eq!(entry.kind, QueueEntryKind::BashCommand);
    }
    #[test]
    fn mixed_queue_drains_fifo_across_kinds() {
        let mut s = test_session();
        s.enqueue_prompt("prompt1".into());
        s.enqueue_bash_command("echo hi".into());
        s.enqueue_command("/compact".into());
        s.enqueue_bash_command("pwd".into());
        let e1 = s.dequeue_prompt().unwrap();
        assert_eq!(e1.kind, QueueEntryKind::Prompt);
        assert_eq!(e1.text, "prompt1");
        let e2 = s.dequeue_prompt().unwrap();
        assert_eq!(e2.kind, QueueEntryKind::BashCommand);
        assert_eq!(e2.text, "echo hi");
        let e3 = s.dequeue_prompt().unwrap();
        assert_eq!(e3.kind, QueueEntryKind::Command);
        assert_eq!(e3.text, "/compact");
        let e4 = s.dequeue_prompt().unwrap();
        assert_eq!(e4.kind, QueueEntryKind::BashCommand);
        assert_eq!(e4.text, "pwd");
        assert!(s.dequeue_prompt().is_none());
    }
    #[test]
    fn swap_works_across_entry_kinds() {
        let mut s = test_session();
        let id_p = s.enqueue_prompt("prompt".into());
        let id_b = s.enqueue_bash_command("ls".into());
        s.swap_prompt_up(id_b);
        assert_eq!(s.pending_prompts[0].id, id_b);
        assert_eq!(s.pending_prompts[0].kind, QueueEntryKind::BashCommand);
        assert_eq!(s.pending_prompts[1].id, id_p);
        assert_eq!(s.pending_prompts[1].kind, QueueEntryKind::Prompt);
    }
    /// `wire_matches_display` splits interjectable rows (no payload, or a raw
    /// skill slash payload equal to the display text) from client-expanded
    /// expanded payloads (such as `/loop`) that must run as their own turn.
    #[test]
    fn wire_matches_display_classifies_payload_shapes() {
        let text_block = |t: &str| acp::ContentBlock::Text(acp::TextContent::new(t.to_string()));
        let plain = QueuedPrompt::plain(1, "hello", QueueEntryKind::Prompt);
        assert!(plain.wire_matches_display(), "no payload = display");
        let raw_skill = QueuedPrompt {
            wire_blocks: Some(vec![text_block("/commit fix")]),
            ..QueuedPrompt::plain(2, "/commit fix", QueueEntryKind::Prompt)
        };
        assert!(raw_skill.wire_matches_display(), "raw slash payload");
        let expanded = QueuedPrompt {
            wire_blocks: Some(vec![text_block("<skill>body</skill>")]),
            ..QueuedPrompt::plain(3, "/loop check status", QueueEntryKind::Prompt)
        };
        assert!(!expanded.wire_matches_display(), "expanded payload");
        let multi_block = QueuedPrompt {
            wire_blocks: Some(vec![text_block("/commit fix"), text_block("more")]),
            ..QueuedPrompt::plain(4, "/commit fix", QueueEntryKind::Prompt)
        };
        assert!(!multi_block.wire_matches_display(), "multi-block payload");
    }
    #[test]
    fn enqueue_prompt_wire_blocks_defaults_to_none() {
        let mut s = test_session();
        s.enqueue_prompt("hello".into());
        let p = s.dequeue_prompt().unwrap();
        assert!(p.wire_blocks.is_none());
        assert!(p.skill_token_ranges.is_empty());
    }
    #[test]
    fn enqueue_prompt_with_skill_tokens_preserves_ranges() {
        let mut s = test_session();
        s.enqueue_prompt_with_skill_tokens("great /commit now".into(), vec![6..13]);
        let p = s.dequeue_prompt().unwrap();
        assert_eq!(p.skill_token_ranges, vec![6..13]);
        assert_eq!(p.kind, QueueEntryKind::Prompt);
        assert!(p.wire_blocks.is_none());
    }
    #[test]
    fn enqueue_prompt_front_into_empty_queue() {
        let mut s = test_session();
        let id = s.enqueue_prompt_front("first".into());
        assert_eq!(id, 0);
        assert_eq!(s.queue_len(), 1);
        let p = s.dequeue_prompt().unwrap();
        assert_eq!(p.text, "first");
        assert_eq!(p.kind, QueueEntryKind::Prompt);
        assert!(p.wire_blocks.is_none());
        assert!(p.images.is_empty());
        assert!(!p.display_as_skill);
    }
    #[test]
    fn enqueue_in_flight_prompt_front_preserves_images_and_chips() {
        let mut session = test_session();
        let image = crate::prompt_images::from_clipboard_data(&crate::clipboard::ImageData {
            data: vec![1, 2, 3],
            mime_type: "image/png".into(),
        });
        session.enqueue_in_flight_prompt_front(InFlightPrompt {
            text: "look [Image #1]".into(),
            images: vec![image],
            scrollback_entry: EntryId::new(1),
            combined_scrollback_entries: Vec::new(),
            chip_elements: vec![ChipElement {
                range: 5..15,
                kind: crate::views::prompt_widget::KIND_IMAGE,
                display: None,
            }],
        });
        let queued = session.dequeue_prompt().unwrap();
        assert_eq!(queued.images.len(), 1);
        assert_eq!(queued.chip_elements.len(), 1);
    }
    #[test]
    fn enqueue_prompt_front_prepends_directive_before_user_prompts() {
        let mut s = test_session();
        let user_a = s.enqueue_prompt("user-a".into());
        let user_b = s.enqueue_prompt("user-b".into());
        let directive = s.enqueue_prompt_front("/fork directive".into());
        assert!(directive > user_b && user_b > user_a);
        assert_eq!(s.queue_len(), 3);
        let texts: Vec<String> =
            std::iter::from_fn(|| s.dequeue_prompt().map(|p| p.text)).collect();
        assert_eq!(texts, vec!["/fork directive", "user-a", "user-b"]);
    }
    #[test]
    fn enqueue_prompt_front_assigns_monotonic_ids() {
        let mut s = test_session();
        let id0 = s.enqueue_prompt_front("a".into());
        let id1 = s.enqueue_prompt_front("b".into());
        let id2 = s.enqueue_prompt_front("c".into());
        assert_eq!((id0, id1, id2), (0, 1, 2));
        let texts: Vec<String> =
            std::iter::from_fn(|| s.dequeue_prompt().map(|p| p.text)).collect();
        assert_eq!(texts, vec!["c", "b", "a"]);
    }
    #[test]
    fn dequeue_preserves_wire_blocks() {
        let mut s = test_session();
        let id = s.next_queue_id;
        s.next_queue_id += 1;
        let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            "<skill>test</skill>",
        ))];
        s.pending_prompts.push_back(QueuedPrompt {
            wire_blocks: Some(blocks.clone()),
            display_as_skill: true,
            ..QueuedPrompt::plain(id, "/commit fix", QueueEntryKind::Prompt)
        });
        let p = s.dequeue_prompt().unwrap();
        assert!(p.wire_blocks.is_some());
        let wb = p.wire_blocks.unwrap();
        assert_eq!(wb.len(), 1);
    }
    #[test]
    fn swap_preserves_wire_blocks() {
        let mut s = test_session();
        let id_normal = s.enqueue_prompt("normal".into());
        let id_skill = s.next_queue_id;
        s.next_queue_id += 1;
        let blocks = vec![acp::ContentBlock::Text(acp::TextContent::new(
            "<skill>body</skill>",
        ))];
        s.pending_prompts.push_back(QueuedPrompt {
            wire_blocks: Some(blocks),
            display_as_skill: true,
            ..QueuedPrompt::plain(id_skill, "/commit", QueueEntryKind::Prompt)
        });
        s.swap_prompt_up(id_skill);
        assert_eq!(s.pending_prompts[0].id, id_skill);
        assert!(s.pending_prompts[0].wire_blocks.is_some());
        assert_eq!(s.pending_prompts[1].id, id_normal);
        assert!(s.pending_prompts[1].wire_blocks.is_none());
    }
    #[test]
    fn mixed_queue_with_wire_blocks_drains_fifo() {
        let mut s = test_session();
        s.enqueue_prompt("plain".into());
        let id = s.next_queue_id;
        s.next_queue_id += 1;
        s.pending_prompts.push_back(QueuedPrompt {
            wire_blocks: Some(vec![acp::ContentBlock::Text(acp::TextContent::new(
                "skill body",
            ))]),
            display_as_skill: true,
            ..QueuedPrompt::plain(id, "/commit fix", QueueEntryKind::Prompt)
        });
        s.enqueue_bash_command("ls".into());
        let e1 = s.dequeue_prompt().unwrap();
        assert!(e1.wire_blocks.is_none());
        assert_eq!(e1.text, "plain");
        let e2 = s.dequeue_prompt().unwrap();
        assert!(e2.wire_blocks.is_some());
        assert_eq!(e2.text, "/commit fix");
        let e3 = s.dequeue_prompt().unwrap();
        assert!(e3.wire_blocks.is_none());
        assert_eq!(e3.kind, QueueEntryKind::BashCommand);
    }
    #[test]
    fn dequeue_combined_prompt_merges_three_consecutive_prompts() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.enqueue_prompt("second".into());
        s.enqueue_prompt("third".into());
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "first\n\nsecond\n\nthird");
        assert_eq!(merged.combined_texts, vec!["first", "second", "third"]);
        assert_eq!(merged.kind, QueueEntryKind::Prompt);
        assert!(s.dequeue_prompt().is_none(), "all three must be consumed");
    }
    #[test]
    fn dequeue_combined_prompt_stops_at_bash_command() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.enqueue_prompt("second".into());
        s.enqueue_bash_command("ls".into());
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "first\n\nsecond");
        assert_eq!(s.queue_len(), 1, "the bash command must stay queued");
        let remaining = s.dequeue_prompt().unwrap();
        assert_eq!(remaining.kind, QueueEntryKind::BashCommand);
        assert_eq!(remaining.text, "ls");
    }
    #[test]
    fn dequeue_combined_prompt_stops_at_command() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.enqueue_prompt("second".into());
        s.enqueue_command("/compact".into());
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "first\n\nsecond");
        assert_eq!(s.queue_len(), 1, "the slash command must stay queued");
        let remaining = s.dequeue_prompt().unwrap();
        assert_eq!(remaining.kind, QueueEntryKind::Command);
        assert_eq!(remaining.text, "/compact");
    }
    #[test]
    fn dequeue_combined_prompt_single_leading_prompt_returns_unchanged() {
        let mut s = test_session();
        s.enqueue_prompt("only".into());
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "only");
        assert!(!merged.text.contains("\n\n"), "no merge means no separator");
        assert!(s.dequeue_prompt().is_none());
    }
    #[test]
    fn dequeue_combined_prompt_front_non_prompt_returns_single_entry_queue_intact() {
        let mut s = test_session();
        s.enqueue_bash_command("ls".into());
        s.enqueue_prompt("follow-up".into());
        let front = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(front.kind, QueueEntryKind::BashCommand);
        assert_eq!(front.text, "ls");
        assert_eq!(
            s.queue_len(),
            1,
            "the trailing prompt must stay queued, not merged into the bash entry"
        );
        let remaining = s.dequeue_prompt().unwrap();
        assert_eq!(remaining.text, "follow-up");
    }
    #[test]
    fn dequeue_combined_prompt_stops_before_row_under_edit() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.enqueue_prompt("second".into());
        s.enqueue_prompt("third".into());
        let second_id = s.pending_prompts[1].id;
        let merged = s.dequeue_combined_prompt(Some(second_id)).unwrap();
        assert_eq!(
            merged.text, "first",
            "merge stops before the edited follower"
        );
        assert_eq!(
            s.queue_len(),
            2,
            "edited row and everything after it stay queued"
        );
        let next = s.dequeue_prompt().unwrap();
        assert_eq!(
            next.id, second_id,
            "edited row preserved at the front, id intact"
        );
        assert_eq!(next.text, "second");
    }
    #[test]
    fn dequeue_combined_prompt_merges_up_to_row_under_edit() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        s.enqueue_prompt("second".into());
        s.enqueue_prompt("third".into());
        let third_id = s.pending_prompts[2].id;
        let merged = s.dequeue_combined_prompt(Some(third_id)).unwrap();
        assert_eq!(merged.text, "first\n\nsecond");
        assert_eq!(s.queue_len(), 1, "only the edited row stays queued");
        let next = s.dequeue_prompt().unwrap();
        assert_eq!(next.id, third_id, "edited row preserved, id intact");
        assert_eq!(next.text, "third");
    }
    #[test]
    fn dequeue_combined_prompt_stops_at_expanded_wire_prompt() {
        let mut s = test_session();
        s.enqueue_prompt("first".into());
        let id = s.next_queue_id;
        s.next_queue_id += 1;
        s.pending_prompts.push_back(QueuedPrompt {
            wire_blocks: Some(vec![acp::ContentBlock::Text(acp::TextContent::new(
                "<skill>body</skill>",
            ))]),
            ..QueuedPrompt::plain(id, "/loop check status", QueueEntryKind::Prompt)
        });
        s.enqueue_prompt("third".into());
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "first");
        assert_eq!(
            s.queue_len(),
            2,
            "the expanded-wire prompt and its follower must stay queued"
        );
    }
    #[test]
    fn dequeue_combined_prompt_reoffsets_chip_ranges_for_second_entry() {
        let mut s = test_session();
        let id0 = s.next_queue_id;
        s.next_queue_id += 1;
        s.pending_prompts.push_back(QueuedPrompt {
            chip_elements: vec![ChipElement {
                range: 0..5,
                kind: crate::views::prompt_widget::KIND_IMAGE,
                display: None,
            }],
            ..QueuedPrompt::plain(id0, "first", QueueEntryKind::Prompt)
        });
        let id1 = s.next_queue_id;
        s.next_queue_id += 1;
        s.pending_prompts.push_back(QueuedPrompt {
            chip_elements: vec![ChipElement {
                range: 2..6,
                kind: crate::views::prompt_widget::KIND_IMAGE,
                display: None,
            }],
            ..QueuedPrompt::plain(id1, "second!", QueueEntryKind::Prompt)
        });
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "first\n\nsecond!");
        assert_eq!(merged.chip_elements.len(), 2);
        assert_eq!(merged.chip_elements[0].range, 0..5);
        assert_eq!(merged.chip_elements[1].range, 9..13);
        assert_eq!(&merged.text[9..13], "cond");
    }
    /// An image-bearing follower must NOT be folded in — merging two image
    /// sets would require renumbering `[Image #N]` placeholders, which this
    /// v1 does not do. The front entry's own image is unaffected.
    #[test]
    fn dequeue_combined_prompt_stops_at_image_bearing_follower_keeps_own_image() {
        let mut s = test_session();
        let own_image = crate::prompt_images::from_clipboard_data(&crate::clipboard::ImageData {
            data: vec![1, 2, 3],
            mime_type: "image/png".into(),
        });
        let follower_image =
            crate::prompt_images::from_clipboard_data(&crate::clipboard::ImageData {
                data: vec![4, 5, 6],
                mime_type: "image/png".into(),
            });
        let id0 = s.next_queue_id;
        s.next_queue_id += 1;
        s.pending_prompts.push_back(QueuedPrompt {
            images: vec![own_image],
            ..QueuedPrompt::plain(id0, "front [Image #1]", QueueEntryKind::Prompt)
        });
        let id1 = s.next_queue_id;
        s.next_queue_id += 1;
        s.pending_prompts.push_back(QueuedPrompt {
            images: vec![follower_image],
            ..QueuedPrompt::plain(id1, "follower [Image #1]", QueueEntryKind::Prompt)
        });
        s.enqueue_prompt("plain follow-up".into());
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(
            merged.text, "front [Image #1]",
            "image-bearing follower must not merge in"
        );
        assert_eq!(
            merged.images.len(),
            1,
            "the front entry's own image is preserved"
        );
        assert_eq!(
            s.queue_len(),
            2,
            "the image-bearing follower and the plain prompt after it must stay queued \
             (the run stops at the first ineligible entry, it doesn't skip past it)"
        );
        let next = s.dequeue_prompt().unwrap();
        assert_eq!(next.text, "follower [Image #1]");
        assert_eq!(next.images.len(), 1);
    }
    #[test]
    fn dequeue_combined_prompt_clears_skill_token_ranges_on_multi() {
        let mut s = test_session();
        s.enqueue_prompt_with_skill_tokens("hi /commit".into(), vec![3..10]);
        s.enqueue_prompt_with_skill_tokens("go /push now".into(), vec![3..8]);
        let merged = s.dequeue_combined_prompt(None).unwrap();
        assert_eq!(merged.text, "hi /commit\n\ngo /push now");
        assert!(merged.skill_token_ranges.is_empty());
        assert_eq!(
            merged.combined_texts,
            vec!["hi /commit".to_string(), "go /push now".to_string()]
        );
    }
}
