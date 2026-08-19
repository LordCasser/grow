use serde::Serialize;

/// Schema version for the event log format. Bumped on breaking changes.
pub const EVENT_SCHEMA_VERSION: &str = "3.0";

/// A single event in the per-turn event log.
///
/// Shell-side producer vocabulary. `EventTracker` maps each value into the
/// canonical Timeline as a typed fact or log-only observation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    TurnStarted {
        session_id: String,
        turn_number: u64,
        identity: chat_state::TurnIdentity,
        model_id: String,
        permission_mode: diagnostics::enums::PermissionMode,
        conversation_message_count: usize,
        prompt_index: Option<usize>,
        prompt_text: Option<String>,
        input_kind: chat_state::TurnInputKind,
        session_relationship: SessionRelationship,
        schema_version: String,
        /// Set when this turn is the user's redirect after a Ctrl+C / Esc abort
        /// of the previous turn: `cancel_then_send` (the user typed a fresh
        /// prompt) or `queued_after_cancel` (a prompt sat queued behind the
        /// aborted turn and was promoted). `None` for normal turns. Pairs with
        /// the `interjected` event's `redirect_kind` so the trace pipeline can
        /// query every user redirect through one shared field.
        #[serde(skip_serializing_if = "Option::is_none")]
        redirect_kind: Option<RedirectKind>,
    },
    PhaseChanged {
        phase: Phase,
    },
    FirstToken,
    LoopStarted {
        loop_index: u32,
    },
    ToolStarted {
        tool_name: String,
    },
    ToolCompleted {
        tool_name: String,
        /// Dispatch wall time; a cancel row reuses the duration measured at dispatch.
        duration_ms: u64,
        outcome: ToolOutcome,
        /// Model/ACP tool call id; matches the conversation's `tool_result`.
        tool_call_id: String,
        /// Which emitter wrote this row. Shell rows time dispatch; workspace
        /// rows time the hub/proxy hop for the same call.
        source: ToolCompletedSource,
    },
    PermissionRequested {
        tool_name: String,
    },
    PermissionResolved {
        tool_name: String,
        decision: PermissionDecision,
        wait_ms: u64,
    },
    TurnEnded {
        outcome: TurnOutcomeLabel,
        #[serde(skip_serializing_if = "Option::is_none")]
        cancellation_category: Option<CancellationCategory>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cancellation_context: Option<serde_json::Value>,
    },
    /// A mid-turn user interjection was merged into the running turn. Unlike
    /// `TurnEnded`, an interjection never ends the turn — the user steered
    /// in-flight (Ctrl+Enter) or promoted a queued prompt into the running
    /// turn. `source` distinguishes those two paths; `image_count` is how
    /// many images rode along (0 for text-only). Emitted at enqueue time,
    /// once per interjection.
    Interjected {
        source: InterjectionSource,
        image_count: u32,
        /// Always [`RedirectKind::Interjection`]. Carried so the shared
        /// `redirect_kind` field is queryable uniformly across every redirect
        /// event (`interjected` + the next-turn-after-abort `turn_started`).
        redirect_kind: RedirectKind,
    },
    PermissionModeChanged {
        previous_mode: diagnostics::enums::PermissionMode,
        mode: diagnostics::enums::PermissionMode,
    },
    /// Runtime TodoGate nudged the model because a content-only turn ended
    /// with pending or unbacked in_progress todos. `reason` is the
    /// `TODO_GATE_*` discriminator constant in `shell::session::events`.
    TodoGateFired {
        fires: u32,
        pending: usize,
        in_progress: usize,
        reason: &'static str,
    },
    /// TodoGate hit its per-prompt fire cap. Distinct event so cap-exhaustion
    /// is not conflated with a normal fire in the dashboards.
    TodoGateExhausted {
        pending: usize,
    },
    /// Layer-3 LazinessDetector classifier completed and produced a verdict.
    /// Fires even in observation-only mode (`max_nudges_per_session = 0`)
    /// so dashboards can validate classification quality before any nudges
    /// are injected. `category` is one of the `LAZINESS_*` discriminator
    /// constants in `shell::session::events`.
    LazinessClassifierFired {
        model_id: String,
        category: &'static str,
        confidence: f32,
    },
    /// Layer-3 LazinessDetector injected a system-reminder nudge into the
    /// session. Always preceded by a `LazinessClassifierFired` for the
    /// same classification. Suppressed when the per-session cap is 0.
    LazinessNudgeFired {
        model_id: String,
        category: &'static str,
        nudges_remaining: u32,
    },
    /// Layer-3 LazinessDetector terminated without producing a verdict.
    /// `reason` is one of the `LAZINESS_ABORT_*` discriminator constants
    /// in `shell::session::events`.
    LazinessClassifierAborted {
        reason: &'static str,
    },
    // ── MCP Diagnostics ──────────────────────────────────────────
    McpConfigResolved {
        servers: Vec<McpConfigServer>,
        disabled: Vec<String>,
    },
    McpManagedConfigResult {
        server_count: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    McpServerStarting {
        server_name: String,
        transport: String,
        target: String,
        timeout_sec: u64,
    },
    McpServerConnected {
        server_name: String,
        transport: String,
        tool_count: u32,
        duration_ms: u64,
        tools: Vec<String>,
    },
    McpServerFailed {
        server_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        transport: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
        error_type: McpErrorCategory,
        error_message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        timeout_sec: Option<u64>,
    },
    McpToolRegistrationFailed {
        server_name: String,
        tool_name: String,
        error: String,
    },
    McpInitCompleted {
        total_servers: u32,
        succeeded: u32,
        failed: u32,
        credentials_rejected: u32,
        total_tools: u32,
        duration_ms: u64,
        is_reinit: bool,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        failed_servers: Vec<String>,
    },
    McpInitCancelled {
        reason: String,
    },
    McpToolCallStarted {
        server_name: String,
        tool_name: String,
        call_id: String,
        timeout_sec: u64,
    },
    McpToolCallCompleted {
        server_name: String,
        tool_name: String,
        call_id: String,
        duration_ms: u64,
        success: bool,
        is_timeout: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        reconnect_attempted: bool,
        auth_retry_attempted: bool,
    },
    McpTransportError {
        server_name: String,
        tool_name: String,
        error: String,
    },
    /// A line on an MCP stdio server's stdout could not be decoded as a
    /// transport. Surfaces the otherwise-invisible "connector shows but
    /// doesn't work" case (a server logging to stdout, a JSON-RPC batch
    /// array, or an off-spec response). Distinct from `McpTransportError`,
    /// environment; either the orchestrator called
    /// which is a per-tool-call transport failure.
    McpTransportDecodeError {
        server_name: String,
        error: String,
        /// Truncated copy of the offending line, for diagnosis.
        sample: String,
    },
    McpTransportReconnect {
        server_name: String,
        success: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    McpAuthRetry {
        server_name: String,
        trigger: String,
        success: bool,
    },
    McpHealthCheck {
        server_name: String,
        healthy: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_state: Option<String>,
    },
    McpServerToggled {
        server_name: String,
        enabled: bool,
    },
}

/// Who emitted a [`Event::ToolCompleted`] row.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCompletedSource {
    /// Shell dispatch clock — join against these.
    Shell,
    /// Workspace hub/proxy hop clock.
    Workspace,
}

/// Where a mid-turn interjection originated. Drives the `source` field on
/// [`Event::Interjected`].
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InterjectionSource {
    /// Direct `grow/interject` while a turn was running (Ctrl+Enter).
    Direct,
    /// A queued (not-yet-running) prompt steered into the identified regular
    /// turn via `SteerQueuedPrompt` (queue "send now").
    Queue,
}

/// The user-redirect mechanism behind an event — the shared discriminator that
/// lets the trace pipeline query every user steer through one field. Present on
/// [`Event::Interjected`] (always [`RedirectKind::Interjection`]) and, for the
/// next turn after a Ctrl+C / Esc abort, on [`Event::TurnStarted`]
/// ([`RedirectKind::CancelThenSend`] / [`RedirectKind::QueuedAfterCancel`]).
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RedirectKind {
    /// Mid-turn interjection — Ctrl+O / `grow/interject`, or "Send now" on a
    /// queued row. The turn keeps running; nothing is cancelled.
    Interjection,
    /// The turn was aborted (Ctrl+C / Esc) and the user then typed and sent a
    /// fresh prompt as the next turn.
    CancelThenSend,
    /// The turn was aborted (Ctrl+C / Esc) while a prompt sat queued behind it;
    /// that queued prompt was promoted as the next turn.
    QueuedAfterCancel,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpErrorCategory {
    SpawnFailed,
    Timeout,
    HandshakeFailed,
    AuthRequired,
    ClientError,
}

/// Server entry in `McpConfigResolved`.
#[derive(Debug, Clone, Serialize)]
pub struct McpConfigServer {
    pub name: String,
    pub transport: String,
    pub source: String,
}

/// Outcome of a single tool call. More granular than a boolean -- distinguishes
/// between tools that executed vs tools that were never run.
#[derive(Debug, Clone, Copy, Serialize, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ToolOutcome {
    /// Tool executed and returned a result.
    Success,
    /// Tool executed but returned an error.
    Error,
    /// User rejected the permission prompt.
    PermissionRejected,
    /// User cancelled the permission prompt (Cmd+C).
    PermissionCancelled,
    /// The permission client did not answer before the session deadline.
    PermissionTimedOut,
    /// User provided a followup message instead of approving.
    Followup,
    /// A user-configured hook blocked execution.
    HookDenied,
    /// Tool not found or arguments couldn't be parsed.
    InvalidTool,
    /// Tool was running when the turn was cancelled (Cmd+C).
    Cancelled,
}

impl From<ToolOutcome> for ::diagnostics::events::ToolOutcome {
    fn from(o: ToolOutcome) -> Self {
        match o {
            ToolOutcome::Success => Self::Success,
            ToolOutcome::Error => Self::Error,
            ToolOutcome::PermissionRejected => Self::PermissionRejected,
            ToolOutcome::PermissionCancelled => Self::PermissionCancelled,
            ToolOutcome::PermissionTimedOut => Self::PermissionTimedOut,
            ToolOutcome::Followup => Self::Followup,
            ToolOutcome::HookDenied => Self::HookDenied,
            ToolOutcome::InvalidTool => Self::InvalidTool,
            ToolOutcome::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    WaitingForModel,
    StreamingText,
    StreamingReasoning,
    ToolExecution,
    PermissionPrompt,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRelationship {
    Primary,
    #[allow(dead_code)]
    Subagent,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcomeLabel {
    Completed,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    Deny,
    Cancelled,
    TimedOut,
    Followup,
}

// `CancellationCategory` is the `StopCancelled` hook payload's `reason` type
// and lives in `hooks::event` (single source of truth) alongside the other
// hook wire types. Re-exported here so the session event layer and every
// existing call site (`crate::session::events::CancellationCategory`) keep
// their path. Wire form is unchanged: bare snake_case strings.
pub use ::hooks::event::CancellationCategory;

// Note: `From<&permission::Decision> for PermissionDecision` crosses the
// crate boundary (orphan rule) and lives in
// `shell/src/session/events.rs`.

#[cfg(test)]
mod tests {
    use super::*;

    fn user_identity() -> chat_state::TurnIdentity {
        chat_state::TurnIdentity {
            origin: "user".into(),
            turn_kind: "user".into(),
            goal_id: None,
            stage_id: None,
        }
    }

    #[test]
    fn tool_completed_source_is_always_explicit() {
        let shell = serde_json::to_value(Event::ToolCompleted {
            tool_name: "bash".into(),
            duration_ms: 10,
            outcome: ToolOutcome::Success,
            tool_call_id: "c1".into(),
            source: ToolCompletedSource::Shell,
        })
        .unwrap();
        assert_eq!(shell["source"], "shell");

        let workspace = serde_json::to_value(Event::ToolCompleted {
            tool_name: "bash".into(),
            duration_ms: 10,
            outcome: ToolOutcome::Success,
            tool_call_id: "c1".into(),
            source: ToolCompletedSource::Workspace,
        })
        .unwrap();
        assert_eq!(workspace["source"], "workspace");
    }

    #[test]
    fn interjected_event_serializes_tag_source_and_count() {
        let ev = Event::Interjected {
            source: InterjectionSource::Direct,
            image_count: 2,
            redirect_kind: RedirectKind::Interjection,
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["type"], "interjected");
        assert_eq!(v["source"], "direct");
        assert_eq!(v["image_count"], 2);
        // Shared discriminator: always present on interjected events.
        assert_eq!(v["redirect_kind"], "interjection");

        let queue = serde_json::to_value(Event::Interjected {
            source: InterjectionSource::Queue,
            image_count: 0,
            redirect_kind: RedirectKind::Interjection,
        })
        .unwrap();
        assert_eq!(queue["source"], "queue");
        assert_eq!(queue["image_count"], 0);
        assert_eq!(queue["redirect_kind"], "interjection");
    }

    #[test]
    fn redirect_kind_serializes_snake_case() {
        for (variant, expected) in [
            (RedirectKind::Interjection, "\"interjection\""),
            (RedirectKind::CancelThenSend, "\"cancel_then_send\""),
            (RedirectKind::QueuedAfterCancel, "\"queued_after_cancel\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected, "{variant:?} must serialize to {expected}");
        }
    }

    #[test]
    fn turn_started_redirect_kind_present_when_set_omitted_when_none() {
        let with_kind = serde_json::to_value(Event::TurnStarted {
            session_id: "s".into(),
            turn_number: 2,
            identity: user_identity(),
            model_id: "grow-4".into(),
            permission_mode: diagnostics::enums::PermissionMode::Ask,
            conversation_message_count: 3,
            prompt_index: Some(2),
            prompt_text: Some("prompt".into()),
            input_kind: chat_state::TurnInputKind::Prompt,
            session_relationship: SessionRelationship::Primary,
            schema_version: EVENT_SCHEMA_VERSION.into(),
            redirect_kind: Some(RedirectKind::QueuedAfterCancel),
        })
        .unwrap();
        assert_eq!(with_kind["type"], "turn_started");
        assert_eq!(with_kind["redirect_kind"], "queued_after_cancel");

        let normal = serde_json::to_value(Event::TurnStarted {
            session_id: "s".into(),
            turn_number: 1,
            identity: user_identity(),
            model_id: "grow-4".into(),
            permission_mode: diagnostics::enums::PermissionMode::Ask,
            conversation_message_count: 0,
            prompt_index: Some(1),
            prompt_text: Some("prompt".into()),
            input_kind: chat_state::TurnInputKind::Prompt,
            session_relationship: SessionRelationship::Primary,
            schema_version: EVENT_SCHEMA_VERSION.into(),
            redirect_kind: None,
        })
        .unwrap();
        assert!(
            normal.get("redirect_kind").is_none(),
            "redirect_kind must be omitted on a normal turn, got {normal}"
        );
    }
}
