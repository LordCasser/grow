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
use crate::types::output::AskUserQuestionOutput;
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
}

impl Default for AskUserQuestionParams {
    fn default() -> Self {
        Self {
            timeout_enabled: DEFAULT_ASK_USER_QUESTION_TIMEOUT_ENABLED,
            timeout_secs: std::num::NonZeroU64::new(RESPONSE_TIMEOUT.as_secs())
                .expect("default timeout is non-zero"),
        }
    }
}

crate::register_resource!("grow_build", "AskUserQuestion", AskUserQuestionParams);

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

    /// Internal flag: when `true`, the tool result is formatted in the
    /// alternate shape (referenced by id, not label).
    /// Skipped on the wire and from the JSON schema so the model never
    /// sees or controls this field.
    #[serde(default, skip)]
    #[schemars(skip)]
    pub use_id_keyed_format: bool,
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
- Put your recommended option first and append "(Recommended)" to its label."#
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
            tool_scope: tool_protocol::ToolScope::Read,
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

        // ── Step 1: Validate unique question text ───────────────────────
        {
            let mut seen = std::collections::HashSet::new();
            for q in &input.questions {
                if !seen.insert(&q.question) {
                    return Err(tool_runtime::ToolError::invalid_arguments(format!(
                        "Duplicate question text: \"{}\"",
                        q.question
                    )));
                }
            }
        }

        // ── Step 2: Obtain UserQuestionSender ───────────────────────────
        let sender = {
            let res = resources.lock().await;
            res.get::<UserQuestionSender>().cloned()
        };

        let sender = sender.ok_or_else(|| {
            tool_runtime::ToolError::custom("missing_resource", "UserQuestionSender".to_string())
        })?;

        // ── Step 3: Create oneshot ──────────────────────────────────────
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        // ── Step 4: Send UserQuestionRequest ────────────────────────────
        let request = types::UserQuestionRequest {
            tool_call_id: ctx.call_id.as_str().to_owned(),
            questions: input.questions.clone(),
            result_tx,
        };

        if sender.0.send(request).is_err() {
            return Err(tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                "User question session ended unexpectedly (coordinator channel closed)",
            ));
        }

        // ── Step 5: Emit UserQuestionAsked + read the wait budget ───────
        let wait = {
            let questions_json = serde_json::to_value(&input.questions)
                .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
            let res = resources.lock().await;
            if let Some(handle) = res.get::<NotificationHandle>() {
                handle.0.send_user_question_asked(UserQuestionAsked {
                    tool_call_id: ctx.call_id.as_str().to_owned(),
                    questions_json,
                });
            }
            res.get::<crate::types::resources::Params<AskUserQuestionParams>>()
                .map(|p| p.0)
                .unwrap_or_default()
                .wait_budget()
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
        let outcome = match wait {
            Some(dur) => tokio::time::timeout(dur, result_rx).await,
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
                });
            }
        };

        // ── Step 7: Map result to formatter or error ────────────────────
        match result {
            Ok(UserQuestionResponse::Accepted {
                answers,
                annotations,
            }) => {
                let message = if input.use_id_keyed_format {
                    format::format_id_keyed_accepted_tool_result(
                        &input.questions,
                        &answers,
                        &annotations,
                    )
                } else {
                    format::format_accepted_tool_result(&answers, &annotations)
                };
                Ok(AskUserQuestionOutput { message })
            }
            Ok(UserQuestionResponse::ChatAboutThis {
                questions,
                partial_answers,
            }) => {
                let message = format::format_chat_about_this(&questions, &partial_answers);
                Ok(AskUserQuestionOutput { message })
            }
            Ok(UserQuestionResponse::SkipInterview {
                questions,
                partial_answers,
            }) => {
                let message = format::format_skip_interview(&questions, &partial_answers);
                Ok(AskUserQuestionOutput { message })
            }
            Ok(UserQuestionResponse::Cancelled) => Ok(AskUserQuestionOutput {
                message: format::CANCEL_TEXT.to_string(),
            }),
            Err(UserQuestionError::TransportError(msg)) => Err(tool_runtime::ToolError::execution(
                tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                format!("Failed to reach the client for user question: {msg}"),
            )),
            Err(UserQuestionError::MalformedResponse(msg)) => {
                Err(tool_runtime::ToolError::execution(
                    tool_protocol::ToolId::new("ask_user_question").expect("valid"),
                    format!("Client returned an invalid response to user question: {msg}"),
                ))
            }
        }
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
    fn tool_scope_is_read() {
        assert_eq!(
            tool_runtime::Tool::capabilities(&AskUserQuestionTool).tool_scope,
            tool_protocol::ToolScope::Read
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
            use_id_keyed_format: false,
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
            questions: vec![make_question("Pick one?", &["A", "B"])],
            use_id_keyed_format: false,
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
            questions: vec![
                make_question("Same question?", &["A"]),
                make_question("Same question?", &["B"]),
            ],
            use_id_keyed_format: false,
        };

        let err = tool_runtime::Tool::run(&tool, test_ctx_with_call_id(shared, "test-call"), input)
            .await
            .unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("Duplicate question text"), "got: {msg}");
        assert!(msg.contains("Same question?"), "got: {msg}");
    }

    // ── Blocking round-trip tests ────────────────────────────────────────

    #[tokio::test]
    async fn blocking_round_trip_accepted() {
        let (shared, mut rx) = resources_with_sender();
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
            use_id_keyed_format: false,
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
            questions: vec![make_question("Q?", &["A"])],
            use_id_keyed_format: false,
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
            questions: vec![
                make_question("Q1?", &["A", "B"]),
                make_question("Q2?", &["C", "D"]),
            ],
            use_id_keyed_format: false,
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
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
            use_id_keyed_format: false,
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
        });
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            questions: vec![make_question("Q?", &["A", "B"])],
            use_id_keyed_format: false,
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
        });
        let tool = AskUserQuestionTool;

        let input = AskUserQuestionInput {
            questions: vec![make_question("Which database?", &["Redis", "Postgres"])],
            use_id_keyed_format: false,
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
            questions: vec![make_question("Q?", &["A"])],
            use_id_keyed_format: false,
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
            questions: vec![make_question("Q?", &["A"])],
            use_id_keyed_format: false,
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
}
