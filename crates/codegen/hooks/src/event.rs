use serde::{Deserialize, Serialize};

/// Maximum serialized size for `toolInput` or `toolResult` in bytes (128 KB).
pub const MAX_PAYLOAD_SIZE: usize = 128 * 1024;

/// Generates [`HookEventName`] and its wire parser, display, traits, and `ALL`
/// from one table. Each event has exactly one canonical snake-case key.
macro_rules! hook_events {
    ($(
        $(#[$vmeta:meta])*
        $variant:ident {
            key: $key:literal,
            traits: ($gate:ident, $matcher:ident $(,)?),
        }
    ),* $(,)?) => {
        /// Hook event types. `Ord` follows table order.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum HookEventName {
            $($(#[$vmeta])* $variant),*
        }

        impl HookEventName {
            /// Every variant, in canonical display order.
            pub const ALL: &'static [HookEventName] = &[$(HookEventName::$variant),*];

            /// Source of truth for canonical keys, behind `Deserialize` and `parse_key`.
            fn from_key_str(s: &str) -> Option<Self> {
                match s {
                    $($key => Some(Self::$variant),)*
                    _ => None,
                }
            }

            /// The event's dispatch traits, generated exhaustively from the table.
            pub fn traits(self) -> EventTraits {
                use GateKind::*;
                use MatcherPolicy::*;
                match self {
                    $(Self::$variant => EventTraits {
                        gate: $gate,
                        matcher: $matcher,
                    },)*
                }
            }
        }

        impl std::fmt::Display for HookEventName {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(match self { $(Self::$variant => $key,)* })
            }
        }

        impl<'de> serde::Deserialize<'de> for HookEventName {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let s = <String as serde::Deserialize>::deserialize(deserializer)?;
                Self::from_key_str(&s).ok_or_else(|| {
                    // Built from the table so it can't drift from the accepted set.
                    let known = Self::ALL
                        .iter()
                        .map(|e| e.to_string())
                        .collect::<std::collections::BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ");
                    serde::de::Error::custom(format!(
                        "unknown hook event: '{s}'. Expected one of: {known}"
                    ))
                })
            }
        }
    };
}

// Table order is the canonical display order (drives `ALL` and `Ord`).
hook_events! {
    SessionStart {
        key: "session_start",
        traits: (Observe, Tested),
    },
    UserPromptSubmit {
        key: "user_prompt_submit",
        traits: (Observe, Ignored),
    },
    PreToolUse {
        key: "pre_tool_use",
        traits: (Tool, Tested),
    },
    PostToolUse {
        key: "post_tool_use",
        traits: (Observe, Tested),
    },
    PostToolUseFailure {
        key: "post_tool_use_failure",
        traits: (Observe, Tested),
    },
    PermissionDenied {
        key: "permission_denied",
        traits: (Observe, Tested),
    },
    /// Fires on a genuine turn-end with stop decision control (a hook can block);
    /// not on user interrupts (API-error turns fire `StopFailure`); observe-only at session end.
    Stop {
        key: "stop",
        traits: (Stop, Ignored),
    },
    /// Fires when the turn ends due to an API error. Output and exit code are ignored.
    StopFailure {
        key: "stop_failure",
        traits: (Observe, Tested),
    },
    /// Fires when a turn is cancelled (user abort, permission/hook gate). Observe-only:
    /// output and exit code are ignored, and a hook can never delay the cancel itself.
    StopCancelled {
        key: "stop_cancelled",
        traits: (Observe, Ignored),
    },
    Notification {
        key: "notification",
        traits: (Observe, Tested),
    },
    SubagentStart {
        key: "subagent_start",
        traits: (Observe, Tested),
    },
    SubagentStop {
        key: "subagent_stop",
        traits: (Stop, Tested),
    },
    PreCompact {
        key: "pre_compact",
        traits: (Observe, Tested),
    },
    PostCompact {
        key: "post_compact",
        traits: (Observe, Tested),
    },
    SessionEnd {
        key: "session_end",
        traits: (Observe, Tested),
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateKind {
    /// Hook output recorded, decisions ignored.
    Observe,
    Tool,
    /// Stop decision control (`block`, `continue: false`, `additionalContext`).
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatcherPolicy {
    /// Never evaluated: kept for display with a load-time warning, the hook fires on every occurrence.
    Ignored,
    /// Tested against the value [`HookPayload::match_value`] extracts from the payload.
    Tested,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventTraits {
    pub gate: GateKind,
    pub matcher: MatcherPolicy,
}

impl HookEventName {
    /// Validate a bare event key against the accepted spellings; `None` if unknown.
    pub fn parse_key(s: &str) -> Option<Self> {
        Self::from_key_str(s)
    }
}

/// Max characters for free-text fields in `StopBackgroundTask`/`StopSessionCron` entries.
pub const MAX_STOP_ENTRY_TEXT_CHARS: usize = 1000;

/// Clip `text` to `max` chars (on a char boundary) with a `… [+N chars]` marker.
pub fn clip_text(text: &str, max: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max {
        return text.to_string();
    }
    let clipped: String = text.chars().take(max).collect();
    format!("{clipped}… [+{} chars]", char_count - max)
}

pub fn clip_stop_entry_text(text: &str) -> String {
    clip_text(text, MAX_STOP_ENTRY_TEXT_CHARS)
}

/// Max characters for the free-text cancel trigger in a `StopCancelled` payload.
pub const MAX_CANCEL_TRIGGER_CHARS: usize = 64;

pub fn clip_cancel_trigger(text: &str) -> String {
    clip_text(text, MAX_CANCEL_TRIGGER_CHARS)
}

/// `SubagentStop` fire phase: always `Gate` today, `Observe` reserved and not emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentStopPhase {
    Gate,
    Observe,
}

/// One in-flight background task in a `Stop` hook input (camelCase on the wire).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopBackgroundTask {
    pub id: String,
    pub r#type: BackgroundTaskType,
    /// Always `running` for in-flight entries.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
}

/// One session-scoped scheduled wakeup (scheduler task or `/loop`) in a `Stop` hook input.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopSessionCron {
    pub id: String,
    /// Human-readable interval (e.g. `every 5 minutes`): grow schedules are intervals, not cron.
    pub schedule: String,
    pub recurring: bool,
    pub prompt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundTaskType {
    Shell,
    Monitor,
    Subagent,
}

/// `StopFailure` error type. Capacity errors fold into `RateLimit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopFailureKind {
    RateLimit,
    AuthenticationFailed,
    InvalidRequest,
    ServerError,
    ContextWindowExceeded,
    Unknown,
}

impl StopFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RateLimit => "rate_limit",
            Self::AuthenticationFailed => "authentication_failed",
            Self::InvalidRequest => "invalid_request",
            Self::ServerError => "server_error",
            Self::ContextWindowExceeded => "context_window_exceeded",
            Self::Unknown => "unknown",
        }
    }
}

/// Why a turn was cancelled. Single source of truth shared by the session
/// event log (`TurnEnded.cancellation_category`) and the `StopCancelled` hook
/// payload's `reason`. `Deserialize`/`PartialEq`/`Eq`/`Hash` let the workspace
/// decode `cancellation_category` strings back into this enum, and `snake_case`
/// keeps the wire form identical to serialization.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CancellationCategory {
    HookDenied,
    PermissionRejected,
    PermissionCancelled,
    PermissionTimedOut,
    MidTurnAbort,
}

/// The normalized event envelope sent to hook commands on stdin as JSON:
/// common metadata plus an event-specific payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEventEnvelope {
    pub hook_event_name: HookEventName,
    pub session_id: String,
    pub cwd: String,
    pub workspace_root: String,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
    /// Canonical Grow permission mode (`ask`, `auto`, or `always-approve`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(flatten)]
    pub payload: HookPayload,
}

/// Event-specific payload, flattened into the envelope JSON.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum HookPayload {
    SessionStart {
        source: String,
        #[serde(rename = "modelId", skip_serializing_if = "Option::is_none")]
        model_id: Option<String>,
        #[serde(rename = "agentType", skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
    },
    SessionEnd {
        reason: String,
        #[serde(rename = "turnCount", skip_serializing_if = "Option::is_none")]
        turn_count: Option<u64>,
        #[serde(rename = "toolCallCount", skip_serializing_if = "Option::is_none")]
        tool_call_count: Option<u64>,
    },
    Stop {
        reason: String,
        /// True when this Stop fires while the agent is already continuing from a
        /// previous Stop-hook block this turn; hooks check it to avoid blocking on a
        /// condition that will never resolve.
        #[serde(rename = "stopHookActive")]
        stop_hook_active: bool,
        #[serde(
            rename = "lastAssistantMessage",
            skip_serializing_if = "Option::is_none"
        )]
        last_assistant_message: Option<String>,
        /// In-flight background work that could wake the session; empty when none in
        /// flight, omitted (not empty) at fire sites that don't enumerate (session end).
        #[serde(rename = "backgroundTasks", skip_serializing_if = "Option::is_none")]
        background_tasks: Option<Vec<StopBackgroundTask>>,
        #[serde(rename = "sessionCrons", skip_serializing_if = "Option::is_none")]
        session_crons: Option<Vec<StopSessionCron>>,
    },
    StopFailure {
        error: StopFailureKind,
        #[serde(rename = "errorDetails", skip_serializing_if = "Option::is_none")]
        error_details: Option<String>,
        /// Rendered error text shown in the conversation: unlike `Stop`, the error
        /// string, not assistant output.
        #[serde(
            rename = "lastAssistantMessage",
            skip_serializing_if = "Option::is_none"
        )]
        last_assistant_message: Option<String>,
    },
    StopCancelled {
        /// Why the turn was cancelled, directly serialized as the shared
        /// [`CancellationCategory`] (bare snake_case on the wire).
        reason: CancellationCategory,
        /// Free-text cancel trigger (e.g. `ctrl_c`, `esc`), clipped to
        /// [`MAX_CANCEL_TRIGGER_CHARS`]; omitted when the cancel path carries none.
        #[serde(skip_serializing_if = "Option::is_none")]
        trigger: Option<String>,
    },

    PreToolUse {
        /// The tool the model invoked. For the meta-dispatch tools (`use_tool`
        /// and the external MCP-call tool) this is the resolved underlying tool
        /// (`server__tool`) rather than the dispatcher, so matchers key on it.
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        #[serde(rename = "toolInput")]
        tool_input: serde_json::Value,
        #[serde(rename = "toolInputTruncated")]
        tool_input_truncated: bool,
        /// The subagent's type when this tool runs inside one (the envelope's `sessionId`
        /// gives its identity); `None` for the top-level session.
        #[serde(rename = "subagentType", skip_serializing_if = "Option::is_none")]
        subagent_type: Option<String>,
    },
    PostToolUse {
        /// Resolved underlying tool for meta-dispatch tools (see `PreToolUse`).
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        #[serde(rename = "toolInput")]
        tool_input: serde_json::Value,
        #[serde(rename = "toolResult")]
        tool_result: serde_json::Value,
        #[serde(rename = "toolInputTruncated")]
        tool_input_truncated: bool,
        #[serde(rename = "toolResultTruncated")]
        tool_result_truncated: bool,
        #[serde(rename = "durationMs", skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
        #[serde(rename = "isBackgrounded")]
        is_backgrounded: bool,
        #[serde(rename = "subagentType", skip_serializing_if = "Option::is_none")]
        subagent_type: Option<String>,
    },
    PostToolUseFailure {
        /// Resolved underlying tool for meta-dispatch tools (see `PreToolUse`).
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        #[serde(rename = "toolInput")]
        tool_input: serde_json::Value,
        #[serde(rename = "toolInputTruncated")]
        tool_input_truncated: bool,
        error: String,
        #[serde(rename = "subagentType", skip_serializing_if = "Option::is_none")]
        subagent_type: Option<String>,
    },
    PermissionDenied {
        /// Resolved underlying tool for meta-dispatch tools (see `PreToolUse`).
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        #[serde(rename = "toolInput")]
        tool_input: serde_json::Value,
        #[serde(rename = "toolInputTruncated")]
        tool_input_truncated: bool,
    },

    UserPromptSubmit {
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    Notification {
        #[serde(rename = "notificationType")]
        notification_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Compat: some callers use `level` instead of `notificationType`.
        #[serde(skip_serializing_if = "Option::is_none")]
        level: Option<String>,
    },

    SubagentStart {
        #[serde(rename = "subagentId")]
        subagent_id: String,
        #[serde(rename = "subagentType")]
        subagent_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    SubagentStop {
        phase: SubagentStopPhase,
        #[serde(rename = "subagentId")]
        subagent_id: String,
        #[serde(rename = "subagentType")]
        subagent_type: String,
        /// Subagent analogue of `Stop::stop_hook_active`.
        #[serde(rename = "stopHookActive", skip_serializing_if = "Option::is_none")]
        stop_hook_active: Option<bool>,
        #[serde(
            rename = "lastAssistantMessage",
            skip_serializing_if = "Option::is_none"
        )]
        last_assistant_message: Option<String>,
    },

    PreCompact {
        /// "manual" or "auto".
        source: String,
    },
    PostCompact {
        /// "manual" or "auto".
        source: String,
    },
}

impl HookPayload {
    /// The value a [`MatcherPolicy::Tested`] matcher is tested against, or `None` when
    /// the payload carries nothing selectable (matchers then fire-all, the fail-open default).
    pub fn match_value(&self) -> Option<&str> {
        let value = match self {
            Self::PreToolUse { tool_name, .. }
            | Self::PostToolUse { tool_name, .. }
            | Self::PostToolUseFailure { tool_name, .. }
            | Self::PermissionDenied { tool_name, .. } => tool_name,
            Self::Notification {
                notification_type, ..
            } => notification_type,
            Self::SubagentStart { subagent_type, .. }
            | Self::SubagentStop { subagent_type, .. } => subagent_type,
            Self::SessionStart { source, .. }
            | Self::PreCompact { source }
            | Self::PostCompact { source } => source,
            Self::SessionEnd { reason, .. } => reason,
            // Always a non-empty name, unlike the free-text arms above.
            Self::StopFailure { error, .. } => return Some(error.as_str()),
            // Ignored events listed explicitly so a new Tested event can't silently return None.
            Self::Stop { .. } | Self::UserPromptSubmit { .. } | Self::StopCancelled { .. } => {
                return None;
            }
        };
        Some(value.as_str()).filter(|v| !v.is_empty())
    }
}

/// Truncate a JSON value if its serialized size exceeds `MAX_PAYLOAD_SIZE`.
///
/// Returns `(possibly_truncated_value, was_truncated)`.
pub fn truncate_payload(value: serde_json::Value) -> (serde_json::Value, bool) {
    let serialized = serde_json::to_string(&value).unwrap_or_default();
    if serialized.len() <= MAX_PAYLOAD_SIZE {
        return (value, false);
    }

    // Cut at the largest char boundary <= MAX_PAYLOAD_SIZE so the slice never
    // splits a multibyte codepoint.
    let mut end = MAX_PAYLOAD_SIZE;
    while !serialized.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = serialized[..end].to_string();
    result.push_str(" [truncated]");
    (serde_json::Value::String(result), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_name_deser_all_variants() {
        let cases: &[(&str, HookEventName)] = &[
            ("session_start", HookEventName::SessionStart),
            ("pre_tool_use", HookEventName::PreToolUse),
            ("post_tool_use", HookEventName::PostToolUse),
            ("post_tool_use_failure", HookEventName::PostToolUseFailure),
            ("session_end", HookEventName::SessionEnd),
            ("stop", HookEventName::Stop),
            ("stop_failure", HookEventName::StopFailure),
            ("stop_cancelled", HookEventName::StopCancelled),
            ("notification", HookEventName::Notification),
            ("user_prompt_submit", HookEventName::UserPromptSubmit),
            ("permission_denied", HookEventName::PermissionDenied),
            ("subagent_start", HookEventName::SubagentStart),
            ("subagent_stop", HookEventName::SubagentStop),
            ("pre_compact", HookEventName::PreCompact),
            ("post_compact", HookEventName::PostCompact),
        ];

        for (key, expected) in cases {
            let parsed: HookEventName = serde_json::from_str(&format!("\"{key}\"")).unwrap();
            assert_eq!(parsed, *expected, "deserialization failed for {key}");
        }
    }

    #[test]
    fn event_name_display_all_variants() {
        let cases: &[(HookEventName, &str)] = &[
            (HookEventName::SessionStart, "session_start"),
            (HookEventName::PreToolUse, "pre_tool_use"),
            (HookEventName::PostToolUse, "post_tool_use"),
            (HookEventName::PostToolUseFailure, "post_tool_use_failure"),
            (HookEventName::SessionEnd, "session_end"),
            (HookEventName::Stop, "stop"),
            (HookEventName::StopFailure, "stop_failure"),
            (HookEventName::StopCancelled, "stop_cancelled"),
            (HookEventName::Notification, "notification"),
            (HookEventName::UserPromptSubmit, "user_prompt_submit"),
            (HookEventName::PermissionDenied, "permission_denied"),
            (HookEventName::SubagentStart, "subagent_start"),
            (HookEventName::SubagentStop, "subagent_stop"),
            (HookEventName::PreCompact, "pre_compact"),
            (HookEventName::PostCompact, "post_compact"),
        ];
        for (event, expected) in cases {
            assert_eq!(&event.to_string(), expected, "Display wrong for {event:?}");
        }
    }

    #[test]
    fn event_name_rejects_noncanonical_spellings() {
        for spelling in ["SessionStart", "sessionStart", "beforeShellExecution"] {
            assert!(
                serde_json::from_str::<HookEventName>(&format!("\"{spelling}\"")).is_err(),
                "noncanonical spelling must be rejected: {spelling}"
            );
        }
    }

    #[test]
    fn event_name_unknown_rejected() {
        let result = serde_json::from_str::<HookEventName>("\"UnknownEvent\"");
        assert!(result.is_err());
    }

    #[test]
    fn event_traits_report_gate_and_matcher() {
        use super::{GateKind, MatcherPolicy};

        assert_eq!(HookEventName::PreToolUse.traits().gate, GateKind::Tool);
        assert_eq!(HookEventName::Stop.traits().gate, GateKind::Stop);
        assert_eq!(HookEventName::SubagentStop.traits().gate, GateKind::Stop);
        assert_eq!(HookEventName::PostToolUse.traits().gate, GateKind::Observe);
        assert_eq!(
            HookEventName::StopCancelled.traits().gate,
            GateKind::Observe,
            "StopCancelled is observe-only: it never participates in a decision gate"
        );

        assert_eq!(HookEventName::Stop.traits().matcher, MatcherPolicy::Ignored);
        assert_eq!(
            HookEventName::StopCancelled.traits().matcher,
            MatcherPolicy::Ignored
        );
        assert_eq!(
            HookEventName::UserPromptSubmit.traits().matcher,
            MatcherPolicy::Ignored
        );
        assert_eq!(
            HookEventName::SessionStart.traits().matcher,
            MatcherPolicy::Tested
        );
    }

    #[test]
    fn clip_stop_entry_text_clips_on_char_boundary() {
        assert_eq!(clip_stop_entry_text("short"), "short");
        let exact = "x".repeat(MAX_STOP_ENTRY_TEXT_CHARS);
        assert_eq!(clip_stop_entry_text(&exact), exact);

        let long = "x".repeat(MAX_STOP_ENTRY_TEXT_CHARS + 42);
        let clipped = clip_stop_entry_text(&long);
        assert!(clipped.ends_with("… [+42 chars]"));

        let unicode = "€".repeat(MAX_STOP_ENTRY_TEXT_CHARS + 7);
        let clipped = clip_stop_entry_text(&unicode);
        assert!(clipped.ends_with("… [+7 chars]"));
    }

    #[test]
    fn stop_payload_serializes_task_and_cron_entries() {
        let envelope = HookEventEnvelope {
            hook_event_name: HookEventName::Stop,
            session_id: "s".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "t".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::Stop {
                reason: "end_turn".into(),
                stop_hook_active: true,
                last_assistant_message: Some("done".into()),
                background_tasks: Some(vec![
                    StopBackgroundTask {
                        id: "task-001".into(),
                        r#type: BackgroundTaskType::Shell,
                        status: "running".into(),
                        description: None,
                        command: Some("tail -f /var/log/syslog".into()),
                        agent_type: None,
                    },
                    StopBackgroundTask {
                        id: "task-002".into(),
                        r#type: BackgroundTaskType::Subagent,
                        status: "running".into(),
                        description: Some("explore the repo".into()),
                        command: None,
                        agent_type: Some("explore".into()),
                    },
                ]),
                session_crons: Some(vec![StopSessionCron {
                    id: "cron-001".into(),
                    schedule: "every 2h".into(),
                    recurring: true,
                    prompt: "check the build".into(),
                }]),
            },
        };
        let value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(value["stopHookActive"], true);
        assert_eq!(value["backgroundTasks"][0]["id"], "task-001");
        assert_eq!(value["backgroundTasks"][0]["type"], "shell");
        assert_eq!(
            value["backgroundTasks"][0]["command"],
            "tail -f /var/log/syslog"
        );
        assert_eq!(value["backgroundTasks"][1]["agentType"], "explore");
        assert_eq!(value["sessionCrons"][0]["schedule"], "every 2h");
        assert_eq!(value["sessionCrons"][0]["recurring"], true);
    }

    #[test]
    fn subagent_stop_phase_serializes_lowercase() {
        let payload = HookPayload::SubagentStop {
            phase: SubagentStopPhase::Observe,
            subagent_id: "sub-1".into(),
            subagent_type: "explore".into(),
            stop_hook_active: None,
            last_assistant_message: None,
        };
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["phase"], "observe");
        assert_eq!(
            serde_json::to_value(SubagentStopPhase::Gate).unwrap(),
            "gate"
        );
    }

    #[test]
    fn stop_failure_kind_as_str_matches_serialization() {
        for kind in [
            StopFailureKind::RateLimit,
            StopFailureKind::AuthenticationFailed,
            StopFailureKind::InvalidRequest,
            StopFailureKind::ServerError,
            StopFailureKind::ContextWindowExceeded,
            StopFailureKind::Unknown,
        ] {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.as_str()),
                "{kind:?} serialization drifted from as_str"
            );
        }
    }

    /// Every variant must survive a `to_value` -> `from_value` round-trip.
    #[test]
    fn cancellation_category_round_trips_every_variant() {
        for variant in [
            CancellationCategory::HookDenied,
            CancellationCategory::PermissionRejected,
            CancellationCategory::PermissionCancelled,
            CancellationCategory::PermissionTimedOut,
            CancellationCategory::MidTurnAbort,
        ] {
            let value = serde_json::to_value(variant).unwrap();
            let decoded: CancellationCategory = serde_json::from_value(value).unwrap();
            assert_eq!(decoded, variant, "{variant:?} must round-trip");
        }
    }

    /// Serialization is bare snake_case strings (identical to the session event
    /// log's `cancellation_category` wire form).
    #[test]
    fn cancellation_category_serializes_snake_case() {
        for (variant, expected) in [
            (CancellationCategory::HookDenied, "\"hook_denied\""),
            (
                CancellationCategory::PermissionRejected,
                "\"permission_rejected\"",
            ),
            (
                CancellationCategory::PermissionCancelled,
                "\"permission_cancelled\"",
            ),
            (
                CancellationCategory::PermissionTimedOut,
                "\"permission_timed_out\"",
            ),
            (CancellationCategory::MidTurnAbort, "\"mid_turn_abort\""),
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            assert_eq!(json, expected, "{variant:?} must serialize to {expected}");
        }
    }

    /// The `StopCancelled` payload serializes the shared `CancellationCategory`
    /// directly as `reason` (single source of truth — no string mapping layer),
    /// carries the cancel trigger, and omits an absent trigger.
    #[test]
    fn stop_cancelled_payload_serializes_reason_and_trigger() {
        let payload = HookPayload::StopCancelled {
            reason: CancellationCategory::MidTurnAbort,
            trigger: Some("esc".into()),
        };
        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(value["reason"], "mid_turn_abort");
        assert_eq!(value["trigger"], "esc");

        let no_trigger = HookPayload::StopCancelled {
            reason: CancellationCategory::PermissionRejected,
            trigger: None,
        };
        let value = serde_json::to_value(&no_trigger).unwrap();
        assert_eq!(value["reason"], "permission_rejected");
        assert!(
            value.get("trigger").is_none(),
            "absent trigger must be omitted, got {value}"
        );
    }

    /// The envelope stays far below `MAX_PAYLOAD_SIZE`: the reason is a fixed
    /// enum and the trigger is clipped to 64 chars.
    #[test]
    fn stop_cancelled_envelope_respects_payload_cap() {
        let long_trigger = "x".repeat(10_000);
        let envelope = HookEventEnvelope {
            hook_event_name: HookEventName::StopCancelled,
            session_id: "s".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "t".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: Some("p-1".into()),
            permission_mode: None,
            payload: HookPayload::StopCancelled {
                reason: CancellationCategory::MidTurnAbort,
                trigger: Some(clip_cancel_trigger(&long_trigger)),
            },
        };
        let value = serde_json::to_value(&envelope).unwrap();
        assert_eq!(value["hookEventName"], "stop_cancelled");
        assert!(value.get("trigger").is_some(), "trigger must be present");
        assert!(value.get("reason").is_some(), "reason must be present");
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(
            serialized.len() <= MAX_PAYLOAD_SIZE,
            "envelope must stay within MAX_PAYLOAD_SIZE"
        );
    }

    /// The cancel trigger clips at 64 chars on a char boundary (same marker
    /// convention as `clip_stop_entry_text`).
    #[test]
    fn clip_cancel_trigger_clips_on_char_boundary() {
        assert_eq!(clip_cancel_trigger("esc"), "esc");
        let exact = "x".repeat(MAX_CANCEL_TRIGGER_CHARS);
        assert_eq!(clip_cancel_trigger(&exact), exact);

        let long = "x".repeat(MAX_CANCEL_TRIGGER_CHARS + 5);
        let clipped = clip_cancel_trigger(&long);
        assert!(clipped.ends_with("… [+5 chars]"));

        let unicode = "€".repeat(MAX_CANCEL_TRIGGER_CHARS + 2);
        let clipped = clip_cancel_trigger(&unicode);
        assert!(clipped.ends_with("… [+2 chars]"));
    }

    #[test]
    fn truncate_small_payload() {
        let value = serde_json::json!({"key": "small"});
        let (result, truncated) = truncate_payload(value.clone());
        assert!(!truncated);
        assert_eq!(result, value);
    }

    #[test]
    fn truncate_large_payload() {
        let value = serde_json::Value::String("x".repeat(MAX_PAYLOAD_SIZE + 1000));
        let (result, truncated) = truncate_payload(value);
        assert!(truncated);
        let s = result.as_str().unwrap();
        assert!(s.ends_with("[truncated]"));
        assert!(s.len() < MAX_PAYLOAD_SIZE + 100);

        // '€' is 3 bytes, so the cut lands mid-codepoint and must fall back to a char boundary.
        let (unicode, truncated) =
            truncate_payload(serde_json::Value::String("€".repeat(MAX_PAYLOAD_SIZE)));
        assert!(truncated);
        assert!(unicode.as_str().unwrap().ends_with("[truncated]"));
    }

    #[test]
    fn envelope_serializes_camel_case() {
        let envelope = HookEventEnvelope {
            hook_event_name: HookEventName::SessionStart,
            session_id: "test-session".into(),
            cwd: "/tmp".into(),
            workspace_root: "/tmp".into(),
            timestamp: "2025-01-01T00:00:00Z".into(),
            transcript_path: None,
            client_identifier: None,
            prompt_id: None,
            permission_mode: None,
            payload: HookPayload::SessionStart {
                source: "new".into(),
                model_id: Some("grow-3".into()),
                agent_type: None,
            },
        };
        let value = serde_json::to_value(&envelope).unwrap();
        for key in ["hookEventName", "sessionId", "workspaceRoot", "modelId"] {
            assert!(value.get(key).is_some(), "missing camelCase key {key}");
        }
        for key in ["hook_event_name", "session_id", "model_id"] {
            assert!(value.get(key).is_none(), "leaked snake_case key {key}");
        }
    }
}
