//! chat-state — Timeline-backed session context management for Grow agents.
//!
//! This crate extracts conversation state management from `shell`'s
//! the shell session implementation into a standalone actor. It follows the same actor pattern
//! as `hunk-tracker`:
//!
//! ```text
//! ┌────────────────┐                  ┌──────────────────────────────────────┐
//! │ SessionActor   │ ─── Command ───▶ │        ChatStateActor                │
//! │  (push_user,   │                  │  (runs in dedicated tokio task)      │
//! │   build_req)   │                  │                                      │
//! └────────────────┘                  │  State (no locks needed):            │
//!                                     │  - append-only Timeline              │
//! ┌────────────────┐                  │  - sampling_config: SamplingConfig   │
//! │   Query (e.g.  │ ── Cmd+Oneshot ─▶│  - derived Timeline projections    │
//! │  get_conv)     │ ◀── Response ────│  - projected_tokens: u64            │
//! └────────────────┘                  │                                      │
//!                                     │         │ ChatStateEvent             │
//!                                     │         ▼                            │
//!                                     │  ┌──────────────────┐               │
//!                                     │  │ event_tx         │───▶ Session   │
//!                                     │  └──────────────────┘               │
//!                                     └──────────────────────────────────────┘
//! ```

pub mod actor;
pub mod commands;
pub mod compaction_utils;
pub mod events;
pub mod handle;
pub mod persistence;
pub mod sideband;
pub mod timeline;
pub mod trajectory;
pub mod types;
pub mod usage;

// Re-export main types for convenience
pub use actor::ChatStateActor;
pub use actor::state::{
    estimate_conversation_tokens, estimate_item_tokens, estimate_messages_tokens,
    estimate_request_input_tokens, estimate_system_message_tokens, estimate_tool_definition_tokens,
    estimate_tool_definitions_tokens, estimate_tool_specs_tokens,
};
pub use commands::{
    ConditionalToolResultOutcome, ImageProjectionReport, ModelMetadata, PruneError, PruneReport,
    RepairHistoryError, TimelineWriteError,
};
pub use events::ChatStateEvent;
pub use handle::ChatStateHandle;
pub use persistence::{
    MockPersistenceReceiver, MockTimelinePersistence, NullTimelinePersistence, PersistenceRecord,
    TimelinePersistence,
};
pub use sideband::{
    RecallMaterialization, SIDEBAND_SCHEMA_VERSION, SidebandAssemblyManifest, SidebandAttempt,
    SidebandBudgetPolicy, SidebandEnd, SidebandError, SidebandEvent, SidebandEventKind,
    SidebandOutcome, SidebandPurpose, SidebandRequest, SidebandResult, SidebandRoute,
    SidebandSpawnEvent, SidebandTimeline, SidebandUsage, TimelineMaterialization, TimelineRangeRef,
    validate_sideband_id,
};
pub use timeline::{
    ActiveControlContext, CompactionEvent, ControlContext, ControlContextActivation,
    ControlContextLayer, ControlEvent, EventSeq, ImageProjectionEvent, ImageShadow,
    ImageShadowSource, ImageToolCallShadow, MAX_NOTIFICATION_ID_BYTES,
    MAX_NOTIFICATION_PAYLOAD_BYTES, MAX_PENDING_MONITOR_PROGRESS_PER_TASK,
    MAX_WORKFLOW_RUN_ID_BYTES, MessageCause, MessageEvent, NotificationDismissReason,
    NotificationEvent, NotificationOwner, NotificationPayloadRef, NotificationSource,
    NotificationSourceVersion, NotificationTaskKind, ObservationEvent, PendingNotification,
    PromptRecord, RecoveryEvent, RequestEvent, RequestUsage, SessionTitleEvent, SessionTitleSource,
    StepEvent, StepId, SubagentContextSource, SubagentEvent, SubagentOutcome, SubagentResultEvent,
    SubagentSeedEvent, SubagentSpawnEvent, SubagentTerminalEvent, SurfaceId, SurfaceOp,
    SurfaceRange, TIMELINE_SCHEMA_VERSION, Timeline, TimelineError, TimelineEvent,
    TimelineEventKind, ToolEvent, TurnEvent, TurnId, TurnIdentity, TurnInputKind, TurnTerminal,
    WorkflowEvent, WorkflowExecutionStatus, WorkflowLifecycle, notification_id,
};
pub use trajectory::{
    SurfaceVisibility, TRAJECTORY_SCHEMA_VERSION, TrajectoryIssueSeverity, TrajectoryProjector,
    TrajectoryRow, TrajectorySnapshot, trajectory_issue_severity,
};
pub use types::*;
pub use usage::{UsageLedger, UsageTotals};
