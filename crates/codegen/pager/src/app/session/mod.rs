//! Agent business types.
//!
//! Pure data types for agent session management. No UI or rendering logic.
//! The view-model that combines these with UI state is [`super::agent_view::AgentView`].
pub(crate) mod activity;
use crate::acp::meta::NotificationMeta;
use crate::acp::model_state::ModelState;
use crate::acp::tracker::{AcpUpdateTracker, TurnActivity};
use crate::app::subagent::SubagentInfo;
use crate::scrollback::EntryId;
use crate::scrollback::state::ScrollbackState;
use acp_transport::AcpAgentTx;
use acp_transport::protocol as acp;
use shell::sampling::types::ReasoningEffort;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use tools::implementations::grow_build::workflow::WORKFLOW_TOOL_NAME;
use unicode_width::UnicodeWidthStr;

/// MCP server initialization progress, received from the shell.
#[derive(Debug, Clone)]
pub struct McpInitProgress {
    pub total: u32,
    pub connected: u32,
    pub started_at: Instant,
}

impl McpInitProgress {
    /// Max age for a `total == 0` seed before it auto-expires.
    pub const SEED_EXPIRE: Duration = Duration::from_secs(30);

    /// Whether the progress indicator should be visible in the UI.
    pub fn is_visible(&self) -> bool {
        self.total > 0 || self.started_at.elapsed() < Self::SEED_EXPIRE
    }
}

#[cfg(test)]
mod mcp_init_progress_tests {
    use super::McpInitProgress;
    #[test]
    fn is_visible_requires_servers_or_fresh_seed() {
        let real = McpInitProgress {
            total: 3,
            connected: 1,
            started_at: std::time::Instant::now(),
        };
        assert!(real.is_visible(), "real progress must be visible");
        let fresh = McpInitProgress {
            total: 0,
            connected: 0,
            started_at: std::time::Instant::now(),
        };
        assert!(fresh.is_visible(), "fresh seed must be visible");
        let expired = McpInitProgress {
            total: 0,
            connected: 0,
            started_at: std::time::Instant::now()
                - McpInitProgress::SEED_EXPIRE
                - std::time::Duration::from_secs(1),
        };
        assert!(!expired.is_visible(), "expired seed must not be visible");
    }
}
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
    BudgetLimited,
    Complete,
}
impl GoalDisplayStatus {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "active" => Self::Active,
            "paused" => Self::Paused,
            "blocked" => Self::Blocked,
            "budget_limited" => Self::BudgetLimited,
            "complete" => Self::Complete,
            _ => return None,
        })
    }

    pub fn stopped_label(&self) -> &'static str {
        match self {
            Self::Paused => "Paused",
            Self::Blocked => "Blocked",
            Self::Active | Self::BudgetLimited | Self::Complete => "",
        }
    }

    pub fn uses_warning_chip(&self) -> bool {
        matches!(self, Self::Paused | Self::Blocked)
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
    /// `tokens_used` is a lower bound because at least one admitted provider
    /// attempt returned no usage. Shell pauses the Goal while this is true.
    pub usage_incomplete: bool,
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
            usage_incomplete: false,
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
    /// Grow prompt usage decoded from the stable ACP response's `_meta`.
    pub(crate) usage: Option<shell::extensions::notification::PromptUsage>,
    /// `_meta.structuredOutput` / `_meta.structuredOutputError` from the late
    /// response: `Ok(value)` when the model produced schema-validated output,
    /// `Err(message)` when the shell reported a structured-output failure.
    pub(crate) structured_output: Option<Result<serde_json::Value, String>>,
    /// `Err` text when the late RPC resolved as an error (the durable rail's
    /// `agent_result` may be coarser or absent).
    pub(crate) error: Option<String>,
}

/// A workflow agent row projected from the Shell's runtime state.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowAgentRowView {
    pub agent_id: String,
    pub label: String,
    pub phase: Option<String>,
    pub model: Option<String>,
    pub state: String,
    pub tokens_used: u64,
    pub duration_ms: u64,
}

/// A workflow run snapshot projected from the Shell's runtime state.
#[derive(Debug, Clone)]
pub struct WorkflowRunSnapshot {
    pub run_id: String,
    pub definition_id: Option<String>,
    pub definition_scope: Option<String>,
    pub definition_hash: Option<String>,
    pub name: String,
    pub objective: String,
    pub status: String,
    pub management_available: bool,
    pub phases: Vec<(String, String)>,
    pub current_phase: Option<String>,
    pub agents: Vec<WorkflowAgentRowView>,
    pub agent_budget: Option<u64>,
    pub agents_used: u64,
    pub agents_remaining: Option<u64>,
    pub agent_usage_incomplete: bool,
    pub active_agents: u32,
    pub elapsed_ms: u64,
    pub received_at: Instant,
    pub pause_message: Option<String>,
    pub result_summary: Option<String>,
}

impl WorkflowRunSnapshot {
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status.as_str(),
            "interrupted" | "complete" | "failed" | "cancelled"
        )
    }

    pub fn can_pause(&self) -> bool {
        self.management_available && self.is_active()
    }

    pub fn can_resume(&self) -> bool {
        if !self.management_available {
            return false;
        }
        matches!(
            self.status.as_str(),
            "user_paused"
                | "back_off_paused"
                | "no_progress_paused"
                | "infra_paused"
                | "blocked"
                | "failed"
        )
    }

    pub fn can_stop(&self) -> bool {
        self.management_available && !self.is_terminal()
    }

    pub fn active_agent_count(&self) -> usize {
        self.agents.iter().filter(|a| a.state == "running").count()
    }

    pub fn live_elapsed_ms_at(&self, now: Instant) -> u64 {
        let base = self.elapsed_ms;
        if self.is_active() {
            base.saturating_add(now.saturating_duration_since(self.received_at).as_millis() as u64)
        } else {
            base
        }
    }

    pub fn live_elapsed_ms(&self) -> u64 {
        self.live_elapsed_ms_at(Instant::now())
    }

    pub fn agents_in_phase(&self, phase: Option<&str>) -> Vec<&WorkflowAgentRowView> {
        match phase {
            Some(title) => self
                .agents
                .iter()
                .filter(|a| a.phase.as_deref() == Some(title))
                .collect(),
            None => self.agents.iter().collect(),
        }
    }

    pub fn phase_has_running_agents(&self, phase: &str) -> bool {
        self.agents
            .iter()
            .any(|a| a.state == "running" && a.phase.as_deref() == Some(phase))
    }
}

/// Per-agent business logic (ACP session, models, state).
///
/// External code should use the facade methods (`handle_update`,
/// `start_turn`, `finish_turn`, `turn_activity`) instead of accessing
/// the tracker directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionControlToken {
    pub(crate) client_id: uuid::Uuid,
    /// Stable user-intent generation. This is deliberately not changed by a
    /// transport reconnect: replaying a request must not become a fresh user
    /// confirmation at the Shell boundary.
    pub(crate) generation: u64,
    pub(crate) sequence: u64,
    /// Local dispatch epoch. This is not sent to the Shell; it only prevents a
    /// completion from the replaced ACP transport from clearing the retry.
    pub(crate) dispatch_generation: u64,
}

impl SessionControlToken {
    pub(crate) fn shell_intent(self) -> shell::session::ControlIntent {
        shell::session::ControlIntent {
            client_id: self.client_id.to_string(),
            generation: self.generation,
            sequence: self.sequence,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PendingSessionControl {
    Model {
        model_id: shell::agent::models::ModelId,
        effort: Option<ReasoningEffort>,
        /// True when the model id is only a local display hint and Shell must
        /// compose the effort with its newest desired Sampling target.
        effort_patch: bool,
    },
    Agent {
        agent_name: String,
    },
    Behavior {
        mode: tools::types::BehaviorId,
    },
}

/// Private process-relaunch handoff for one Session's newest desired controls.
///
/// `/minimal` and `/fullscreen` replace the pager process. Embedded Shell
/// actors therefore disappear with it, while a leader-owned actor may remain.
/// Preserving the original Shell intent token lets the resumed pager safely
/// cover both cases: a fresh actor admits it, and a resident actor recognizes
/// the exact in-flight/terminal receipt without treating a renderer change as
/// a second user decision.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionControlHandoff {
    pub(crate) session_id: String,
    pub(crate) client_id: uuid::Uuid,
    pub(crate) generation: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sampling: Option<SamplingControlHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<NamedControlHandoff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) behavior: Option<BehaviorControlHandoff>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SamplingControlHandoff {
    pub(crate) sequence: u64,
    pub(crate) model_id: String,
    pub(crate) effort: Option<ReasoningEffort>,
    pub(crate) effort_patch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NamedControlHandoff {
    pub(crate) sequence: u64,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BehaviorControlHandoff {
    pub(crate) sequence: u64,
    pub(crate) behavior: tools::types::BehaviorId,
}

#[derive(Debug, Clone)]
struct InFlightSessionControl {
    token: SessionControlToken,
    control: PendingSessionControl,
    needs_dispatch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionControlCompletion {
    Stale,
    Drained,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BehaviorControlResolution {
    Applied,
    Rejected,
    ConfirmationRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionControlDomain {
    Sampling,
    Agent,
    Behavior,
}

impl PendingSessionControl {
    fn domain(&self) -> SessionControlDomain {
        match self {
            Self::Model { .. } => SessionControlDomain::Sampling,
            Self::Agent { .. } => SessionControlDomain::Agent,
            Self::Behavior { .. } => SessionControlDomain::Behavior,
        }
    }
}

#[derive(Debug, Default)]
struct SessionControlSlot {
    in_flight: Option<InFlightSessionControl>,
    reconnect_applied_candidate: Option<SessionControlToken>,
}

#[derive(Debug)]
struct SessionControlState {
    client_id: uuid::Uuid,
    generation: u64,
    dispatch_generation: u64,
    next_sequence: u64,
    sampling: SessionControlSlot,
    agent: SessionControlSlot,
    behavior: SessionControlSlot,
}

impl Default for SessionControlState {
    fn default() -> Self {
        Self {
            client_id: uuid::Uuid::new_v4(),
            generation: 0,
            dispatch_generation: 0,
            next_sequence: 0,
            sampling: SessionControlSlot::default(),
            agent: SessionControlSlot::default(),
            behavior: SessionControlSlot::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct ShellControlSlot {
    revision: u64,
    intent: Option<shell::session::ControlIntent>,
    phase: shell::extensions::notification::ControlPhase,
    current: shell::extensions::notification::ControlTarget,
    desired: Option<shell::extensions::notification::ControlTarget>,
    /// Present only on the immutable durable terminal event. A reconnect
    /// snapshot may legitimately publish `Applied` with no message before
    /// replay reaches that event, so phase alone cannot seal a revision.
    terminal_message: Option<String>,
    phase_since: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShellControlApplyOutcome {
    Rejected,
    Accepted { changed: bool },
}

impl ShellControlApplyOutcome {
    pub(crate) const fn accepted(self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    pub(crate) const fn changed(self) -> bool {
        matches!(self, Self::Accepted { changed: true })
    }
}

#[derive(Debug, Default)]
struct ShellControlState {
    active_epoch: Option<String>,
    retired_epochs: HashSet<String>,
    sampling: Option<ShellControlSlot>,
    agent: Option<ShellControlSlot>,
    behavior: Option<ShellControlSlot>,
}

impl ShellControlState {
    fn slot(
        &self,
        domain: shell::extensions::notification::ControlDomain,
    ) -> Option<&ShellControlSlot> {
        match domain {
            shell::extensions::notification::ControlDomain::Sampling => self.sampling.as_ref(),
            shell::extensions::notification::ControlDomain::Agent => self.agent.as_ref(),
            shell::extensions::notification::ControlDomain::Behavior => self.behavior.as_ref(),
        }
    }

    fn slot_mut(
        &mut self,
        domain: shell::extensions::notification::ControlDomain,
    ) -> &mut Option<ShellControlSlot> {
        match domain {
            shell::extensions::notification::ControlDomain::Sampling => &mut self.sampling,
            shell::extensions::notification::ControlDomain::Agent => &mut self.agent,
            shell::extensions::notification::ControlDomain::Behavior => &mut self.behavior,
        }
    }
}

impl SessionControlState {
    fn slot(&self, domain: SessionControlDomain) -> &SessionControlSlot {
        match domain {
            SessionControlDomain::Sampling => &self.sampling,
            SessionControlDomain::Agent => &self.agent,
            SessionControlDomain::Behavior => &self.behavior,
        }
    }

    fn slot_mut(&mut self, domain: SessionControlDomain) -> &mut SessionControlSlot {
        match domain {
            SessionControlDomain::Sampling => &mut self.sampling,
            SessionControlDomain::Agent => &mut self.agent,
            SessionControlDomain::Behavior => &mut self.behavior,
        }
    }

    fn slots(&self) -> [&SessionControlSlot; 3] {
        [&self.sampling, &self.agent, &self.behavior]
    }
}

pub struct AgentSession {
    pub id: AgentId,
    pub acp_tx: AcpAgentTx,
    pub session_id: Option<acp::SessionId>,
    pub models: ModelState,
    pub state: AgentState,
    pub cwd: PathBuf,
    /// Cached server-reported context state.
    pub(crate) context_state: Option<shell::session::ContextInfo>,
    /// Current long-lived Goal state. Set by `GoalUpdated` session
    /// notifications, cleared when a new session starts.
    pub(crate) goal_state: Option<GoalDisplayState>,
    /// Goal id of the most recently cleared goal, captured from the dropped
    /// state (the `cleared` event itself carries an empty id). Drops a late
    /// in-flight `GoalUpdated` that would otherwise resurrect the cleared
    /// chip/modal. Single slot: goal ids are unique, so only the latest clear
    /// can race a stale update.
    pub(crate) last_cleared_goal_id: Option<String>,
    /// Public workflow runs projected from ACP/Grow notifications.
    pub(crate) workflow_runs: Vec<WorkflowRunSnapshot>,
    /// Highest accepted revision per workflow run.
    pub(crate) workflow_run_revisions: HashMap<String, u64>,
    /// Tombstones for workflow runs explicitly cleared by the user/runtime.
    pub(crate) cleared_workflow_runs: HashSet<String>,
    /// Whether this session is running inside a git worktree.
    pub is_worktree: bool,
    /// `AgentId` of the parent session if this session was created via
    /// `/fork`. Display-only (status bar, future agent picker grouping);
    /// navigation does not consult it -- the session picker is the
    /// source of truth for navigation history.
    pub forked_from: Option<AgentId>,
    /// Runtime metadata for child sessions owned by this ACP session.
    ///
    /// This is the canonical lifecycle/provenance map used by notification
    /// routing, permission attribution, dashboard rows, and reconnect
    /// reconciliation. The recursive `AgentView::subagent_views` tree remains
    /// presentation state and is intentionally kept on the view side.
    pub subagent_sessions: HashMap<String, SubagentInfo>,
    /// Prompts waiting to be sent. Drained front-to-back when
    /// `state` becomes [`AgentState::Idle`].
    pub pending_prompts: VecDeque<QueuedPrompt>,
    /// Next monotonic ID for [`QueuedPrompt`].
    pub(crate) next_queue_id: u64,
    /// Canonical permission mode for this session.
    pub(crate) permission_mode: shell::util::config::PermissionMode,
    /// Number of permission requests waiting for the session's response.
    /// The permission view owns the presentation queue; this scalar is the
    /// lifecycle fact consumed by activity projection.
    pub(crate) pending_permission_count: usize,
    /// Whether this session currently has a question awaiting user input.
    /// The question view remains presentation state on `AgentView`.
    pub(crate) question_pending: bool,
    /// Whether an extensions list fetch is pending for this session.
    pending_extensions_fetch: bool,
    /// IDs of this client's server-queue rows that are still optimistic
    /// echoes: their `session/prompt` RPC is in flight and no authoritative
    /// `grow/queue/changed` broadcast has confirmed them yet.
    optimistic_queue_ids: HashSet<String>,
    /// A queue-row send-now intent parked until its optimistic echo is
    /// confirmed. Sending earlier could overtake the row's `session/prompt`
    /// RPC and silently no-op in the shell.
    send_now_awaiting_confirm: Option<String>,
    /// MCP server initialization progress owned by this ACP session.
    mcp_init_progress: Option<McpInitProgress>,
    /// IDs of interjections this client sent and already rendered locally.
    /// The shell broadcasts each interjection to every attached pane; the
    /// originating session consumes its own id here to suppress the echoed
    /// copy while other panes still render it.
    self_interjection_ids: HashSet<String>,
    /// Running agent definition reported for this ACP session.
    session_agent_name: Option<String>,
    /// Local request correlation only. The Shell owns desired state and
    /// publishes the presentation projection below.
    controls: SessionControlState,
    /// Latest typed Shell-authoritative UI projection. It never enters prompt
    /// construction and is cleared on reconnect until the Shell re-sends a
    /// fresh snapshot.
    shell_controls: ShellControlState,
    /// One keyed, replaceable progress fact for the live status row. It is
    /// presentation-only, never persisted, and never projected into model
    /// context. Clearing is key-checked so completion of an older operation
    /// cannot erase a newer status that replaced it.
    live_feedback: Option<(&'static str, crate::scrollback::blocks::UiFeedback)>,
    /// Latest server-authoritative model state observed while a local route
    /// control is outstanding. Applying it immediately would make the local
    /// completion appear unchanged; dropping it would let a second client leave
    /// this view permanently stale. It is applied once the matching Sampling
    /// revision settles.
    pending_authoritative_model_change: Option<(String, Option<String>)>,
    /// Equivalent parked server-authoritative Agent state. Sampling and Agent
    /// revisions are independent, so retain the latest value of each kind.
    pending_authoritative_agent_change: Option<String>,
    /// Monotonic token for asynchronous session metadata reads.
    agent_metadata_revision: u64,
    /// Prompt currently being reconciled by the submission watchdog. This
    /// bounds status requests to one in flight per prompt.
    prompt_status_query_for: Option<String>,
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
    /// [`crate::app::root::effects`]'s parser tests + the deserialise tests in
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
    /// Agent names frozen by the owning Workflow Run. `None` means this
    /// session follows ordinary live Agent discovery.
    pub(crate) workflow_agent_names: Option<Vec<String>>,
    /// Model the user chose this session via `/model` / the model picker, or
    /// the last successfully applied live remote `ModelChanged` (leader-mode
    /// fan-out). Survives reconnect (`begin_session_reload` does **not** clear
    /// it). History-replay silent-revert of a prior choice is suppressed on the
    /// shell side via `ReconnectState::user_selected_model`; the pager still
    /// applies live remote switches and updates this field to match.
    pub user_model_preference: Option<shell::agent::models::ModelId>,
    /// `/model X [effort]` issued before the session was ready, applied on SessionCreated.
    pub deferred_model_switch: Option<(shell::agent::models::ModelId, Option<ReasoningEffort>)>,
    /// Whether the confirmed Behavior is Plan. Derived only from
    /// `CurrentModeUpdate`; tool titles never change it.
    pub(crate) plan_mode_active: bool,
    /// Confirmed user-facing Behavior. Permission policy is tracked separately.
    pub(crate) behavior_mode: tools::types::BehaviorId,
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
    /// UTC ms when the current turn started (`turnStartMs` from notification meta).
    /// Used for turn elapsed display.
    pub turn_start_ms: Option<i64>,
    /// Prompt id the stored `turn_start_ms` belongs to (stamped together from
    /// the same delta meta): wake markers may only claim an elapsed whose
    /// anchor is provably their own turn's.
    pub turn_start_ms_prompt: Option<String>,
    /// Local wall-clock time when the current turn started.
    /// Set by `maybe_drain_queue` when a prompt is sent. Used to compute
    /// elapsed time for "Worked for Xm Ys" system messages.
    pub turn_started_at: Option<Instant>,
    /// Last reducer-observed prompt activity for lifecycle reconciliation.
    /// Never updated by rendering.
    pub(crate) last_prompt_event_at: Option<Instant>,
    /// Last authoritative Running status observation for the current prompt.
    pub(crate) last_status_observed_at: Option<Instant>,
    /// Turn-start anchor a `turn.first_activity` log was already emitted for (fire-once-per-turn guard).
    pub first_activity_logged_for: Option<Instant>,
    /// Accumulated duration the turn timer was paused (while the user was
    /// answering questions via `AskUserQuestion`). Reset when the turn ends.
    pub turn_paused_duration: Duration,
    /// Wall-clock twin of `turn_paused_duration`: the same pauses measured on
    /// the wall clock, which keeps counting through OS suspend while `Instant`
    /// does not. Netted against the wall-anchored turn span so a suspend
    /// during an open question isn't reported as worked time.
    pub turn_paused_wall: Duration,
    /// Local wall-clock time when the most recent turn finished
    /// (success, failure, or cancellation). Used by the dashboard
    /// modal to display "Nm ago" idle markers. Initialised to the
    /// session-creation time in [`AgentSession::new`] so newly-created
    /// agents that have never run a turn still show a sensible
    /// relative time.
    pub last_active_at: Option<Instant>,
    /// Local wall-clock time when the current activity phase started.
    /// Reset on each activity transition (thinking → responding → tool, etc.).
    /// Used for the `(5s)` phase timer in the turn status line.
    pub activity_started_at: Option<Instant>,
    /// Last observed [`TurnActivity`] — used to detect phase transitions
    /// and reset `activity_started_at`.
    pub(crate) last_activity: Option<TurnActivity>,
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
    /// During a replay window, records whether a live update has already
    /// advanced the cursor. Event IDs are opaque across resume generations;
    /// this prevents a later historical replay from overwriting that live
    /// frontier without comparing unrelated IDs.
    pub(crate) replay_live_cursor_seen: bool,
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
const WORKFLOW_RUN_COMMAND_NAME: &str = "workflow-run";
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
            context_state: None,
            goal_state: None,
            last_cleared_goal_id: None,
            workflow_runs: Vec::new(),
            workflow_run_revisions: HashMap::new(),
            cleared_workflow_runs: HashSet::new(),
            is_worktree: false,
            forked_from: None,
            subagent_sessions: HashMap::new(),
            pending_prompts: VecDeque::new(),
            next_queue_id: 0,
            permission_mode,
            pending_permission_count: 0,
            question_pending: false,
            pending_extensions_fetch: false,
            optimistic_queue_ids: HashSet::new(),
            send_now_awaiting_confirm: None,
            mcp_init_progress: None,
            self_interjection_ids: HashSet::new(),
            session_agent_name: None,
            controls: SessionControlState::default(),
            shell_controls: ShellControlState::default(),
            live_feedback: None,
            pending_authoritative_model_change: None,
            pending_authoritative_agent_change: None,
            agent_metadata_revision: 0,
            prompt_status_query_for: None,
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
            workflow_agent_names: None,
            user_model_preference: None,
            deferred_model_switch: None,
            plan_mode_active: false,
            behavior_mode: tools::types::BehaviorId::Normal,
            plan_phase: None,
            deferred_session_mode: None,
            bg_tasks: BTreeMap::new(),
            bg_tool_call_to_task: HashMap::new(),
            scheduled_tasks: HashMap::new(),
            in_flight_prompt: None,
            compact_held_prompt: None,
            current_prompt_id: None,
            turn_start_ms: None,
            turn_start_ms_prompt: None,
            turn_started_at: None,
            last_prompt_event_at: None,
            last_status_observed_at: None,
            first_activity_logged_for: None,
            turn_paused_duration: Duration::ZERO,
            turn_paused_wall: Duration::ZERO,
            last_active_at: Some(Instant::now()),
            activity_started_at: None,
            last_activity: None,
            shared_queue: Vec::new(),
            attached_as_viewer: false,
            self_originated_prompt_ids: VecDeque::new(),
            rewound_prompt_ids: VecDeque::new(),
            last_applied_event_seq: None,
            last_applied_grow_event_seq: None,
            last_seen_event_id: None,
            replay_live_cursor_seen: false,
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
        self.replay_live_cursor_seen = false;
    }

    pub(crate) fn mark_created_via_new(&mut self) {
        self.created_via_new = true;
    }

    pub(crate) fn remember_self_interjection(&mut self, id: impl Into<String>) {
        self.self_interjection_ids.insert(id.into());
    }

    pub(crate) fn consume_self_interjection(&mut self, id: &str) -> bool {
        self.self_interjection_ids.remove(id)
    }

    #[cfg(test)]
    pub(crate) fn has_self_interjection(&self, id: &str) -> bool {
        self.self_interjection_ids.contains(id)
    }

    pub(crate) fn agent_name(&self) -> Option<&str> {
        self.session_agent_name.as_deref()
    }

    pub(crate) fn begin_agent_metadata_read(&mut self) -> u64 {
        self.agent_metadata_revision = self.agent_metadata_revision.saturating_add(1);
        self.agent_metadata_revision
    }

    pub(crate) fn agent_metadata_read_is_current(&self, revision: u64) -> bool {
        self.agent_metadata_revision == revision
    }

    pub(crate) fn apply_agent_name(&mut self, name: Option<String>) -> bool {
        let changed = self.session_agent_name != name;
        self.session_agent_name = name;
        self.agent_metadata_revision = self.agent_metadata_revision.saturating_add(1);
        changed
    }

    /// Publish a desired control target. Sampling and Agent requests are sent
    /// immediately so the Shell can replace an older not-yet-applied revision;
    /// Every domain replaces its local correlation token immediately. The
    /// Shell is the sole desired-state authority and rejects stale revisions.
    pub(crate) fn enqueue_control(
        &mut self,
        control: PendingSessionControl,
    ) -> Option<(SessionControlToken, PendingSessionControl)> {
        Some(self.publish_control(control, false))
    }

    /// Retain the newest desired target while the ACP transport is unavailable.
    /// Reconnect reconciliation will claim it exactly once after the replacement
    /// transport has restored the session.
    pub(crate) fn defer_control(&mut self, control: PendingSessionControl) {
        let _ = self.publish_control(control, true);
    }

    fn publish_control(
        &mut self,
        control: PendingSessionControl,
        needs_dispatch: bool,
    ) -> (SessionControlToken, PendingSessionControl) {
        let token = SessionControlToken {
            client_id: self.controls.client_id,
            generation: self.controls.generation,
            sequence: self.controls.next_sequence,
            dispatch_generation: self.controls.dispatch_generation,
        };
        self.controls.next_sequence = self.controls.next_sequence.saturating_add(1);
        let domain = control.domain();
        let queued = InFlightSessionControl {
            token,
            control,
            needs_dispatch,
        };
        let slot = self.controls.slot_mut(domain);
        slot.in_flight = Some(queued.clone());
        (token, queued.control)
    }

    pub(crate) fn screen_mode_control_handoff(&self) -> Option<SessionControlHandoff> {
        let session_id = self.session_id.as_ref()?.0.to_string();
        let sampling = self
            .controls
            .sampling
            .in_flight
            .as_ref()
            .and_then(|pending| {
                let PendingSessionControl::Model {
                    model_id,
                    effort,
                    effort_patch,
                } = &pending.control
                else {
                    return None;
                };
                Some(SamplingControlHandoff {
                    sequence: pending.token.sequence,
                    model_id: model_id.0.to_string(),
                    effort: *effort,
                    effort_patch: *effort_patch,
                })
            });
        let agent = self.controls.agent.in_flight.as_ref().and_then(|pending| {
            let PendingSessionControl::Agent { agent_name } = &pending.control else {
                return None;
            };
            Some(NamedControlHandoff {
                sequence: pending.token.sequence,
                name: agent_name.clone(),
            })
        });
        let behavior = self
            .controls
            .behavior
            .in_flight
            .as_ref()
            .and_then(|pending| {
                let PendingSessionControl::Behavior { mode } = pending.control else {
                    return None;
                };
                Some(BehaviorControlHandoff {
                    sequence: pending.token.sequence,
                    behavior: mode,
                })
            });
        if sampling.is_none() && agent.is_none() && behavior.is_none() {
            return None;
        }
        Some(SessionControlHandoff {
            session_id,
            client_id: self.controls.client_id,
            generation: self.controls.generation,
            sampling,
            agent,
            behavior,
        })
    }

    /// Restore desired controls before replay starts. Every restored slot is
    /// dispatchable and marked as a reconnect candidate so the authoritative
    /// load projection may resolve an already-applied Sampling/Agent target.
    pub(crate) fn restore_screen_mode_control_handoff(&mut self, handoff: SessionControlHandoff) {
        debug_assert_eq!(
            self.session_id.as_ref().map(|id| id.0.as_ref()),
            Some(handoff.session_id.as_str())
        );
        self.controls.client_id = handoff.client_id;
        self.controls.generation = handoff.generation;
        let dispatch_generation = self.controls.dispatch_generation;
        let mut max_sequence = self.controls.next_sequence;

        let mut install =
            |domain: SessionControlDomain, sequence: u64, control: PendingSessionControl| {
                let token = SessionControlToken {
                    client_id: handoff.client_id,
                    generation: handoff.generation,
                    sequence,
                    dispatch_generation,
                };
                let slot = self.controls.slot_mut(domain);
                slot.in_flight = Some(InFlightSessionControl {
                    token,
                    control,
                    needs_dispatch: true,
                });
                slot.reconnect_applied_candidate = Some(token);
                max_sequence = max_sequence.max(sequence.saturating_add(1));
            };
        if let Some(sampling) = handoff.sampling {
            install(
                SessionControlDomain::Sampling,
                sampling.sequence,
                PendingSessionControl::Model {
                    model_id: shell::agent::models::ModelId::new(sampling.model_id),
                    effort: sampling.effort,
                    effort_patch: sampling.effort_patch,
                },
            );
        }
        if let Some(agent) = handoff.agent {
            install(
                SessionControlDomain::Agent,
                agent.sequence,
                PendingSessionControl::Agent {
                    agent_name: agent.name,
                },
            );
        }
        if let Some(behavior) = handoff.behavior {
            install(
                SessionControlDomain::Behavior,
                behavior.sequence,
                PendingSessionControl::Behavior {
                    mode: behavior.behavior,
                },
            );
        }
        drop(install);
        self.controls.next_sequence = max_sequence;
    }

    pub(crate) fn complete_control(
        &mut self,
        token: SessionControlToken,
    ) -> SessionControlCompletion {
        let domain = [
            SessionControlDomain::Sampling,
            SessionControlDomain::Agent,
            SessionControlDomain::Behavior,
        ]
        .into_iter()
        .find(|domain| {
            self.controls
                .slot(*domain)
                .in_flight
                .as_ref()
                .is_some_and(|pending| pending.token == token)
        });
        let Some(domain) = domain else {
            return SessionControlCompletion::Stale;
        };
        let slot = self.controls.slot_mut(domain);
        if slot.reconnect_applied_candidate == Some(token) {
            slot.reconnect_applied_candidate = None;
        }
        slot.in_flight = None;
        SessionControlCompletion::Drained
    }

    pub(crate) fn claim_control_for_dispatch(
        &mut self,
    ) -> Option<(SessionControlToken, PendingSessionControl)> {
        let domain = [
            SessionControlDomain::Sampling,
            SessionControlDomain::Agent,
            SessionControlDomain::Behavior,
        ]
        .into_iter()
        .filter_map(|domain| {
            self.controls
                .slot(domain)
                .in_flight
                .as_ref()
                .filter(|pending| pending.needs_dispatch)
                .map(|pending| (domain, pending.token.sequence))
        })
        .min_by_key(|(_, sequence)| *sequence)
        .map(|(domain, _)| domain)?;
        let pending = self
            .controls
            .slot_mut(domain)
            .in_flight
            .as_mut()
            .expect("selected dispatchable control");
        pending.needs_dispatch = false;
        Some((pending.token, pending.control.clone()))
    }

    #[cfg(test)]
    pub(crate) fn invalidate_controls(&mut self) {
        self.controls.generation = self.controls.generation.saturating_add(1);
        self.controls.dispatch_generation = self.controls.dispatch_generation.saturating_add(1);
        self.controls.sampling = SessionControlSlot::default();
        self.controls.agent = SessionControlSlot::default();
        self.controls.behavior = SessionControlSlot::default();
        self.pending_authoritative_model_change = None;
        self.pending_authoritative_agent_change = None;
    }

    /// Reissue user controls after the ACP transport has been replaced. Old
    /// task results become stale through the local dispatch-epoch bump while
    /// the semantic user intent remains stable. Each domain keeps only its
    /// newest desired target and can reconnect independently.
    pub(crate) fn rearm_controls_for_reconnect(&mut self) {
        self.shell_controls = ShellControlState::default();
        self.controls.dispatch_generation = self.controls.dispatch_generation.saturating_add(1);
        let controls = [
            SessionControlDomain::Sampling,
            SessionControlDomain::Agent,
            SessionControlDomain::Behavior,
        ]
        .into_iter()
        .filter_map(|domain| {
            let slot = self.controls.slot_mut(domain);
            let had_in_flight = slot.in_flight.is_some();
            let pending = slot.in_flight.take()?;
            slot.reconnect_applied_candidate = None;
            Some((domain, had_in_flight, pending))
        })
        .collect::<Vec<_>>();
        for (domain, had_in_flight, pending) in controls {
            let token = SessionControlToken {
                client_id: pending.token.client_id,
                generation: pending.token.generation,
                sequence: pending.token.sequence,
                dispatch_generation: self.controls.dispatch_generation,
            };
            let slot = self.controls.slot_mut(domain);
            if had_in_flight {
                slot.reconnect_applied_candidate = Some(token);
            }
            slot.in_flight = Some(InFlightSessionControl {
                token,
                control: pending.control,
                needs_dispatch: true,
            });
        }
        // Replay reconstructs the authoritative server projection. Parked
        // notifications from the old transport must not overwrite it.
        self.pending_authoritative_model_change = None;
        self.pending_authoritative_agent_change = None;
    }

    pub(crate) fn has_pending_model_control(
        &self,
        model_id: &shell::agent::models::ModelId,
        effort: Option<ReasoningEffort>,
    ) -> bool {
        let slot = &self.controls.sampling;
        slot.in_flight.iter().any(|pending| {
            matches!(
                &pending.control,
                PendingSessionControl::Model {
                    model_id: pending_model,
                    effort: pending_effort,
                    ..
                } if pending_model == model_id && *pending_effort == effort
            )
        })
    }

    pub(crate) fn has_pending_behavior_control(&self, mode: tools::types::BehaviorId) -> bool {
        let slot = &self.controls.behavior;
        slot.in_flight.iter().any(|pending| {
            matches!(
                &pending.control,
                PendingSessionControl::Behavior { mode: pending_mode }
                    if *pending_mode == mode
            )
        })
    }

    /// Resolve an exact local intent from a Shell-authoritative terminal
    /// receipt. Domain-specific committed projections still update the visible
    /// current value, but correlation never guesses from target equality.
    pub(crate) fn resolve_reconnect_control_projection(
        &mut self,
        domain: shell::extensions::notification::ControlDomain,
        phase: shell::extensions::notification::ControlPhase,
        current: &shell::extensions::notification::ControlTarget,
        desired: Option<&shell::extensions::notification::ControlTarget>,
        intent: Option<&shell::session::ControlIntent>,
    ) -> bool {
        use shell::extensions::notification::{ControlDomain, ControlPhase, ControlTarget};

        fn target_matches(control: &PendingSessionControl, target: &ControlTarget) -> bool {
            match (control, target) {
                (
                    PendingSessionControl::Model {
                        model_id,
                        effort,
                        effort_patch,
                    },
                    ControlTarget::Sampling {
                        model_id: actual,
                        reasoning_effort,
                    },
                ) => {
                    (*effort_patch || model_id.0.as_ref() == actual)
                        && effort.is_none_or(|expected| {
                            reasoning_effort.as_deref() == Some(expected.to_string().as_str())
                        })
                }
                (
                    PendingSessionControl::Agent { agent_name },
                    ControlTarget::Agent { agent_name: actual },
                ) => agent_name == actual,
                (
                    PendingSessionControl::Behavior { mode },
                    ControlTarget::Behavior {
                        behavior_id: actual,
                    },
                ) => mode.as_id() == actual,
                _ => false,
            }
        }

        let local_domain = match domain {
            ControlDomain::Sampling => SessionControlDomain::Sampling,
            ControlDomain::Agent => SessionControlDomain::Agent,
            ControlDomain::Behavior => SessionControlDomain::Behavior,
        };
        let slot = self.controls.slot(local_domain);
        let Some(pending) = slot.in_flight.as_ref() else {
            return false;
        };
        let Some(intent) = intent else {
            return false;
        };
        let pending_intent = pending.token.shell_intent();
        if &pending_intent != intent {
            return false;
        }
        let terminal_matches = match phase {
            // A durable terminal receipt carries the target that this exact
            // intent applied. `current` may already reflect a later client
            // revision by the time a reconnecting client replays the receipt.
            ControlPhase::Applied => target_matches(&pending.control, desired.unwrap_or(current)),
            ControlPhase::Rejected => {
                desired.is_some_and(|target| target_matches(&pending.control, target))
            }
            ControlPhase::Superseded => true,
            ControlPhase::Pending | ControlPhase::Applying => false,
        };
        terminal_matches
            && self.complete_control(pending.token) == SessionControlCompletion::Drained
    }

    /// Preserve the newest live model notification until the matching local
    /// Sampling revision reaches its terminal state. The server is authoritative;
    /// this only defers its projection to avoid racing our own RPC completion.
    pub(crate) fn defer_authoritative_model_change(
        &mut self,
        model_id: String,
        reasoning_effort: Option<String>,
    ) {
        self.pending_authoritative_model_change = Some((model_id, reasoning_effort));
    }

    /// A newer model event that can be applied immediately supersedes an
    /// older catalog-blocked value. Without clearing the parked value here, a
    /// later catalog publication could resurrect that stale event.
    pub(crate) fn clear_deferred_authoritative_model_change(&mut self) {
        self.pending_authoritative_model_change = None;
    }

    /// Preserve the newest live Agent notification for the same control barrier
    /// as a model change.
    pub(crate) fn defer_authoritative_agent_change(&mut self, agent_name: String) {
        self.pending_authoritative_agent_change = Some(agent_name);
    }

    /// Take authoritative values only for domains whose desired target has
    /// drained. One slow domain must not delay another domain's projection.
    pub(crate) fn take_deferred_authoritative_controls(
        &mut self,
    ) -> (Option<(String, Option<String>)>, Option<String>) {
        (
            (!self.sampling_control_pending())
                .then(|| self.pending_authoritative_model_change.take())
                .flatten(),
            (!self.agent_control_pending())
                .then(|| self.pending_authoritative_agent_change.take())
                .flatten(),
        )
    }

    pub(crate) fn controls_pending(&self) -> bool {
        self.controls
            .slots()
            .into_iter()
            .any(|slot| slot.in_flight.is_some())
    }

    pub(crate) fn sampling_control_pending(&self) -> bool {
        let slot = &self.controls.sampling;
        slot.in_flight.is_some()
    }

    /// Resolve a local Sampling intent only from the authoritative model
    /// projection that matches its complete desired target. Transport RPC
    /// success is intentionally insufficient: clearing there lets a
    /// following `/effort` compose against the old model.
    pub(crate) fn resolve_sampling_control(
        &mut self,
        model_id: &shell::agent::models::ModelId,
        effort: Option<ReasoningEffort>,
    ) -> bool {
        let Some((token, matches)) = self.controls.sampling.in_flight.as_ref().map(|pending| {
            let matches = match &pending.control {
                PendingSessionControl::Model {
                    model_id: desired_model,
                    effort: desired_effort,
                    effort_patch,
                } => {
                    let expected_effort = (*desired_effort).or_else(|| {
                        self.models.available.get(desired_model).and_then(|info| {
                            shell::sampling::types::parse_reasoning_effort_meta(info.meta.as_ref())
                        })
                    });
                    !*effort_patch && desired_model == model_id && expected_effort == effort
                }
                PendingSessionControl::Agent { .. } | PendingSessionControl::Behavior { .. } => {
                    false
                }
            };
            (pending.token, matches)
        }) else {
            return false;
        };
        matches && self.complete_control(token) == SessionControlCompletion::Drained
    }

    /// Resolve an Agent intent only once the matching AgentChanged projection
    /// arrives. The RPC completion is merely transport correlation.
    pub(crate) fn resolve_agent_control(&mut self, agent_name: &str) -> bool {
        let Some((token, matches)) = self.controls.agent.in_flight.as_ref().map(|pending| {
            let matches = matches!(
                &pending.control,
                PendingSessionControl::Agent { agent_name: desired } if desired == agent_name
            );
            (pending.token, matches)
        }) else {
            return false;
        };
        matches && self.complete_control(token) == SessionControlCompletion::Drained
    }

    pub(crate) fn apply_shell_control_state_outcome(
        &mut self,
        update: shell::extensions::notification::ControlStateUpdate,
        allow_snapshot_reset: bool,
    ) -> ShellControlApplyOutcome {
        use shell::extensions::notification::ControlPhase;
        fn phase_rank(phase: ControlPhase) -> u8 {
            match phase {
                ControlPhase::Pending => 0,
                ControlPhase::Applying => 1,
                ControlPhase::Applied | ControlPhase::Rejected | ControlPhase::Superseded => 2,
            }
        }

        if update.current.domain() != update.domain
            || update
                .desired
                .as_ref()
                .is_some_and(|target| target.domain() != update.domain)
        {
            tracing::warn!(?update.domain, "ignoring mismatched control-state target");
            return ShellControlApplyOutcome::Rejected;
        }
        if self.shell_controls.active_epoch.as_deref() != Some(update.epoch.as_str()) {
            // Receipt-only projections are historical acknowledgements, not a
            // live state stream. They must never establish or rotate the
            // epoch (the ACP handler handles their immutable notice path).
            if update.receipt_only {
                return ShellControlApplyOutcome::Rejected;
            }
            // Only an explicit snapshot (or the bounded load replay window)
            // may establish or rotate authority. Accepting an arbitrary first
            // live packet lets a delayed update from an old actor incarnation
            // capture a freshly-created view that reuses the same session ID.
            if !allow_snapshot_reset && !update.snapshot {
                tracing::debug!(
                    update_epoch = %update.epoch,
                    active_epoch = ?self.shell_controls.active_epoch,
                    "ignoring live control update from unknown epoch"
                );
                return ShellControlApplyOutcome::Rejected;
            }
            if self.shell_controls.retired_epochs.contains(&update.epoch) {
                return ShellControlApplyOutcome::Rejected;
            }
            if let Some(previous) = self
                .shell_controls
                .active_epoch
                .replace(update.epoch.clone())
            {
                self.shell_controls.retired_epochs.insert(previous);
            }
            self.shell_controls.sampling = None;
            self.shell_controls.agent = None;
            self.shell_controls.behavior = None;
        }
        let slot = self.shell_controls.slot_mut(update.domain);
        if allow_snapshot_reset
            && update.snapshot
            && slot
                .as_ref()
                .is_some_and(|current| current.revision > update.revision)
        {
            // Durable terminal projections replay before the live actor is
            // asked for its state. A replacement actor starts a fresh local
            // revision sequence; only its explicit load snapshot may reset
            // the replayed high-water mark. Ordinary live updates and delayed
            // snapshots outside a replay window remain monotonic.
            *slot = None;
        }
        if update.snapshot
            && !allow_snapshot_reset
            && slot.as_ref().is_some_and(|current| {
                current.revision == update.revision
                    && (current.phase != update.phase
                        || current.intent != update.intent
                        || current.current != update.current
                        || current.desired != update.desired)
            })
        {
            return ShellControlApplyOutcome::Rejected;
        }
        if slot.as_ref().is_some_and(|current| {
            current.revision > update.revision
                || (current.revision == update.revision
                    && (current.terminal_message.is_some()
                        || phase_rank(current.phase) > phase_rank(update.phase)))
        }) {
            return ShellControlApplyOutcome::Rejected;
        }
        let phase_since = slot
            .as_ref()
            .filter(|current| {
                current.revision == update.revision
                    && current.intent == update.intent
                    && current.phase == update.phase
            })
            .map_or_else(Instant::now, |current| current.phase_since);
        let changed = slot.as_ref().is_none_or(|current| {
            current.revision != update.revision
                || current.intent != update.intent
                || current.phase != update.phase
                || current.current != update.current
                || current.desired != update.desired
                || current.terminal_message != update.message
        });
        *slot = Some(ShellControlSlot {
            revision: update.revision,
            intent: update.intent,
            phase: update.phase,
            current: update.current,
            desired: update.desired,
            terminal_message: update.message,
            phase_since,
        });
        ShellControlApplyOutcome::Accepted { changed }
    }

    #[cfg(test)]
    pub(crate) fn apply_shell_control_state(
        &mut self,
        update: shell::extensions::notification::ControlStateUpdate,
        allow_snapshot_reset: bool,
    ) -> bool {
        self.apply_shell_control_state_outcome(update, allow_snapshot_reset)
            .changed()
    }

    /// Newest complete Sampling target, including a switch staged before the
    /// ACP session exists.  User intents such as `/effort` must compose with
    /// this target instead of the last committed model; otherwise changing
    /// effort while a model switch is pending silently resurrects the old
    /// model.
    pub(crate) fn sampling_control_target(
        &self,
    ) -> Option<(shell::agent::models::ModelId, Option<ReasoningEffort>)> {
        let pending = self.controls.sampling.in_flight.as_ref();
        pending
            .and_then(|pending| match &pending.control {
                PendingSessionControl::Model {
                    model_id, effort, ..
                } => Some((model_id.clone(), *effort)),
                PendingSessionControl::Agent { .. } | PendingSessionControl::Behavior { .. } => {
                    None
                }
            })
            .or_else(|| self.deferred_model_switch.clone())
    }

    /// Model projection used only while interpreting a new UI control intent.
    /// It is deliberately separate from `self.models`: committed footer and
    /// prompt metadata must continue to show the authoritative current model
    /// until the Shell publishes the applied transition.
    pub(crate) fn models_for_control_intent(&self) -> ModelState {
        let mut models = self.models.clone();
        if let Some((model_id, effort)) = self.sampling_control_target() {
            models.set_current(model_id, effort);
        }
        models
    }

    pub(crate) fn agent_control_pending(&self) -> bool {
        let slot = &self.controls.agent;
        slot.in_flight.is_some()
    }

    /// Whether a Shell-authoritative control projection contains time-based
    /// presentation state. Pending controls animate their spinner; Applying
    /// controls also cross the 300ms feedback threshold without requiring an
    /// unrelated session event to trigger a redraw.
    pub(crate) fn control_feedback_active(&self) -> bool {
        use shell::extensions::notification::ControlPhase;
        self.live_feedback.is_some()
            || [
                self.shell_controls.sampling.as_ref(),
                self.shell_controls.agent.as_ref(),
                self.shell_controls.behavior.as_ref(),
            ]
            .into_iter()
            .flatten()
            .any(|slot| matches!(slot.phase, ControlPhase::Pending | ControlPhase::Applying))
    }

    pub(crate) fn set_live_feedback(
        &mut self,
        key: &'static str,
        tone: crate::scrollback::blocks::NoticeTone,
        message: impl Into<String>,
    ) {
        self.live_feedback = Some((
            key,
            crate::scrollback::blocks::UiFeedback::new(tone, message),
        ));
    }

    pub(crate) fn clear_live_feedback(&mut self, key: &'static str) {
        if self
            .live_feedback
            .as_ref()
            .is_some_and(|(active, _)| *active == key)
        {
            self.live_feedback = None;
        }
    }

    pub(crate) fn live_status(&self, width: usize) -> Option<String> {
        let control = self.control_status(width);
        let progress = self
            .live_feedback
            .as_ref()
            .map(|(_, feedback)| feedback.as_str());
        match (progress, control) {
            (None, None) => None,
            (Some(progress), None) => Some(progress.to_owned()),
            (None, Some(control)) => Some(control),
            (Some(progress), Some(control)) => {
                let combined = format!("{progress} · {control}");
                if UnicodeWidthStr::width(combined.as_str()) <= width {
                    Some(combined)
                } else {
                    Some(format!(
                        "{progress} · {} changes pending",
                        self.pending_control_count()
                    ))
                }
            }
        }
    }

    fn pending_control_count(&self) -> usize {
        use shell::extensions::notification::ControlPhase;
        [
            self.shell_controls.sampling.as_ref(),
            self.shell_controls.agent.as_ref(),
            self.shell_controls.behavior.as_ref(),
        ]
        .into_iter()
        .flatten()
        .filter(|slot| matches!(slot.phase, ControlPhase::Pending | ControlPhase::Applying))
        .count()
    }

    /// Compact projection of the newest target in each pending control domain.
    /// This is presentation-only state: it is never copied into ChatState or
    /// an ACP prompt. Both retained and minimal renderers consume this method.
    pub(crate) fn control_status(&self, width: usize) -> Option<String> {
        use shell::extensions::notification::{ControlDomain, ControlPhase, ControlTarget};
        fn short_model(model_id: &str) -> &str {
            model_id
                .rsplit_once('/')
                .map_or(model_id, |(_, model)| model)
        }
        fn behavior_label(behavior_id: &str) -> String {
            tools::types::BehaviorId::try_from_id(behavior_id).map_or_else(
                || behavior_id.to_string(),
                |mode| mode.display_label().to_string(),
            )
        }
        let label = |domain| {
            let slot = self.shell_controls.slot(domain)?;
            if !matches!(slot.phase, ControlPhase::Pending | ControlPhase::Applying) {
                return None;
            }
            let target = slot.desired.as_ref()?;
            let text = match (&slot.current, target) {
                (
                    ControlTarget::Sampling {
                        model_id: current_model,
                        reasoning_effort: current_effort,
                    },
                    ControlTarget::Sampling {
                        model_id: desired_model,
                        reasoning_effort: desired_effort,
                    },
                ) if current_model == desired_model && current_effort != desired_effort => format!(
                    "effort {}→{}",
                    current_effort.as_deref().unwrap_or("default"),
                    desired_effort.as_deref().unwrap_or("default")
                ),
                (
                    ControlTarget::Sampling {
                        model_id: current_model,
                        ..
                    },
                    ControlTarget::Sampling {
                        model_id: desired_model,
                        reasoning_effort,
                    },
                ) => {
                    let current_short = short_model(current_model);
                    let desired_short = short_model(desired_model);
                    let (current_label, desired_label) =
                        if current_model != desired_model && current_short == desired_short {
                            (current_model.as_str(), desired_model.as_str())
                        } else {
                            (current_short, desired_short)
                        };
                    let transition = format!("model {}→{}", current_label, desired_label);
                    match reasoning_effort {
                        Some(effort) => format!("{transition} ({effort})"),
                        None => transition,
                    }
                }
                (
                    ControlTarget::Agent {
                        agent_name: current,
                    },
                    ControlTarget::Agent {
                        agent_name: desired,
                    },
                ) => format!("agent {current}→{desired}"),
                (
                    ControlTarget::Behavior {
                        behavior_id: current,
                    },
                    ControlTarget::Behavior {
                        behavior_id: desired,
                    },
                ) => format!(
                    "behavior {}→{}",
                    behavior_label(current),
                    behavior_label(desired)
                ),
                _ => return None,
            };
            Some(
                if slot.phase == ControlPhase::Applying
                    && slot.phase_since.elapsed() >= Duration::from_millis(300)
                {
                    format!("applying {text}")
                } else {
                    text
                },
            )
        };
        let parts = [
            label(ControlDomain::Sampling),
            label(ControlDomain::Agent),
            label(ControlDomain::Behavior),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        match parts.as_slice() {
            [] => None,
            _ => {
                let verbose = parts.join(" · ");
                if UnicodeWidthStr::width(verbose.as_str()) <= width {
                    Some(verbose)
                } else {
                    let noun = if parts.len() == 1 {
                        "change"
                    } else {
                        "changes"
                    };
                    Some(format!("{} {noun} pending", parts.len()))
                }
            }
        }
    }

    pub(crate) fn agent_switch_in_flight(&self) -> bool {
        self.controls
            .agent
            .in_flight
            .as_ref()
            .is_some_and(|pending| matches!(pending.control, PendingSessionControl::Agent { .. }))
    }

    /// The newest locally correlated Behavior request. This is the sole
    /// optimistic Behavior projection; authoritative
    /// `CurrentModeUpdate` remains the only committed state transition.
    pub(crate) fn behavior_control_target(&self) -> Option<tools::types::BehaviorId> {
        let slot = &self.controls.behavior;
        slot.in_flight
            .iter()
            .find_map(|pending| match &pending.control {
                PendingSessionControl::Behavior { mode } => Some(*mode),
                PendingSessionControl::Model { .. } | PendingSessionControl::Agent { .. } => None,
            })
    }

    pub(crate) fn effective_behavior(&self) -> tools::types::BehaviorId {
        self.behavior_control_target()
            .or(self.deferred_session_mode)
            .unwrap_or(self.behavior_mode)
    }

    pub(crate) fn effective_plan_mode(&self) -> bool {
        self.effective_behavior().is_plan()
    }

    /// Consume the in-flight Behavior control only when the authoritative
    /// session update resolves it. A plain unrelated CurrentModeUpdate never
    /// advances the desired-state domain.
    pub(crate) fn resolve_in_flight_behavior(
        &mut self,
        observed: tools::types::BehaviorId,
        resolution: BehaviorControlResolution,
        outcome_target: Option<tools::types::BehaviorId>,
    ) -> Option<SessionControlCompletion> {
        let (token, target) = match self.controls.behavior.in_flight.as_ref()? {
            InFlightSessionControl {
                token,
                control: PendingSessionControl::Behavior { mode },
                ..
            } => (*token, *mode),
            _ => return None,
        };
        let settled = match resolution {
            BehaviorControlResolution::Applied => target == observed,
            BehaviorControlResolution::Rejected
            | BehaviorControlResolution::ConfirmationRequired => outcome_target == Some(target),
        };
        settled.then(|| self.complete_control(token))
    }

    #[cfg(test)]
    pub(crate) fn model_switch_pending(&self) -> bool {
        self.sampling_control_pending()
    }

    #[cfg(test)]
    pub(crate) fn agent_switch_target(&self) -> Option<&str> {
        let slot = &self.controls.agent;
        slot.in_flight
            .iter()
            .find_map(|pending| match &pending.control {
                PendingSessionControl::Agent { agent_name } => Some(agent_name.as_str()),
                PendingSessionControl::Model { .. } | PendingSessionControl::Behavior { .. } => {
                    None
                }
            })
    }

    #[cfg(test)]
    pub(crate) fn begin_model_switch_for_test(&mut self) -> SessionControlToken {
        let _ = self.enqueue_control(PendingSessionControl::Model {
            model_id: shell::agent::models::ModelId::new("test-model-control"),
            effort: None,
            effort_patch: false,
        });
        self.controls
            .sampling
            .in_flight
            .as_ref()
            .expect("test model control was admitted")
            .token
    }

    #[cfg(test)]
    pub(crate) fn begin_agent_switch(&mut self, name: impl Into<String>) -> SessionControlToken {
        let _ = self.enqueue_control(PendingSessionControl::Agent {
            agent_name: name.into(),
        });
        self.controls
            .agent
            .in_flight
            .as_ref()
            .expect("test Agent control was admitted")
            .token
    }

    #[cfg(test)]
    pub(crate) fn begin_behavior_switch(
        &mut self,
        mode: tools::types::BehaviorId,
    ) -> SessionControlToken {
        let _ = self.enqueue_control(PendingSessionControl::Behavior { mode });
        let slot = &self.controls.behavior;
        slot.in_flight
            .as_ref()
            .expect("test Behavior control was admitted")
            .token
    }

    #[cfg(test)]
    pub(crate) fn complete_agent_switch(&mut self) -> bool {
        let Some(token) = self.controls.agent.in_flight.as_ref().and_then(|pending| {
            matches!(pending.control, PendingSessionControl::Agent { .. }).then_some(pending.token)
        }) else {
            return false;
        };
        self.complete_control(token) != SessionControlCompletion::Stale
    }

    #[cfg(test)]
    pub(crate) fn current_control_token_for_test(&self) -> SessionControlToken {
        self.controls
            .slots()
            .into_iter()
            .filter_map(|slot| slot.in_flight.as_ref())
            .min_by_key(|pending| pending.token.sequence)
            .expect("test requires an in-flight session control")
            .token
    }

    pub(crate) fn prompt_status_query_matches(&self, prompt_id: &str) -> bool {
        self.prompt_status_query_for.as_deref() == Some(prompt_id)
    }

    pub(crate) fn begin_prompt_status_query(&mut self, prompt_id: impl Into<String>) {
        self.prompt_status_query_for = Some(prompt_id.into());
    }

    pub(crate) fn clear_prompt_status_query(&mut self) {
        self.prompt_status_query_for = None;
    }

    pub(crate) fn mark_optimistic_queue_echo(&mut self, prompt_id: impl Into<String>) {
        self.optimistic_queue_ids.insert(prompt_id.into());
    }

    pub(crate) fn has_optimistic_queue_echo(&self, prompt_id: &str) -> bool {
        self.optimistic_queue_ids.contains(prompt_id)
    }

    pub(crate) fn park_send_now_until_queue_confirmation(&mut self, prompt_id: String) {
        self.send_now_awaiting_confirm = Some(prompt_id);
    }

    /// Reconcile optimistic echoes against the raw, pre-merge
    /// `grow/queue/changed` entries and resolve a parked queue-row send-now.
    /// The mirrored queue cannot be used here because it re-pins unconfirmed
    /// echoes and therefore cannot distinguish an echo from confirmation.
    ///
    /// Returns `Some((id, version))` when the parked row is now confirmed as
    /// QUEUED. A parked row confirmed as RUNNING clears the park with nothing
    /// to do (the natural drain won the race). A row in neither set stays
    /// parked (its RPC is still in flight).
    pub(crate) fn resolve_send_now_awaiting_confirm(
        &mut self,
        broadcast_entries: &[(String, u64)],
        running_prompt_id: Option<&str>,
    ) -> Option<(String, u64)> {
        self.optimistic_queue_ids.retain(|id| {
            running_prompt_id != Some(id.as_str())
                && !broadcast_entries.iter().any(|(eid, _)| eid == id)
        });
        let awaiting = self.send_now_awaiting_confirm.as_deref()?;
        if running_prompt_id == Some(awaiting) {
            self.send_now_awaiting_confirm = None;
            return None;
        }
        if let Some((id, version)) = broadcast_entries.iter().find(|(eid, _)| eid == awaiting) {
            self.send_now_awaiting_confirm = None;
            return Some((id.clone(), *version));
        }
        None
    }

    /// A server-queue echo resolved without landing (RPC failure, removal, or
    /// cancellation): forget it and any parked send-now intent because there
    /// is no row left to promote.
    pub(crate) fn note_queue_echo_retired(&mut self, prompt_id: &str) {
        self.optimistic_queue_ids.remove(prompt_id);
        if self.send_now_awaiting_confirm.as_deref() == Some(prompt_id) {
            self.send_now_awaiting_confirm = None;
        }
    }

    pub(crate) fn clear_queue_echo_state(&mut self) {
        self.optimistic_queue_ids.clear();
        self.send_now_awaiting_confirm = None;
    }

    /// Update context state with a full snapshot from live callers.
    pub(crate) fn apply_full_context_info(&mut self, next: shell::session::ContextInfo) {
        self.context_state = Some(next);
    }

    /// Update context state from a streaming notification carrying only
    /// `used` and `total` fields.
    pub(crate) fn apply_context_used(&mut self, used: u64, total: u64) {
        let total = if total > 0 {
            total
        } else {
            self.context_state.as_ref().map(|s| s.total).unwrap_or(0)
        };
        match self.context_state.as_mut() {
            Some(snap) => {
                snap.used = used;
                if total > 0 {
                    snap.total = total;
                }
                snap.usage_pct = token_estimation::usage_percentage_u8(used, snap.total);
                snap.free_tokens = token_estimation::free_tokens(snap.total, used);
            }
            None => {
                self.context_state =
                    Some(shell::session::ContextInfo::from_notification(used, total));
            }
        }
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

    /// Whether this session currently requires user input.
    pub(crate) fn needs_input(&self) -> bool {
        self.pending_permission_count > 0 || self.question_pending
    }

    pub(crate) fn mcp_init_progress(&self) -> Option<&McpInitProgress> {
        self.mcp_init_progress.as_ref()
    }

    pub(crate) fn update_mcp_init_progress(&mut self, total: u32, connected: u32) {
        match self.mcp_init_progress.as_mut() {
            Some(progress) => {
                progress.total = total;
                progress.connected = connected;
            }
            None => {
                self.mcp_init_progress = Some(McpInitProgress {
                    total,
                    connected,
                    started_at: Instant::now(),
                });
            }
        }
    }

    pub(crate) fn clear_mcp_init_progress(&mut self) -> bool {
        self.mcp_init_progress.take().is_some()
    }

    pub(crate) fn set_pending_extensions_fetch(&mut self) {
        self.pending_extensions_fetch = true;
    }

    pub(crate) fn take_pending_extensions_fetch(&mut self) -> bool {
        std::mem::take(&mut self.pending_extensions_fetch)
    }

    pub(crate) fn clear_pending_extensions_fetch(&mut self) {
        self.pending_extensions_fetch = false;
    }

    #[cfg(test)]
    pub(crate) fn pending_extensions_fetch(&self) -> bool {
        self.pending_extensions_fetch
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
    /// 1. Both tool and command discovery are still empty, or the published
    ///    tool snapshot contains the `workflow` tool. A non-empty command
    ///    catalog is already a bootstrap capability signal, so it must not be
    ///    overridden merely because the separate tool snapshot is pending.
    /// 2. A Workflow runtime slash command is advertised.
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
        let discovery_unknown = available_tools.is_none() && available_commands.is_empty();
        let has_workflow_tool =
            available_tools.is_some_and(|tools| tools.contains(WORKFLOW_TOOL_NAME));
        let has_workflow_command = available_commands
            .iter()
            .any(|c| c.name == WORKFLOW_TOOL_NAME || c.name == WORKFLOW_RUN_COMMAND_NAME);
        discovery_unknown || has_workflow_tool || has_workflow_command || has_workflow_runs
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
    fn workflows_unknown_is_optimistically_visible_but_known_empty_is_not() {
        // (c) No tool, no command, no runs → an unknown tool snapshot remains
        // enterable; an authoritative empty snapshot does not.
        assert!(AgentSession::bootstrap_workflow_support(None, &[], false));
        assert!(!AgentSession::bootstrap_workflow_support(
            Some(&HashSet::new()),
            &[],
            false
        ));
        let unrelated = [acp::AvailableCommand::new(
            "goal".to_string(),
            "Manage Goal".to_string(),
        )];
        assert!(
            !AgentSession::bootstrap_workflow_support(None, &unrelated, false),
            "a known non-Workflow command catalog must not be treated as total discovery absence"
        );
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
        assert_eq!(GoalDisplayStatus::Active.stopped_label(), "");
        assert_eq!(GoalDisplayStatus::BudgetLimited.stopped_label(), "");
        assert_eq!(GoalDisplayStatus::Complete.stopped_label(), "");
        assert!(GoalDisplayStatus::Paused.uses_warning_chip());
        assert!(GoalDisplayStatus::Blocked.uses_warning_chip());
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

    #[test]
    fn behavior_controls_replace_local_correlation_with_the_latest_target() {
        let mut s = test_session();
        let first = s
            .enqueue_control(PendingSessionControl::Behavior {
                mode: tools::types::BehaviorId::Plan,
            })
            .expect("first Behavior is in flight");
        let latest = s
            .enqueue_control(PendingSessionControl::Behavior {
                mode: tools::types::BehaviorId::Goal,
            })
            .expect("latest Behavior is dispatched immediately");
        assert_eq!(s.effective_behavior(), tools::types::BehaviorId::Goal);

        assert_eq!(
            s.resolve_in_flight_behavior(
                tools::types::BehaviorId::Plan,
                BehaviorControlResolution::Applied,
                None,
            ),
            None,
            "the superseded Plan response cannot consume Goal correlation"
        );
        assert_eq!(s.effective_behavior(), tools::types::BehaviorId::Goal);

        // A delayed terminal outcome for Plan must not consume the newer Goal
        // request that has just become in-flight.
        assert_eq!(
            s.resolve_in_flight_behavior(
                tools::types::BehaviorId::Plan,
                BehaviorControlResolution::Rejected,
                Some(tools::types::BehaviorId::Plan),
            ),
            None
        );
        assert_eq!(
            s.resolve_in_flight_behavior(
                tools::types::BehaviorId::Plan,
                BehaviorControlResolution::Rejected,
                Some(tools::types::BehaviorId::Goal),
            ),
            Some(SessionControlCompletion::Drained)
        );
        assert!(!s.controls_pending());
        assert!(matches!(
            first.1,
            PendingSessionControl::Behavior {
                mode: tools::types::BehaviorId::Plan
            }
        ));
        assert!(matches!(
            latest.1,
            PendingSessionControl::Behavior {
                mode: tools::types::BehaviorId::Goal
            }
        ));
    }

    #[test]
    fn reconnect_rearms_only_the_latest_sampling_target() {
        let mut s = test_session();
        let model_id = shell::agent::models::ModelId::new("grow-test");
        s.models.available.insert(
            model_id.clone(),
            shell::agent::models::ModelInfo::new(model_id.clone(), "Grow Test".to_owned()),
        );
        s.models.set_current(model_id.clone(), None);
        let old = s
            .enqueue_control(PendingSessionControl::Model {
                model_id: model_id.clone(),
                effort: None,
                effort_patch: false,
            })
            .expect("first control is in flight")
            .0;
        let latest = s
            .enqueue_control(PendingSessionControl::Model {
                model_id: model_id.clone(),
                effort: None,
                effort_patch: false,
            })
            .expect("latest Sampling target is dispatched immediately")
            .0;
        assert_ne!(old, latest);

        s.rearm_controls_for_reconnect();
        assert_eq!(
            s.complete_control(old),
            SessionControlCompletion::Stale,
            "old-transport terminal must not mutate the rearmed queue"
        );
        let retry = s
            .claim_control_for_dispatch()
            .map(|(token, _)| token)
            .expect("latest intent is reissued on the replacement transport");
        assert_eq!(retry.client_id, latest.client_id);
        assert_eq!(retry.generation, latest.generation);
        assert_eq!(retry.sequence, latest.sequence);
        assert_ne!(
            retry.dispatch_generation, latest.dispatch_generation,
            "the semantic intent stays stable while the transport epoch advances"
        );
    }

    #[test]
    fn effort_intent_projects_the_latest_pending_model_without_committing_it() {
        let mut s = test_session();
        let old = shell::agent::models::ModelId::new("provider/old");
        let pending = shell::agent::models::ModelId::new("provider/pending");
        s.models.available.insert(
            old.clone(),
            shell::agent::models::ModelInfo::new(old.clone(), "Old".to_owned()),
        );
        s.models.available.insert(
            pending.clone(),
            shell::agent::models::ModelInfo::new(pending.clone(), "Pending".to_owned()).meta(
                serde_json::json!({
                    "reasoningEfforts": ["low", "high"],
                    "reasoningEffort": "high"
                })
                .as_object()
                .cloned(),
            ),
        );
        s.models.set_current(old.clone(), None);
        s.enqueue_control(PendingSessionControl::Model {
            model_id: pending.clone(),
            effort: None,
            effort_patch: false,
        })
        .expect("pending model is dispatched");

        let intent_models = s.models_for_control_intent();
        assert_eq!(intent_models.current, Some(pending));
        assert_eq!(intent_models.reasoning_effort, Some(ReasoningEffort::High));
        assert_eq!(
            s.models.current,
            Some(old),
            "committed projection is unchanged"
        );
        assert_eq!(
            intent_models
                .resolve_effort_for_model(intent_models.current.as_ref().unwrap(), "low")
                .unwrap(),
            ReasoningEffort::Low
        );
    }

    #[test]
    fn reconnect_never_inferrs_behavior_confirmation_from_mode_equality() {
        let mut s = test_session();
        s.enqueue_control(PendingSessionControl::Behavior {
            mode: tools::types::BehaviorId::Normal,
        })
        .expect("Behavior control is in flight");

        s.rearm_controls_for_reconnect();

        assert!(
            matches!(
                s.claim_control_for_dispatch(),
                Some((_, PendingSessionControl::Behavior { mode }))
                    if mode == tools::types::BehaviorId::Normal
            ),
            "same-mode selection must be reissued to clear the Shell latch"
        );
    }

    #[test]
    fn reconnect_agent_terminal_resolves_only_the_exact_intent() {
        use shell::extensions::notification::{ControlDomain, ControlPhase, ControlTarget};

        let mut s = test_session();
        let token = s
            .enqueue_control(PendingSessionControl::Agent {
                agent_name: "reviewer".into(),
            })
            .expect("Agent control is in flight")
            .0;
        s.rearm_controls_for_reconnect();
        let current = ControlTarget::Agent {
            agent_name: "reviewer".into(),
        };
        let mut stale = token.shell_intent();
        stale.sequence = stale.sequence.saturating_add(1);
        assert!(!s.resolve_reconnect_control_projection(
            ControlDomain::Agent,
            ControlPhase::Applied,
            &current,
            None,
            Some(&stale),
        ));
        assert!(s.agent_control_pending());
        assert!(s.resolve_reconnect_control_projection(
            ControlDomain::Agent,
            ControlPhase::Applied,
            &current,
            None,
            Some(&token.shell_intent()),
        ));
        assert!(!s.agent_control_pending());
    }

    #[test]
    fn reconnect_applied_receipt_matches_its_target_after_current_advances() {
        use shell::extensions::notification::{ControlDomain, ControlPhase, ControlTarget};

        let mut s = test_session();
        let token = s
            .enqueue_control(PendingSessionControl::Agent {
                agent_name: "reviewer".into(),
            })
            .expect("Agent control is in flight")
            .0;
        s.rearm_controls_for_reconnect();
        let later_current = ControlTarget::Agent {
            agent_name: "coder".into(),
        };
        let applied_target = ControlTarget::Agent {
            agent_name: "reviewer".into(),
        };

        assert!(s.resolve_reconnect_control_projection(
            ControlDomain::Agent,
            ControlPhase::Applied,
            &later_current,
            Some(&applied_target),
            Some(&token.shell_intent()),
        ));
        assert!(!s.agent_control_pending());
    }

    #[test]
    fn reconnect_behavior_waits_for_an_authoritative_terminal_projection() {
        use shell::extensions::notification::{ControlDomain, ControlPhase, ControlTarget};

        let mut s = test_session();
        let token = s
            .enqueue_control(PendingSessionControl::Behavior {
                mode: tools::types::BehaviorId::Plan,
            })
            .expect("Behavior control is in flight")
            .0;
        s.rearm_controls_for_reconnect();
        let current = ControlTarget::Behavior {
            behavior_id: "normal".into(),
        };
        let desired = ControlTarget::Behavior {
            behavior_id: "plan".into(),
        };

        assert!(!s.resolve_reconnect_control_projection(
            ControlDomain::Behavior,
            ControlPhase::Pending,
            &current,
            Some(&desired),
            Some(&token.shell_intent()),
        ));
        assert!(
            s.controls_pending(),
            "pending confirmation must stay fenced"
        );

        assert!(s.resolve_reconnect_control_projection(
            ControlDomain::Behavior,
            ControlPhase::Applied,
            &desired,
            None,
            Some(&token.shell_intent()),
        ));
        assert!(
            !s.controls_pending(),
            "applied snapshot is terminal authority"
        );
    }

    #[test]
    fn shell_control_status_combines_domains_without_committing_targets() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut s = test_session();
        let sampling_current = ControlTarget::Sampling {
            model_id: "provider/old".into(),
            reasoning_effort: None,
        };
        assert!(s.apply_shell_control_state(
            ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Sampling,
                revision: 1,
                intent: None,
                snapshot: true,
                receipt_only: false,
                phase: ControlPhase::Pending,
                current: sampling_current,
                desired: Some(ControlTarget::Sampling {
                    model_id: "provider/new".into(),
                    reasoning_effort: Some("high".into()),
                }),
                message: None,
            },
            false
        ));
        assert!(s.apply_shell_control_state(
            ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Agent,
                revision: 4,
                intent: None,
                snapshot: false,
                receipt_only: false,
                phase: ControlPhase::Pending,
                current: ControlTarget::Agent {
                    agent_name: "builder".into(),
                },
                desired: Some(ControlTarget::Agent {
                    agent_name: "reviewer".into(),
                }),
                message: None,
            },
            false
        ));
        assert!(s.apply_shell_control_state(
            ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Behavior,
                revision: 2,
                intent: None,
                snapshot: false,
                receipt_only: false,
                phase: ControlPhase::Pending,
                current: ControlTarget::Behavior {
                    behavior_id: "normal".into(),
                },
                desired: Some(ControlTarget::Behavior {
                    behavior_id: "goal".into(),
                }),
                message: None,
            },
            false
        ));

        let wide = s.control_status(100).expect("pending status");
        assert!(wide.contains("model old→new (high)"), "{wide}");
        assert!(wide.contains("agent builder→reviewer"), "{wide}");
        assert!(wide.contains("behavior Normal→Goal"), "{wide}");
        assert_eq!(s.control_status(40).as_deref(), Some("3 changes pending"));
        s.set_live_feedback(
            "compaction",
            crate::scrollback::blocks::NoticeTone::Progress,
            "Compacting…",
        );
        assert_eq!(
            s.live_status(40).as_deref(),
            Some("Compacting… · 3 changes pending")
        );
        assert_eq!(
            s.models.current.as_ref().map(|model| model.0.as_ref()),
            None,
            "presentation desired state must never become committed model state"
        );
    }

    #[test]
    fn shell_control_projection_rejects_stale_phases_and_hides_terminals() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut s = test_session();
        let update = |revision, phase, desired: Option<&str>| ControlStateUpdate {
            epoch: "epoch-a".into(),
            domain: ControlDomain::Agent,
            revision,
            intent: None,
            snapshot: revision == 2 && phase == ControlPhase::Applying,
            receipt_only: false,
            phase,
            current: ControlTarget::Agent {
                agent_name: "builder".into(),
            },
            desired: desired.map(|name| ControlTarget::Agent {
                agent_name: name.into(),
            }),
            message: None,
        };
        assert!(
            s.apply_shell_control_state(update(2, ControlPhase::Applying, Some("reviewer")), false)
        );
        s.shell_controls
            .agent
            .as_mut()
            .expect("Agent projection")
            .phase_since = Instant::now() - Duration::from_millis(301);
        assert_eq!(
            s.control_status(100).as_deref(),
            Some("applying agent builder→reviewer")
        );
        assert!(
            !s.apply_shell_control_state(update(1, ControlPhase::Applied, Some("stale")), false)
        );
        assert!(
            !s.apply_shell_control_state(
                update(2, ControlPhase::Pending, Some("regressed")),
                false
            )
        );
        assert!(s.apply_shell_control_state(update(2, ControlPhase::Applied, None), false));
        assert_eq!(s.control_status(100), None);
    }

    #[test]
    fn shell_control_status_describes_effort_only_transition() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut s = test_session();
        assert!(s.apply_shell_control_state(
            ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Sampling,
                revision: 1,
                intent: None,
                snapshot: true,
                receipt_only: false,
                phase: ControlPhase::Pending,
                current: ControlTarget::Sampling {
                    model_id: "provider/model".into(),
                    reasoning_effort: Some("high".into()),
                },
                desired: Some(ControlTarget::Sampling {
                    model_id: "provider/model".into(),
                    reasoning_effort: Some("max".into()),
                }),
                message: None,
            },
            false,
        ));
        assert_eq!(s.control_status(100).as_deref(), Some("effort high→max"));
        assert_eq!(s.control_status(5).as_deref(), Some("1 change pending"));
    }

    #[test]
    fn shell_control_status_preserves_provider_when_model_names_match() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut s = test_session();
        assert!(s.apply_shell_control_state(
            ControlStateUpdate {
                epoch: "epoch-a".into(),
                domain: ControlDomain::Sampling,
                revision: 1,
                intent: None,
                snapshot: true,
                receipt_only: false,
                phase: ControlPhase::Pending,
                current: ControlTarget::Sampling {
                    model_id: "bigmodel/glm-5.3".into(),
                    reasoning_effort: None,
                },
                desired: Some(ControlTarget::Sampling {
                    model_id: "volcengine/glm-5.3".into(),
                    reasoning_effort: None,
                }),
                message: None,
            },
            false,
        ));
        assert_eq!(
            s.control_status(100).as_deref(),
            Some("model bigmodel/glm-5.3→volcengine/glm-5.3")
        );
    }

    #[test]
    fn durable_control_terminal_seals_its_revision_but_may_follow_a_snapshot() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut s = test_session();
        let update = |phase, desired: Option<&str>, message: Option<&str>| ControlStateUpdate {
            epoch: "epoch-a".into(),
            domain: ControlDomain::Agent,
            revision: 7,
            intent: None,
            snapshot: message.is_none(),
            receipt_only: false,
            phase,
            current: ControlTarget::Agent {
                agent_name: "builder".into(),
            },
            desired: desired.map(|name| ControlTarget::Agent {
                agent_name: name.into(),
            }),
            message: message.map(str::to_owned),
        };

        assert!(s.apply_shell_control_state(update(ControlPhase::Applied, None, None), false));
        assert!(s.apply_shell_control_state(
            update(
                ControlPhase::Rejected,
                Some("reviewer"),
                Some("Agent switch failed")
            ),
            false
        ));
        assert!(
            !s.apply_shell_control_state(update(ControlPhase::Applied, None, None), false),
            "a later reconnect snapshot must not rewrite a durable terminal"
        );
        assert!(
            !s.apply_shell_control_state(
                update(
                    ControlPhase::Applied,
                    Some("reviewer"),
                    Some("conflicting terminal")
                ),
                false
            ),
            "one revision has exactly one immutable terminal outcome"
        );
    }

    #[test]
    fn load_snapshot_may_reset_a_replayed_actor_local_revision() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let mut s = test_session();
        let update = |epoch: &str, revision, snapshot, name: &str| ControlStateUpdate {
            epoch: epoch.into(),
            domain: ControlDomain::Agent,
            revision,
            intent: None,
            snapshot,
            receipt_only: false,
            phase: ControlPhase::Applied,
            current: ControlTarget::Agent {
                agent_name: name.into(),
            },
            desired: None,
            message: None,
        };
        assert!(s.apply_shell_control_state(update("old", 9, true, "old"), false));
        assert!(
            s.apply_shell_control_state(update("replay", 0, false, "replayed"), true),
            "the bounded load replay may establish its replacement epoch"
        );
        assert_eq!(s.shell_controls.active_epoch.as_deref(), Some("replay"));
        assert!(s.apply_shell_control_state(update("fresh", 0, true, "fresh"), true));
        let slot = s.shell_controls.agent.as_ref().expect("Agent snapshot");
        assert_eq!(slot.revision, 0);
        assert_eq!(
            slot.current,
            ControlTarget::Agent {
                agent_name: "fresh".into()
            }
        );
        assert!(
            !s.apply_shell_control_state(update("old", 10, true, "delayed"), false),
            "an actor-local reset is legal only inside the load window"
        );
    }

    #[test]
    fn control_epoch_bootstraps_once_then_requires_snapshot_to_rotate() {
        use shell::extensions::notification::{
            ControlDomain, ControlPhase, ControlStateUpdate, ControlTarget,
        };

        let update =
            |epoch: &str, revision, snapshot, receipt_only, agent_name: &str| ControlStateUpdate {
                epoch: epoch.into(),
                domain: ControlDomain::Agent,
                revision,
                intent: None,
                snapshot,
                receipt_only,
                phase: ControlPhase::Applied,
                current: ControlTarget::Agent {
                    agent_name: agent_name.into(),
                },
                desired: None,
                message: Some(format!("Agent switched to {agent_name}")),
            };
        let mut session = test_session();

        assert!(
            !session
                .apply_shell_control_state(update("stale", 1, false, false, "stale-agent"), false,),
            "a fresh view must not let an arbitrary live packet establish authority"
        );
        assert!(
            session.apply_shell_control_state(update("epoch-a", 1, true, false, "a-agent"), false,)
        );
        assert_eq!(
            session.shell_controls.active_epoch.as_deref(),
            Some("epoch-a")
        );

        assert!(
            session.apply_shell_control_state(update("epoch-b", 7, true, false, "b-agent"), false)
        );
        assert!(
            !session
                .apply_shell_control_state(update("epoch-c", 1, false, false, "c-agent"), false)
        );
        assert_eq!(
            session.shell_controls.active_epoch.as_deref(),
            Some("epoch-b")
        );
        assert_eq!(
            session
                .shell_controls
                .agent
                .as_ref()
                .expect("epoch B projection")
                .revision,
            7
        );
        assert!(
            session
                .apply_shell_control_state(update("epoch-b", 8, false, false, "b-agent-2"), false)
        );
        assert_eq!(
            session.shell_controls.active_epoch.as_deref(),
            Some("epoch-b")
        );
        assert_eq!(session.shell_controls.agent.as_ref().unwrap().revision, 8);
        assert!(
            !session
                .apply_shell_control_state(update("epoch-c", 2, false, true, "historical"), false)
        );
        assert_eq!(
            session.shell_controls.active_epoch.as_deref(),
            Some("epoch-b")
        );
        assert!(
            session.apply_shell_control_state(update("epoch-c", 1, true, false, "c-agent"), false)
        );
        assert_eq!(
            session.shell_controls.active_epoch.as_deref(),
            Some("epoch-c")
        );
        assert!(
            !session.apply_shell_control_state(
                update("epoch-b", 9, false, false, "late-b-agent"),
                false
            ),
            "an update from a retired epoch must never reactivate it"
        );
        assert_eq!(
            session.shell_controls.active_epoch.as_deref(),
            Some("epoch-c")
        );
    }
}
