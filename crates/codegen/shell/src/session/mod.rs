pub mod acp_types;
pub(crate) mod actor;
pub mod announcement_state;
pub mod commands;
pub mod compaction_config;
pub mod control;
pub mod handle;
pub mod listing;
pub mod memory_state;
pub(crate) mod notification_inbox;
pub mod notifications;
pub mod pending_interaction;
pub mod prompt_queue;
pub(crate) mod subagent_capability;
pub use self::acp_types::*;
pub use self::actor::*;
pub use self::commands::*;
pub use self::fork::{ForkSessionRequest, ForkSessionResponse, fork_session};
pub use self::handle::*;
pub use self::persistence::{
    resolve_local_session, resolve_local_session_any_cwd, session_exists_for_cwd,
};
pub use self::result::{Empty, ExtMethodResult};
pub use fsnotify::{FsConfig, FsEvent, FsEventKind, FsEventSource, FsNotifyError, GitMetaKind};
/// Pull the `ContentBlock::Image`s out of a block list — the single spelling
/// of "only Image blocks ride structurally" (interject parse + queue-interject
/// harvest).
pub(crate) fn image_blocks(
    blocks: impl IntoIterator<Item = agent_client_protocol::ContentBlock>,
) -> Vec<agent_client_protocol::ImageContent> {
    blocks
        .into_iter()
        .filter_map(|block| match block {
            agent_client_protocol::ContentBlock::Image(img) => Some(img),
            _ => None,
        })
        .collect()
}
/// Structured origin of a regular turn.
///
/// Producers assign this value explicitly. Opaque prompt ids are identities,
/// not a second wire protocol for reconstructing lifecycle semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptOrigin {
    /// A normal user-initiated prompt.
    User,
    /// Auto-wake prompt injected when a background terminal task completed.
    TaskCompleted {
        /// The background task ID (without the `task-completed-` prefix).
        task_id: String,
    },
    /// Auto-wake prompt injected when a background subagent completed.
    SubagentCompleted {
        /// The subagent ID (without the `subagent-completed-` prefix).
        subagent_id: String,
    },
    WorkflowCompleted {
        completion_id: String,
    },
    /// Server-initiated prompt from the idle-gated notification drain
    /// (`maybe_drain_notifications`). Batches one or more monitor-event
    /// or bash-task-completed notifications into a single turn while the
    /// user is idle.
    NotificationDrain,
    /// Shell-owned slash command scheduled through the command plane. It may
    /// execute a finite prompt task for lifecycle consistency, but is never a
    /// user/model prompt and is hidden from conversation UI.
    HostCommand,
    /// Idle-admitted implementer continuation for an active Goal.
    GoalContinuation {
        goal_id: String,
    },
    /// Turn injected after a resumed plan-approval decision: the
    /// shell re-parked Plan approval on resume, the user approved/revised,
    /// and the shell injects the follow-up turn. Synthetic so the user never
    /// typed it — kept out of prompt history — but it still runs a real turn.
    PlanResume,
}

/// Participation of a regular turn in the user-visible lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    User,
    Internal,
}
impl PromptOrigin {
    pub fn turn_identity(&self, turn_kind: TurnKind) -> chat_state::TurnIdentity {
        let (goal_id, stage_id) = match self {
            Self::GoalContinuation { goal_id } => (Some(goal_id.clone()), None),
            _ => (None, None),
        };
        chat_state::TurnIdentity {
            origin: self.wire_name().to_string(),
            turn_kind: turn_kind.wire_name().to_string(),
            goal_id,
            stage_id,
        }
    }

    pub fn wire_name(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::TaskCompleted { .. } => "task_completed",
            Self::SubagentCompleted { .. } => "subagent_completed",
            Self::WorkflowCompleted { .. } => "workflow_completed",
            Self::NotificationDrain => "notification_drain",
            Self::HostCommand => "host_command",
            Self::GoalContinuation { .. } => "goal_continuation",
            Self::PlanResume => "plan_resume",
        }
    }
    /// Returns `true` for auto-wake (synthetic) prompts.
    pub fn is_synthetic(&self) -> bool {
        !matches!(self, Self::User)
    }
    /// Whether this foreground turn is owned by the Goal runtime rather than
    /// merely occurring while Goal Behavior is selected.
    pub fn is_goal_internal(&self) -> bool {
        matches!(self, Self::GoalContinuation { .. })
    }
    /// Synthetic wake work that a newer user prompt may replace.
    pub fn is_preemptible_wake(&self) -> bool {
        matches!(
            self,
            Self::TaskCompleted { .. }
                | Self::SubagentCompleted { .. }
                | Self::WorkflowCompleted { .. }
                | Self::NotificationDrain
        )
    }
    /// Whether a `UserMessageChunk` echo for this origin must stay out of
    /// client scrollback (live and on resume). Model-only / side-channel
    /// content — UI already surfaces it via task pane, monitor gutter, etc.
    ///
    /// Plan-resume follow-ups and real user turns still render.
    pub fn hide_user_echo_from_scrollback(&self) -> bool {
        match self {
            Self::User | Self::PlanResume => false,
            Self::TaskCompleted { .. }
            | Self::SubagentCompleted { .. }
            | Self::WorkflowCompleted { .. }
            | Self::NotificationDrain
            | Self::HostCommand
            | Self::GoalContinuation { .. } => true,
        }
    }
    pub fn completion_id(&self) -> Option<&str> {
        match self {
            Self::TaskCompleted { task_id } => Some(task_id),
            Self::SubagentCompleted { subagent_id } => Some(subagent_id),
            Self::WorkflowCompleted { completion_id } => Some(completion_id),
            Self::User
            | Self::NotificationDrain
            | Self::HostCommand
            | Self::GoalContinuation { .. }
            | Self::PlanResume => None,
        }
    }
}

impl TurnKind {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Internal => "internal",
        }
    }
}
#[cfg(test)]
mod turn_identity_tests {
    use super::{PromptOrigin, TurnKind};

    #[test]
    fn origin_and_kind_are_structured_independently_of_prompt_id() {
        let origin = PromptOrigin::GoalContinuation {
            goal_id: "g1".into(),
        };
        assert_eq!(origin.wire_name(), "goal_continuation");
        assert!(origin.is_synthetic());
        assert!(origin.hide_user_echo_from_scrollback());
        assert_eq!(TurnKind::Internal.wire_name(), "internal");
    }

    #[test]
    fn only_replaceable_wakes_are_preemptible() {
        assert!(PromptOrigin::NotificationDrain.is_preemptible_wake());
        assert!(
            !PromptOrigin::GoalContinuation {
                goal_id: "g1".into(),
            }
            .is_preemptible_wake()
        );
    }
}

/// Client-requested fs notification mode (was fsnotify::FsNotifyMode).
/// Determines whether the session sends an initial file index to the client
/// or just streams raw file events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum ClientFsMode {
    #[default]
    Events,
    Index,
}
/// Client-side fs notification config: fs source settings + mode.
#[derive(Debug, Clone, Default)]
pub struct ClientFsConfig {
    pub fs: FsConfig,
    pub mode: ClientFsMode,
}
pub mod acp_conversion;
pub mod acp_mcp;
pub(crate) mod agent_rebuild;
pub(crate) mod event_tracker;
pub(crate) mod event_types;
pub(crate) mod events;
pub mod file_system;
pub mod fork;
pub(crate) mod fs_watch;
pub(crate) mod goal_notification;
pub mod goal_tracker;
pub mod helpers;
pub(crate) mod image_describe;
pub(crate) mod image_normalize;
pub mod inference_metrics;
pub mod timeline_persistence;
pub mod trajectory;
pub use client_support::session::info;
pub mod mcp_catalog;
pub mod mcp_dispatcher;
#[cfg(test)]
mod mcp_dispatcher_e2e_tests;
pub mod mcp_restart;
pub mod mcp_servers;
pub mod memory;
pub(crate) mod normalize_cache;
pub mod persistence;
pub use client_support::placeholder_images;
pub mod behavior;
pub(crate) mod diagnostics;
pub mod prompt_parser;
pub(crate) mod prompt_timing;
pub(crate) mod replay_events;
pub mod result;
pub mod signals;
pub(crate) mod slash_commands;
pub mod storage;
#[cfg(feature = "test-support")]
pub mod testkit;
pub mod tool_index;
pub(crate) mod turn_completion;
pub mod unified_list;
pub(crate) mod user_message;
pub(crate) mod wire_tags;
pub(crate) mod workflow;
pub mod worktree;
pub mod worktree_pool;
