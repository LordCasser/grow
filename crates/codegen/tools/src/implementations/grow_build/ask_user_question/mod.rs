//! `AskUserQuestion` tool — new architecture (`Tool` trait).
//!
//! Interactive Q&A tool that presents the user with structured questions and
//! option sets. In plan mode it serves as the **interview mechanism** — the
//! agent clarifies requirements, disambiguates approaches, and gets user input
//! on design decisions before finalizing the plan. Outside plan mode it is a
//! general-purpose tool for gathering user preferences during implementation.
//!
//! ## How It Works
//!
//! 1. The agent calls `AskUserQuestion` with an array of structured questions
//!    (each with options, optional preview, optional multi_select).
//! 2. The tool sends the structured request to the session coordinator and
//!    blocks on the response.
//! 3. The client presents the question UI and returns a typed response.
//! 4. The tool formats that response as the completed tool result.
//!
//! ## Plan-Mode Interview Actions
//!
//! When called during plan mode, the client can present two extra buttons:
//! - **"Respond to agent"** — partial answers, agent reformulates questions
//! - **"Finish plan interview"** — agent stops asking, proceeds with what it has
//!
//! These are client-side behaviors that produce different tool-result strings;
//! the tool itself is identical in and out of plan mode.

pub mod format;
pub mod types;

pub use types::{
    AskUserQuestionExtRequest, AskUserQuestionExtResponse, AskUserQuestionMode, QuestionAnnotation,
    UserQuestionError, UserQuestionRequest, UserQuestionResponse, UserQuestionResult,
    UserQuestionSender,
};

use crate::notification::types::UserQuestionAsked;
use crate::types::output::{AdvisoryQuestion, AskUserQuestionOutput};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::NotificationHandle;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Default max time to wait for the user to answer the questionnaire (all
/// questions in this tool call share one timer): 30 minutes. On expiry the
/// tool returns the same skipped/cancel text as a user dismiss
/// (`CANCEL_TEXT`), not a tool failure.
///
/// The shell resolves `[toolset.ask_user_question]` across its config tiers
/// and injects the result as [`AskUserQuestionParams`].
pub const RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Default for `timeout_enabled` across every resolver tier and settings
/// surface: the questionnaire timer is armed unless something disarms it.
/// Single source — the shell resolver's `.default(...)` and the pager's
/// settings registry both anchor on this const.
pub const DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED: bool = true;

/// Default for `allow_non_blocking` across every resolver tier and settings
/// surface: the Agent may select `blocking = false` per call unless the user
/// disables it. Single source, same anchoring as the timeout default above.
pub const DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING: bool = true;

/// Fixed presentation budget shared by the tool and the shell worker for
/// effective advisory questions (`blocking = false`): one deadline covering
/// enqueueing and the presentation acknowledgement round-trip. Not a user
/// wait budget — the user is never waited on in advisory mode.
pub const ADVISORY_PRESENTATION_BUDGET: std::time::Duration = std::time::Duration::from_secs(5);

/// Runtime-configurable parameters for the `ask_user_question` tool,
/// injected via `Params<AskUserQuestionParams>` in `SharedResources`.
///
/// The shell resolves `[toolset.ask_user_question]` across requirements >
/// env > user `config.toml` > managed > remote feature config and injects the
/// concrete result. Registry consumers that do not inject an override receive
/// the same concrete defaults through [`Default`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskUserQuestionParams {
    /// `false` disarms the questionnaire timer entirely.
    pub timeout_enabled: bool,
    /// Wait budget in seconds when the timer is armed (positive integer).
    pub timeout_secs: std::num::NonZeroU64,
    /// Whether the Agent may select nonblocking (advisory) questions.
    /// `false` forces every call to the blocking form, even when the model
    /// requests `blocking = false`.
    #[serde(default = "default_allow_non_blocking")]
    pub allow_non_blocking: bool,
}

/// Serde default for `allow_non_blocking`, anchored on the shared const.
fn default_allow_non_blocking() -> bool {
    DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING
}

impl Default for AskUserQuestionParams {
    fn default() -> Self {
        Self {
            timeout_enabled: DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED,
            timeout_secs: std::num::NonZeroU64::new(RESPONSE_TIMEOUT.as_secs())
                .expect("default timeout is non-zero"),
            allow_non_blocking: DEFAULT_ASK_USER_QUESTION_ALLOW_NON_BLOCKING,
        }
    }
}

crate::register_resource!("grow_build", "AskUserQuestion", AskUserQuestionParams);

/// Resource: live policy resolver injected by the shell (Factory), letting
/// the shell own config IO while the tool takes a fresh effective snapshot
/// on every admission. Same dependency-inversion pattern as
/// `Params<AskUserQuestionParams>` / `TaskModelValidator`.
///
/// Each call re-resolves env > effective TOML > default; a disk parse
/// failure returns `Err` rather than silently reopening a gate. When the
/// resolver is present it takes precedence over the injected
/// `Params<AskUserQuestionParams>`. The resource lock is released before
/// any user wait — never hold it while blocking.
#[derive(Clone, educe::Educe)]
#[educe(Debug)]
pub struct UserQuestionPolicyResolver(
    #[educe(Debug(ignore))]
    pub  std::sync::Arc<dyn Fn() -> Result<AskUserQuestionParams, String> + Send + Sync>,
);

crate::register_resource!(
    "grow_build",
    "UserQuestionPolicyResolver",
    UserQuestionPolicyResolver
);

impl AskUserQuestionParams {
    /// Effective wait budget: `Some(duration)` = bounded, `None` = wait forever.
    pub fn wait_budget(&self) -> Option<std::time::Duration> {
        if !self.timeout_enabled {
            return None;
        }
        Some(std::time::Duration::from_secs(self.timeout_secs.get()))
    }
}

/// A single option within a question.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QuestionOption {
    /// Option text shown to the user; a few words at most.
    #[schemars(description = "Option text shown to the user. A few words at most.")]
    pub label: String,

    /// What picking this option means or implies.
    #[schemars(description = "What picking this option means or implies.")]
    pub description: String,

    /// Optional content shown while the option is focused — mockups, code
    /// snippets, anything the user should compare. Single-select only.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Optional content shown while the option is focused — mockups, code snippets, anything the user should compare. Single-select questions only."
    )]
    pub preview: Option<String>,

    /// Opaque id; hidden from the model. Grow callers leave it `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub id: Option<String>,
}

/// A single question with its options.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Question {
    /// The question to ask, phrased as a full question.
    #[schemars(description = "The question to ask, phrased as a full question.")]
    pub question: String,

    /// The choices for this question.
    #[schemars(description = "The choices for this question.")]
    pub options: Vec<QuestionOption>,

    /// Let the user pick more than one option (default false).
    #[serde(default)]
    #[schemars(
        rename = "multi_select",
        description = "Let the user pick more than one option (default false)."
    )]
    pub multi_select: Option<bool>,

    /// Explicitly recommended option for this question; must exactly match
    /// one of this question's explicit option labels. Any present-but-invalid
    /// value is rejected as invalid arguments in both modes. Required on
    /// every question of an effective advisory (`blocking = false`) call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Optional recommended option; must exactly match one option label. Required for every question when blocking=false."
    )]
    pub recommended_option: Option<String>,

    /// See `QuestionOption.id`. Hidden from the JSON schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(skip)]
    pub id: Option<String>,
}

/// Input for the `AskUserQuestion` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AskUserQuestionInput {
    /// The questions to ask, each with its own options. At least one question
    /// is required.
    #[schemars(description = "The questions to ask, each with its own options.")]
    pub questions: Vec<Question>,

    /// `true` (default): the tool waits for the user's necessary decision.
    /// `false`: present optional advice and continue on the Agent's
    /// recommendation without waiting for a user answer. The effective mode
    /// also depends on the user's persisted preference.
    #[serde(default = "default_input_blocking")]
    #[schemars(
        description = "true (default) waits for the user's necessary decision; false presents optional advice and continues without a user answer."
    )]
    pub blocking: bool,
}

/// Serde default for `AskUserQuestionInput.blocking`: absent means blocking.
fn default_input_blocking() -> bool {
    true
}

/// `AskUserQuestion` tool.
///
/// Blocks inside `run()` until the user responds or the configured wait
/// budget elapses for the whole questionnaire (default [`RESPONSE_TIMEOUT`],
/// 30 minutes). Sends a request over an in-process mpsc channel to a
/// session-owned coordinator (in shell), which performs an ACP
/// `ext_method` round-trip to the client/pager. The response is sent back
/// over a oneshot channel and formatted into the model-visible tool result.
///
/// Params: [`AskUserQuestionParams`] — timeout policy resolved by the shell
/// across its config tiers.
#[derive(Debug, Default)]
pub struct AskUserQuestionTool;

impl crate::types::tool_metadata::ToolMetadata for AskUserQuestionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::AskUser
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn emitted_notifications(&self) -> &'static [&'static str] {
        &["UserQuestionAsked"]
    }

    fn description_template(&self) -> &str {
        r#"Ask the user one or more multiple-choice questions.

- Every question automatically gets an "Other" choice where the user can type their own answer.
- Put your recommended option first and append "(Recommended)" to its label.
- Keep blocking=true (default) when you need the user's decision. Use blocking=false only to present optional advice you can proceed on: every question must set recommended_option to one of its exact option labels; the tool returns once the questions are shown, and the user has not answered."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        // Standalone. The plan-mode prompt note is
        // `${% if tools.by_kind.plan_control %}`-guarded, so it renders
        // fine without the plan tools.
        Expr::True
    }
}

impl tool_runtime::Tool for AskUserQuestionTool {
    type Args = AskUserQuestionInput;
    type Output = AskUserQuestionOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new("ask_user_question").expect("valid tool id")
    }

    fn description(&self, _ctx: &::tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(
            "ask_user_question",
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::None,
            ..Default::default()
        }
    }

    #[tracing::instrument(
        name = "tool.ask_user_question",
        skip_all,
        fields(question_count = input.questions.len()),
    )]
    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: AskUserQuestionInput,
    ) -> Result<AskUserQuestionOutput, tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let question_count = input.questions.len();

        if question_count == 0 {
            return Err(tool_runtime::ToolError::invalid_arguments(
                "questions must contain at least one question",
            ));
        }

        // ── Step 1: Validate unambiguous question and option text ───────────────────────
        {
            let mut seen = std::collections::HashSet::new();
            for q in &input.questions {
                if !seen.insert(&q.question) {
                    return Err(tool_runtime::ToolError::invalid_arguments(format!(
                        "Duplicate question text: \"{}\"",
                        q.question
                    )));
                }
                let mut labels = std::collections::HashSet::new();
                for option in &q.options {
                    if !labels.insert(&option.label) {
                        return Err(tool_runtime::ToolError::invalid_arguments(format!(
                            "Duplicate option label {:?} in question {:?}",
                            option.label, q.question
                        )));
                    }
                }
                // An explicit recommendation must exactly match one of this
                // question's option labels in both modes; nothing is guessed
                // from label text or ordering.
                if let Some(rec) = &q.recommended_option
                    && !q.options.iter().any(|o| &o.label == rec)
                {
                    return Err(tool_runtime::ToolError::invalid_arguments(format!(
                        "recommended_option {:?} does not match any option label in question {:?}",
                        rec, q.question
                    )));
                }
            }
        }

        // ── Step 2: Obtain sender and one effective policy snapshot ────
        // The shell-injected resolver (when present) wins over the injected
        // Params; either way the lock is released before any wait below.
        let (sender, params) = {
            let res = resources.lock().await;
            let params = if let Some(resolver) = res.get::<UserQuestionPolicyResolver>() {
                resolver.0().map_err(|error| {
                    tool_runtime::ToolError::custom(
                        "ask_user_question_policy",
                        format!("failed to resolve ask_user_question policy: {error}"),
                    )
                })?
            } else {
                res.get::<crate::types::resources::Params<AskUserQuestionParams>>()
                    .map(|p| p.0)
                    .unwrap_or_default()
            };
            (res.get::<UserQuestionSender>().cloned(), params)
        };

        let sender = sender.ok_or_else(|| {
            tool_runtime::ToolError::custom("missing_resource", "UserQuestionSender".to_string())
        })?;

        // Effective mode: advisory only when the model asked for
        // non-blocking AND the user's policy allows it. A disabled
        // preference forces the blocking form; already-admitted questions
        // keep this committed value for the whole call.
        let advisory = !input.blocking && params.allow_non_blocking;
        if advisory {
            for q in &input.questions {
                if q.recommended_option.is_none() {
                    return Err(tool_runtime::ToolError::invalid_arguments(format!(
                        "question {:?} is missing recommended_option; every question in a non-blocking call needs one exact option label",
                        q.question
                    )));
                }
            }
        }
        let wait = params.wait_budget();

        // ── Step 3: Create oneshot and deadline ────────────────────────
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();
        // Blocking: the user-wait budget. Advisory: the fixed presentation
        // budget, shared with the shell worker, counted from before enqueue
        // (queue time included).
        let deadline = if advisory {
            Some(tokio::time::Instant::now() + ADVISORY_PRESENTATION_BUDGET)
        } else {
            wait.and_then(|duration| tokio::time::Instant::now().checked_add(duration))
        };

        // ── Step 4: Send UserQuestionRequest (effective blocking) ──────
        let request = types::UserQuestionRequest {
            tool_call_id: ctx.call_id.as_str().to_owned(),
            questions: input.questions.clone(),
            blocking: !advisory,
            deadline,
            result_tx,
        };

        if sender.0.send(request).is_err() {
            return Err(tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                "User question session ended unexpectedly (coordinator channel closed)",
            ));
        }

        // ── Step 5: Emit UserQuestionAsked ─────────────────────────────
        {
            let questions_json = serde_json::to_value(&input.questions)
                .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
            let res = resources.lock().await;
            if let Some(handle) = res.get::<NotificationHandle>() {
                handle.0.send_user_question_asked(UserQuestionAsked {
                    tool_call_id: ctx.call_id.as_str().to_owned(),
                    questions_json,
                });
            }
        };
        tracing::info!(
            question_count,
            timeout_secs = ?wait.map(|d| d.as_secs()),
            "Asked user questions, blocking for response"
        );

        // ── Step 6: Block on the oneshot result (whole batch, one timer) ─
        // A single pending-decision timeout covers the questionnaire, not per
        // question: N questions in one call share one wait.
        // A `None` budget (`timeout_enabled = false`) runs the same await with
        // no timer, normalized into the timed shape so one match handles both.
        let outcome = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, result_rx).await,
            None => Ok(result_rx.await),
        };
        let result = match outcome {
            Ok(Ok(r)) => r,
            Ok(Err(_recv_error)) => {
                return Err(tool_runtime::ToolError::execution(
                    tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                    "User question session ended unexpectedly (client may have disconnected)",
                ));
            }
            Err(_elapsed) => {
                if advisory {
                    tracing::warn!(
                        question_count,
                        "Advisory question presentation was not acknowledged within the budget"
                    );
                    // Presentation failed: an infrastructure failure, never a
                    // fake user cancel. Drop the receiver; the shell worker
                    // races `result_tx.closed()` and stops waiting.
                    return Err(tool_runtime::ToolError::execution(
                        tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                        "Advisory question presentation timed out before the client acknowledged it",
                    ));
                }
                tracing::info!(
                    question_count,
                    timeout_secs = ?wait.map(|d| d.as_secs()),
                    "User question timed out; continuing without answers"
                );
                // Drop the oneshot receiver on return. The shell coordinator
                // races `result_tx.closed()` against ACP so it unblocks and
                // can open the next questionnaire (stale UI is cancelled when
                // a new ext_method arrives). Same model text as cancel.
                return Ok(AskUserQuestionOutput {
                    message: format::CANCEL_TEXT.to_string(),
                    advisory: None,
                });
            }
        };

        // ── Step 7: Map result to formatter or error ────────────────────
        // The two modes accept disjoint response sets: advisory only the
        // presentation ACK, blocking only the four user paths. A late human
        // answer never becomes an advisory tool result and vice versa.
        if advisory {
            return match result {
                Ok(UserQuestionResponse::Presented) => Ok(AskUserQuestionOutput {
                    message: format::format_presented_advisory(&input.questions),
                    advisory: Some(AdvisoryQuestion {
                        tool_call_id: ctx.call_id.as_str().to_owned(),
                        questions: input.questions.clone(),
                    }),
                }),
                Ok(other) => Err(tool_runtime::ToolError::execution(
                    tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                    format!(
                        "Client returned a user response to an advisory question (expected presentation acknowledgement): {other:?}"
                    ),
                )),
                Err(error) => Err(map_user_question_error(error)),
            };
        }
        match result {
            Ok(UserQuestionResponse::Accepted {
                answers,
                annotations,
            }) => {
                let message = format::format_accepted_tool_result(&answers, &annotations);
                Ok(AskUserQuestionOutput {
                    message,
                    advisory: None,
                })
            }
            Ok(UserQuestionResponse::ChatAboutThis {
                questions,
                partial_answers,
                annotations,
            }) => {
                let message = format::format_chat_about_this(
                    &questions,
                    &partial_answers,
                    annotations.as_ref(),
                );
                Ok(AskUserQuestionOutput {
                    message,
                    advisory: None,
                })
            }
            Ok(UserQuestionResponse::SkipInterview {
                questions,
                partial_answers,
                annotations,
            }) => {
                let message = format::format_skip_interview(
                    &questions,
                    &partial_answers,
                    annotations.as_ref(),
                );
                Ok(AskUserQuestionOutput {
                    message,
                    advisory: None,
                })
            }
            Ok(UserQuestionResponse::Cancelled) => Ok(AskUserQuestionOutput {
                message: format::CANCEL_TEXT.to_string(),
                advisory: None,
            }),
            Ok(UserQuestionResponse::Presented) => Err(tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                "Client returned a presentation acknowledgement to a blocking question",
            )),
            Err(error) => Err(map_user_question_error(error)),
        }
    }
}

/// Map an infrastructure failure from the coordinator to a tool error.
fn map_user_question_error(error: types::UserQuestionError) -> tool_runtime::ToolError {
    match error {
        types::UserQuestionError::TransportError(msg) => tool_runtime::ToolError::execution(
            tool_protocol::ToolId::new("ask_user_question").expect("valid"),
            format!("Failed to reach the client for user question: {msg}"),
        ),
        types::UserQuestionError::MalformedResponse(msg) => tool_runtime::ToolError::execution(
            tool_protocol::ToolId::new("ask_user_question").expect("valid"),
            format!("Client returned an invalid response to user question: {msg}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::resources::{Resources, SharedResources};
    use crate::types::tool_metadata::test_ctx_with_call_id;
    use indexmap::IndexMap;
    use tokio::sync::mpsc;

    fn make_question(question: &str, labels: &[&str]) -> Question {
        Question {
            question: question.to_string(),
            options: labels
                .iter()
                .map(|l| QuestionOption {
                    label: l.to_string(),
                    description: format!("Description for {l}"),
                    preview: None,
                    id: None,
                })
                .collect(),
            multi_select: None,
            recommended_option: None,
            id: None,
        }
    }

    fn timeout_secs(seconds: u64) -> std::num::NonZeroU64 {
        std::num::NonZeroU64::new(seconds).expect("test timeout must be non-zero")
    }

    /// Create resources with a UserQuestionSender injected.
    /// Returns (shared_resources, rx) where rx receives UserQuestionRequests.
    fn resources_with_sender() -> (
        SharedResources,
        mpsc::UnboundedReceiver<types::UserQuestionRequest>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut resources = Resources::new();
        resources.insert(UserQuestionSender(tx));
        (resources.into_shared(), rx)
    }

    /// Like [`resources_with_sender`], with shell-resolved params injected.
    fn resources_with_sender_and_params(
        params: AskUserQuestionParams,
    ) -> (
        SharedResources,
        mpsc::UnboundedReceiver<types::UserQuestionRequest>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut resources = Resources::new();
        resources.insert(UserQuestionSender(tx));
        resources.insert(crate::types::resources::Params(params));
        (resources.into_shared(), rx)
    }

    // ── Basic tool metadata tests ────────────────────────────────────────

    #[test]
    fn tool_name_and_description() {
        let tool = AskUserQuestionTool;
        assert_eq!(tool_runtime::Tool::id(&tool).as_str(), "ask_user_question");
        let desc = crate::types::tool_metadata::ToolMetadata::description_template(&tool);
        assert!(desc.contains("Ask the user"));
        assert!(desc.contains("Other"));
        assert!(desc.contains("(Recommended)"));
    }

    #[test]
    fn tool_access_is_internal_control() {
        assert_eq!(
            tool_runtime::Tool::capabilities(&AskUserQuestionTool).max_access,
            tool_protocol::ToolAccess::None
        );
    }

    #[test]
    fn tool_kind_is_ask_user() {
        assert_eq!(
            crate::types::tool_metadata::ToolMetadata::kind(&AskUserQuestionTool),
            ToolKind::AskUser
        );
    }

    #[test]
    fn input_deserializes_from_json() {
        let json = serde_json::json!({
            "questions": [{
                "question": "Pick DB?",
                "options": [
                    {"label": "Postgres", "description": "Relational DB"},
                    {"label": "SQLite", "description": "Embedded SQL database", "preview": "```\nSELECT 1;\n```"}
                ],
                "multi_select": false
            }]
        });

        let input: AskUserQuestionInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.questions.len(), 1);
        assert_eq!(input.questions[0].question, "Pick DB?");
        assert_eq!(input.questions[0].options.len(), 2);
        assert_eq!(input.questions[0].options[0].label, "Postgres");
        assert!(input.questions[0].options[0].preview.is_none());
        assert_eq!(input.questions[0].options[1].label, "SQLite");
        assert!(input.questions[0].options[1].preview.is_some());
        assert_eq!(input.questions[0].multi_select, Some(false));
    }

    #[test]
    fn model_schema_advertises_snake_case_multi_select() {
        let schema = schemars::schema_for!(AskUserQuestionInput);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(
            json.contains("multi_select"),
            "model schema should advertise multi_select: {json}"
        );
        assert!(
            !json.contains("multiSelect"),
            "model schema should not advertise camelCase multiSelect: {json}"
        );
    }

    #[test]
    fn input_accepts_snake_case_multi_select() {
        let json = serde_json::json!({
            "questions": [{
                "question": "Pick DB?",
                "options": [{"label": "Postgres", "description": "Relational DB"}],
                "multi_select": true
            }]
        });
        let input: AskUserQuestionInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.questions[0].multi_select, Some(true));
    }

    #[tokio::test]
    async fn empty_questions_are_invalid() {
        let resources = Resources::new();
        let shared = resources.into_shared();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            questions: vec![],
            blocking: true,
        };

        let error =
            tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "test-call"), input)
                .await
                .expect_err("empty question list must fail");
        assert!(error.to_string().contains("at least one question"));
    }

    #[tokio::test]
    async fn missing_coordinator_is_an_error() {
        let resources = Resources::new();
        let shared = resources.into_shared();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Pick one?", &["A", "B"])],
        };

        let error = tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "call-q"), input)
            .await
            .expect_err("coordinator is required");
        assert!(error.to_string().contains("UserQuestionSender"));
    }

    // ── Validation tests ─────────────────────────────────────────────────

    #[tokio::test]
    async fn validate_duplicate_question_text() {
        let resources = Resources::new();
        let shared = resources.into_shared();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![
                make_question("Same question?", &["A"]),
                make_question("Same question?", &["B"]),
            ],
        };

        let err = tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "test-call"), input)
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("Duplicate question text"), "got: {msg}");
        assert!(msg.contains("Same question?"), "got: {msg}");
    }

    #[tokio::test]
    async fn duplicate_option_labels_rejected_before_send() {
        {
            let (shared, mut rx) = resources_with_sender();
            let mut question = make_question("Which version?", &["Same", "Same"]);
            question.id = Some("q1".into());
            question.options[0].id = Some("first".into());
            question.options[1].id = Some("second".into());
            question.options[1].description = "Different meaning".into();
            let input = AskUserQuestionInput {
                blocking: true,
                questions: vec![question],
            };
            let result = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                tool_runtime::Tool::run(
                    &AskUserQuestionTool,
                    test_ctx_with_call_id(shared, "duplicates"),
                    input,
                ),
            )
            .await
            .expect("ambiguous options must fail before waiting for a response");
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("Duplicate option label")
            );
            assert!(rx.try_recv().is_err(), "no question should be sent");
        }
    }

    #[tokio::test]
    async fn option_labels_can_repeat_across_questions() {
        let (shared, mut rx) = resources_with_sender();
        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![
                make_question("First?", &["Yes", "No"]),
                make_question("Second?", &["Yes", "No"]),
            ],
        };
        let handle = tokio::spawn(async move {
            tool_runtime::Tool::run(
                &AskUserQuestionTool,
                test_ctx_with_call_id(shared, "cross-question"),
                input,
            )
            .await
        });
        let request = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let answers = IndexMap::from([
            ("First?".into(), vec!["Yes".into()]),
            ("Second?".into(), vec!["No".into()]),
        ]);
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Accepted {
                answers,
                annotations: None,
            }))
            .unwrap();
        let output = handle.await.unwrap().unwrap();
        assert!(output.message.contains("First?"));
        assert!(output.message.contains("Second?"));
    }

    // ── Blocking round-trip tests ────────────────────────────────────────

    #[tokio::test]
    async fn blocking_round_trip_accepted() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-1"), input).await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        assert_eq!(request.tool_call_id, "tc-1");
        assert_eq!(request.questions.len(), 1);

        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Redis".to_string()]);

        request
            .result_tx
            .send(Ok(UserQuestionResponse::Accepted {
                answers,
                annotations: None,
            }))
            .unwrap();

        let result = handle.await.unwrap().unwrap();
        assert!(
            result
                .message
                .starts_with("User has answered your questions:")
        );
        assert!(result.message.contains("\"Which database?\"=\"Redis\""));
    }

    #[tokio::test]
    async fn blocking_round_trip_cancelled() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Q?", &["A"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-4"), input).await
            }
        });

        let request = rx.recv().await.unwrap();
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Cancelled))
            .unwrap();

        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.message, format::CANCEL_TEXT);
    }

    /// Whole questionnaire (multi-question batch) shares one default timer.
    #[tokio::test(start_paused = true)]
    async fn blocking_times_out_after_default_budget_for_batch() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![
                make_question("Q1?", &["A", "B"]),
                make_question("Q2?", &["C", "D"]),
            ],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-timeout"), input)
                    .await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        assert_eq!(request.questions.len(), 2);
        tokio::time::advance(RESPONSE_TIMEOUT + std::time::Duration::from_secs(1)).await;

        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.message, format::CANCEL_TEXT);
    }

    #[tokio::test(start_paused = true)]
    async fn answer_before_timeout_still_succeeds() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-ok"), input).await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        let advance = RESPONSE_TIMEOUT
            .checked_div(6)
            .unwrap_or(std::time::Duration::from_secs(1))
            .max(std::time::Duration::from_secs(1));
        tokio::time::advance(advance).await;

        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Redis".to_string()]);
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Accepted {
                answers,
                annotations: None,
            }))
            .unwrap();

        let result = handle.await.unwrap().unwrap();
        assert!(result.message.contains("\"Which database?\"=\"Redis\""));
    }

    // ── Configured timeout params tests ──────────────────────────────────

    /// Concrete defaults remain bounded; `timeout_enabled = false` disarms the
    /// timer and zero is rejected at deserialization.
    #[test]
    fn wait_budget_mapping() {
        assert_eq!(
            AskUserQuestionParams::default().wait_budget(),
            Some(RESPONSE_TIMEOUT),
        );
        assert_eq!(
            RESPONSE_TIMEOUT,
            std::time::Duration::from_secs(30 * 60),
            "default ask_user_question budget is 30 minutes"
        );
        let disabled = AskUserQuestionParams {
            timeout_enabled: false,
            timeout_secs: timeout_secs(30),
            allow_non_blocking: true,
        };
        assert_eq!(disabled.wait_budget(), None, "disabled timer waits forever");
        assert!(
            serde_json::from_value::<AskUserQuestionParams>(serde_json::json!({
                "timeout_enabled": true,
                "timeout_secs": 0
            }))
            .is_err(),
            "zero must be rejected instead of activating a fallback"
        );
    }

    /// A short shell-resolved budget fires with the same silent-skip text as
    /// a user dismiss.
    #[tokio::test(start_paused = true)]
    async fn short_params_timeout_fires_with_cancel_text() {
        let (shared, mut rx) = resources_with_sender_and_params(AskUserQuestionParams {
            timeout_enabled: true,
            timeout_secs: timeout_secs(5),
            allow_non_blocking: true,
        });
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Q?", &["A", "B"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-short"), input)
                    .await
            }
        });

        let _request = rx.recv().await.expect("should receive request");
        tokio::time::advance(std::time::Duration::from_secs(6)).await;

        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.message, format::CANCEL_TEXT);
    }

    /// `timeout_enabled = false` waits arbitrarily long — an answer far past
    /// the default budget still succeeds instead of timing out.
    #[tokio::test(start_paused = true)]
    async fn timeout_disabled_waits_beyond_default_budget() {
        let (shared, mut rx) = resources_with_sender_and_params(AskUserQuestionParams {
            timeout_enabled: false,
            timeout_secs: timeout_secs(1),
            allow_non_blocking: true,
        });
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-forever"), input)
                    .await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        tokio::time::advance(RESPONSE_TIMEOUT * 4).await;

        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Redis".to_string()]);
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Accepted {
                answers,
                annotations: None,
            }))
            .unwrap();

        let result = handle.await.unwrap().unwrap();
        assert!(result.message.contains("\"Which database?\"=\"Redis\""));
    }

    // ── Failure path tests ───────────────────────────────────────────────

    #[tokio::test]
    async fn channel_drop_returns_error() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Q?", &["A"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-5"), input).await
            }
        });

        let request = rx.recv().await.unwrap();
        drop(request.result_tx);

        let err = handle.await.unwrap().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unexpectedly"), "msg: {msg}");
    }

    #[tokio::test]
    async fn transport_error_not_cancel() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_question("Q?", &["A"])],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "tc-6"), input).await
            }
        });

        let request = rx.recv().await.unwrap();
        request
            .result_tx
            .send(Err(UserQuestionError::TransportError(
                "connection reset".to_string(),
            )))
            .unwrap();

        let err = handle.await.unwrap().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Failed to reach the client"), "msg: {msg}");
        assert!(msg.contains("connection reset"), "msg: {msg}");
    }

    // ── Advisory (blocking = false) tests ───────────────────────────────

    /// Question with an explicit recommendation on the first label.
    fn make_advisory_question(question: &str, labels: &[&str], recommended: &str) -> Question {
        let mut q = make_question(question, labels);
        q.recommended_option = Some(recommended.to_string());
        q
    }

    fn advisory_input() -> AskUserQuestionInput {
        AskUserQuestionInput {
            blocking: false,
            questions: vec![make_advisory_question(
                "Which database?",
                &["Redis", "Postgres"],
                "Redis",
            )],
        }
    }

    #[tokio::test]
    async fn advisory_presented_round_trip() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;
        let input = advisory_input();

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "adv-1"), input).await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        assert_eq!(request.tool_call_id, "adv-1");
        // The request carries the effective mode, not the requested one.
        assert!(
            !request.blocking,
            "effective advisory request must be non-blocking"
        );

        request
            .result_tx
            .send(Ok(UserQuestionResponse::Presented))
            .unwrap();

        let output = handle.await.unwrap().unwrap();
        // Structured advisory payload only after the presentation ACK.
        let advisory = output.advisory.expect("presented advisory must be Some");
        assert_eq!(advisory.tool_call_id, "adv-1");
        assert_eq!(advisory.questions.len(), 1);
        assert_eq!(
            advisory.questions[0].recommended_option.as_deref(),
            Some("Redis")
        );
        // Model text names the recommendation, its provenance, and the fact
        // that the user has not answered.
        assert!(output.message.contains("\"Which database?\""));
        assert!(output.message.contains("\"Redis\""));
        assert!(output.message.contains("Agent recommendation"));
        assert!(output.message.contains("NOT answered"));
        assert!(!output.message.contains("User has answered"));
    }

    #[tokio::test]
    async fn advisory_missing_recommendation_rejected_before_send() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: false,
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "adv-missing"), input),
        )
        .await
        .expect("missing recommendation must fail before waiting");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("recommended_option"), "got: {msg}");
        assert!(rx.try_recv().is_err(), "no question should be sent");
    }

    #[tokio::test]
    async fn invalid_recommended_label_rejected_in_both_modes() {
        for blocking in [true, false] {
            let resources = Resources::new();
            let shared = resources.into_shared();
            let input = AskUserQuestionInput {
                blocking,
                questions: vec![make_advisory_question(
                    "Which database?",
                    &["Redis", "Postgres"],
                    "Redis (Recommended)",
                )],
            };
            let err = tool_runtime::Tool::run(
                &AskUserQuestionTool,
                test_ctx_with_call_id(shared, "adv-bad-label"),
                input,
            )
            .await
            .expect_err("explicit invalid recommendation must be rejected");
            let msg = err.to_string();
            assert!(
                msg.contains("does not match any option label"),
                "got: {msg}"
            );
        }
    }

    /// Advisory timeout (5s shared budget, queue included) is an
    /// infrastructure failure, never a fake user cancel.
    #[tokio::test(start_paused = true)]
    async fn advisory_timeout_is_failure_not_cancel() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;
        let input = advisory_input();

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "adv-t"), input).await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        assert!(request.deadline.is_some(), "advisory always has a deadline");
        // Never reply; the fixed presentation budget expires.
        tokio::time::advance(ADVISORY_PRESENTATION_BUDGET + std::time::Duration::from_secs(1))
            .await;

        let err = handle
            .await
            .unwrap()
            .expect_err("advisory timeout must fail");
        let msg = err.to_string();
        assert!(msg.contains("timed out"), "got: {msg}");
        assert!(
            !msg.contains(format::CANCEL_TEXT),
            "must not fake a user cancel"
        );
    }

    #[tokio::test]
    async fn advisory_receiving_human_answer_is_error() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;
        let input = advisory_input();

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "adv-a"), input).await
            }
        });

        let request = rx.recv().await.unwrap();
        let mut answers = IndexMap::new();
        answers.insert("Which database?".to_string(), vec!["Redis".to_string()]);
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Accepted {
                answers,
                annotations: None,
            }))
            .unwrap();

        let err = handle
            .await
            .unwrap()
            .expect_err("human answers cannot become advisory results");
        assert!(
            err.to_string()
                .contains("expected presentation acknowledgement")
        );
    }

    #[tokio::test]
    async fn blocking_receiving_presented_is_error() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            blocking: true,
            questions: vec![make_advisory_question("Q?", &["A", "B"], "A")],
        };

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "blk-p"), input).await
            }
        });

        let request = rx.recv().await.unwrap();
        assert!(request.blocking);
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Presented))
            .unwrap();

        let err = handle
            .await
            .unwrap()
            .expect_err("blocking questions cannot accept a presentation ACK");
        assert!(err.to_string().contains("blocking question"));
    }

    /// A disabled `allow_non_blocking` preference forces the blocking form,
    /// even when the model requested `blocking = false`.
    #[tokio::test(start_paused = true)]
    async fn preference_disabled_forces_blocking() {
        let (shared, mut rx) = resources_with_sender_and_params(AskUserQuestionParams {
            timeout_enabled: true,
            timeout_secs: timeout_secs(5),
            allow_non_blocking: false,
        });
        let tool = AskUserQuestionTool;
        let input = advisory_input();

        let handle = tokio::spawn({
            let shared = shared.clone();
            async move {
                tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "force-blk"), input)
                    .await
            }
        });

        let request = rx.recv().await.expect("should receive request");
        assert!(request.blocking, "forced blocking keeps the wait policy");
        // Unanswered blocking question: old cancel-text policy, advisory None.
        tokio::time::advance(std::time::Duration::from_secs(6)).await;
        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.message, format::CANCEL_TEXT);
        assert!(result.advisory.is_none());
    }

    #[tokio::test]
    async fn resolver_failure_is_admission_failure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut resources = Resources::new();
        resources.insert(UserQuestionSender(tx));
        resources.insert(UserQuestionPolicyResolver(std::sync::Arc::new(|| {
            Err("config.toml [toolset.ask_user_question] failed to parse".to_string())
        })));
        let shared = resources.into_shared();

        let input = advisory_input();
        let err = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            tool_runtime::Tool::run(
                &AskUserQuestionTool,
                test_ctx_with_call_id(shared, "res-fail"),
                input,
            ),
        )
        .await
        .expect("resolver failure must fail admission without waiting")
        .unwrap_err();
        assert!(err.to_string().contains("failed to resolve"), "got: {err}");
        assert!(rx.try_recv().is_err(), "no question should be sent");
    }

    /// The shell resolver takes precedence over an injected Params snapshot.
    #[tokio::test]
    async fn resolver_overrides_injected_params() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut resources = Resources::new();
        resources.insert(UserQuestionSender(tx));
        // Params say allowed; resolver says disabled — resolver wins.
        resources.insert(crate::types::resources::Params(AskUserQuestionParams {
            allow_non_blocking: true,
            ..Default::default()
        }));
        resources.insert(UserQuestionPolicyResolver(std::sync::Arc::new(|| {
            Ok(AskUserQuestionParams {
                allow_non_blocking: false,
                ..Default::default()
            })
        })));
        let shared = resources.into_shared();

        let handle = tokio::spawn(async move {
            tool_runtime::Tool::run(
                &AskUserQuestionTool,
                test_ctx_with_call_id(shared, "res-win"),
                advisory_input(),
            )
            .await
        });
        let request = rx.recv().await.expect("should receive request");
        assert!(request.blocking, "resolver snapshot wins over Params");
        request
            .result_tx
            .send(Ok(UserQuestionResponse::Cancelled))
            .unwrap();
        let result = handle.await.unwrap().unwrap();
        assert_eq!(result.message, format::CANCEL_TEXT);
    }

    // ── Wire/default serde for the new fields ──────────────────────────

    #[test]
    fn input_blocking_defaults_true() {
        let input: AskUserQuestionInput = serde_json::from_value(serde_json::json!({
            "questions": [{ "question": "Q?", "options": [{"label": "A", "description": "d"}] }]
        }))
        .unwrap();
        assert!(input.blocking);
        assert!(input.questions[0].recommended_option.is_none());
        // Recommendation round-trips when present.
        let input: AskUserQuestionInput = serde_json::from_value(serde_json::json!({
            "questions": [{
                "question": "Q?",
                "options": [{"label": "A", "description": "d"}],
                "recommended_option": "A"
            }],
            "blocking": false
        }))
        .unwrap();
        assert!(!input.blocking);
        assert_eq!(input.questions[0].recommended_option.as_deref(), Some("A"));
    }

    #[test]
    fn output_advisory_omitted_when_none() {
        let json = serde_json::to_value(AskUserQuestionOutput {
            message: "m".to_string(),
            advisory: None,
        })
        .unwrap();
        assert!(json.get("advisory").is_none());

        let json = serde_json::to_value(AskUserQuestionOutput {
            message: "m".to_string(),
            advisory: Some(AdvisoryQuestion {
                tool_call_id: "tc".to_string(),
                questions: vec![make_advisory_question("Q?", &["A"], "A")],
            }),
        })
        .unwrap();
        assert_eq!(json["advisory"]["tool_call_id"], "tc");
        assert_eq!(json["advisory"]["questions"].as_array().unwrap().len(), 1);
    }
}
