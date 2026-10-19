use acp_transport::AcpResult;
use acp_transport::protocol as acp;

pub use tools::implementations::grow_build::plan_control::{
    PlanApprovalExtRequest, PlanApprovalExtResponse, PlanApprovalOutcome,
};

use crate::views::prompt_widget::StashedPrompt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanApprovalFocus {
    Preview,
    Prompt,
    Commenting,
}

#[derive(Debug, Clone)]
pub struct PlanComment {
    pub id: u64,
    pub line_range: std::ops::Range<usize>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct PendingPlanApprovalDecision {
    pub session_id: String,
    pub binding_epoch: u32,
    pub response: PlanApprovalExtResponse,
    pub artifact_revision: u64,
    pub artifact_hash: String,
    pub comments: Vec<PlanComment>,
}

pub struct PlanApprovalViewState {
    pub source_session_id: String,
    pub tool_call_id: String,
    pub plan_content: String,
    pub submission_id: u64,
    pub artifact_revision: u64,
    pub artifact_hash: String,
    pub stashed_prompt: StashedPrompt,
    pub response_tx: Option<tokio::sync::oneshot::Sender<AcpResult<acp::ExtResponse>>>,
    pub(crate) decision_response: Option<PlanApprovalExtResponse>,

    pub focus: PlanApprovalFocus,
    pub comments: Vec<PlanComment>,
    pub next_comment_id: u64,
    pub editing_comment_id: Option<u64>,
    pub commenting_range: Option<std::ops::Range<usize>>,

    pub stashed_feedback_prompt: Option<StashedPrompt>,
}

impl PlanApprovalViewState {
    pub fn decision_pending(&self) -> bool {
        self.decision_response.is_some()
    }

    pub fn new(
        request: PlanApprovalExtRequest,
        stashed_prompt: StashedPrompt,
        response_tx: tokio::sync::oneshot::Sender<AcpResult<acp::ExtResponse>>,
    ) -> Self {
        Self {
            source_session_id: request.session_id.clone(),
            tool_call_id: request.tool_call_id,
            plan_content: request.plan_content,
            submission_id: request.submission_id,
            artifact_revision: request.artifact_revision,
            artifact_hash: request.artifact_hash,
            stashed_prompt,
            response_tx: Some(response_tx),
            decision_response: None,
            focus: PlanApprovalFocus::Preview,
            comments: Vec::new(),
            next_comment_id: 0,
            editing_comment_id: None,
            commenting_range: None,
            stashed_feedback_prompt: None,
        }
    }

    pub fn format_feedback(&self, freeform: Option<&str>) -> String {
        let mut parts: Vec<String> = self
            .comments
            .iter()
            .map(|comment| {
                let label = if comment.line_range.len() == 1 {
                    format!("Proposed plan line {}:", comment.line_range.start)
                } else {
                    format!(
                        "Proposed plan lines {}-{}:",
                        comment.line_range.start,
                        comment.line_range.end - 1
                    )
                };
                let snippets = inline_plan_snippets(Some(&self.plan_content), &comment.line_range);
                format!("{label}\n{snippets}\n\nComment:\n{}", comment.text)
            })
            .collect();

        if let Some(text) = freeform
            && !text.trim().is_empty()
        {
            let text = if self.comments.is_empty() {
                text.to_owned()
            } else {
                format!("Additional feedback:\n{text}")
            };
            parts.push(text);
        }

        parts.join("\n\n")
    }
}

fn build_plan_approval_response(
    outcome: PlanApprovalOutcome,
    feedback: Option<String>,
    submission_id: u64,
) -> PlanApprovalExtResponse {
    PlanApprovalExtResponse {
        outcome,
        feedback: feedback.filter(|f| !f.trim().is_empty()),
        submission_id,
        decision_id: uuid::Uuid::new_v4().to_string(),
    }
}

fn send_plan_approval_response(
    tx: tokio::sync::oneshot::Sender<AcpResult<acp::ExtResponse>>,
    response: &PlanApprovalExtResponse,
) {
    let raw = serde_json::value::to_raw_value(response)
        .expect("PlanApprovalExtResponse serialization should not fail");
    tx.send(Ok(acp::ExtResponse::new(raw.into()))).ok();
}

fn send_ext_response(
    state: &mut PlanApprovalViewState,
    outcome: PlanApprovalOutcome,
    feedback: Option<String>,
) -> PlanApprovalExtResponse {
    if let Some(response) = state.decision_response.as_ref() {
        return response.clone();
    }
    let response = build_plan_approval_response(outcome, feedback, state.submission_id);
    state.decision_response = Some(response.clone());
    if let Some(tx) = state.response_tx.take() {
        send_plan_approval_response(tx, &response);
    }
    response
}

impl PlanApprovalViewState {
    pub fn send_approved(&mut self, feedback: Option<String>) -> PlanApprovalExtResponse {
        send_ext_response(self, PlanApprovalOutcome::Approved, feedback)
    }

    pub fn send_abandoned(&mut self) -> PlanApprovalExtResponse {
        send_ext_response(self, PlanApprovalOutcome::Abandoned, None)
    }

    pub fn send_cancelled(&mut self, feedback: Option<String>) -> PlanApprovalExtResponse {
        send_ext_response(self, PlanApprovalOutcome::Cancelled, feedback)
    }

    pub fn send_deferred(&mut self, feedback: Option<String>) -> PlanApprovalExtResponse {
        send_ext_response(self, PlanApprovalOutcome::Deferred, feedback)
    }

    /// Dismisses a stale transport waiter without turning cleanup into a user decision.
    pub fn close_stale_waiter(&mut self) {
        drop(self.response_tx.take());
    }
}

pub(crate) fn inline_plan_snippets(
    plan_content: Option<&str>,
    range: &std::ops::Range<usize>,
) -> String {
    let Some(plan_content) = plan_content else {
        return "> [plan content unavailable]".to_owned();
    };
    let lines: Vec<&str> = plan_content.lines().collect();
    if range.start == 0 || range.start >= range.end || range.start > lines.len() {
        return "> [selected lines unavailable]".to_owned();
    }

    let end = range.end.saturating_sub(1).min(lines.len());
    if end < range.start {
        return "> [selected lines unavailable]".to_owned();
    }

    lines[range.start - 1..end]
        .iter()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn format_plan_comments(comments: &[PlanComment], plan_content: Option<&str>) -> String {
    comments
        .iter()
        .map(|comment| {
            let label = if comment.line_range.len() == 1 {
                format!("Proposed plan line {}:", comment.line_range.start)
            } else {
                format!(
                    "Proposed plan lines {}-{}:",
                    comment.line_range.start,
                    comment.line_range.end - 1
                )
            };
            let snippets = inline_plan_snippets(plan_content, &comment.line_range);
            format!("{label}\n{snippets}\n\nComment:\n{}", comment.text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_state() -> (
        PlanApprovalViewState,
        tokio::sync::oneshot::Receiver<AcpResult<acp::ExtResponse>>,
    ) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let request = PlanApprovalExtRequest {
            session_id: "test-session".into(),
            tool_call_id: "call_123".into(),
            plan_content: "# Plan\n\n## Step 1\nDo something".into(),
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
        };
        let state = PlanApprovalViewState::new(
            request,
            StashedPrompt {
                text: "stashed text".into(),
                cursor: 0,
                images: Vec::new(),
                chip_elements: Vec::new(),
                image_counter: 0,
                image_undo_stash: Vec::new(),
            },
            tx,
        );
        (state, rx)
    }

    #[test]
    fn test_send_approved() {
        let (mut state, mut rx) = make_test_state();
        let response = state.send_approved(None);
        let resp = rx.try_recv().expect("should receive response");
        let raw = resp.expect("should be Ok");
        let parsed: serde_json::Value =
            serde_json::from_str(raw.0.get()).expect("should be valid JSON");
        assert_eq!(parsed["outcome"], "approved");
        assert_eq!(parsed["submissionId"], 1);
        assert_eq!(
            parsed["decisionId"].as_str(),
            Some(response.decision_id.as_str())
        );
        assert!(uuid::Uuid::parse_str(parsed["decisionId"].as_str().unwrap()).is_ok());
        assert!(parsed.get("feedback").is_none());
    }

    #[test]
    fn test_send_approved_with_feedback() {
        let (mut state, mut rx) = make_test_state();
        let response = state.send_approved(Some("keep the public API stable".into()));
        let resp = rx.try_recv().expect("should receive response");
        let raw = resp.expect("should be Ok");
        let parsed: serde_json::Value =
            serde_json::from_str(raw.0.get()).expect("should be valid JSON");
        assert_eq!(parsed["outcome"], "approved");
        assert_eq!(parsed["submissionId"], 1);
        assert_eq!(
            parsed["decisionId"].as_str(),
            Some(response.decision_id.as_str())
        );
        assert_eq!(parsed["feedback"], "keep the public API stable");
    }

    #[test]
    fn test_send_cancelled_with_feedback() {
        let (mut state, mut rx) = make_test_state();
        let response = state.send_cancelled(Some("fix auth flow".into()));
        let resp = rx.try_recv().expect("should receive response");
        let raw = resp.expect("should be Ok");
        let parsed: serde_json::Value =
            serde_json::from_str(raw.0.get()).expect("should be valid JSON");
        assert_eq!(parsed["outcome"], "cancelled");
        assert_eq!(parsed["submissionId"], 1);
        assert_eq!(
            parsed["decisionId"].as_str(),
            Some(response.decision_id.as_str())
        );
        assert_eq!(parsed["feedback"], "fix auth flow");
    }

    #[test]
    fn test_send_cancelled_without_feedback() {
        let (mut state, mut rx) = make_test_state();
        let response = state.send_cancelled(None);
        let resp = rx.try_recv().expect("should receive response");
        let raw = resp.expect("should be Ok");
        let parsed: serde_json::Value =
            serde_json::from_str(raw.0.get()).expect("should be valid JSON");
        assert_eq!(parsed["outcome"], "cancelled");
        assert_eq!(parsed["submissionId"], 1);
        assert_eq!(
            parsed["decisionId"].as_str(),
            Some(response.decision_id.as_str())
        );
        assert!(parsed.get("feedback").is_none());
    }

    #[test]
    fn test_send_cancelled_empty_feedback_is_none() {
        let (mut state, mut rx) = make_test_state();
        let response = state.send_cancelled(Some("   ".into()));
        let resp = rx.try_recv().expect("should receive response");
        let raw = resp.expect("should be Ok");
        let parsed: serde_json::Value =
            serde_json::from_str(raw.0.get()).expect("should be valid JSON");
        assert_eq!(parsed["outcome"], "cancelled");
        assert_eq!(
            parsed["decisionId"].as_str(),
            Some(response.decision_id.as_str())
        );
        assert!(parsed.get("feedback").is_none());
    }

    #[test]
    fn stale_waiter_cleanup_closes_channel_without_a_decision() {
        let (mut state, mut rx) = make_test_state();
        state.close_stale_waiter();

        assert!(state.decision_response.is_none());
        assert!(state.response_tx.is_none());
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Closed)
        ));
    }

    #[test]
    fn repeated_decision_returns_the_same_response_and_sends_once() {
        let (mut state, mut rx) = make_test_state();
        let first = state.send_approved(None);
        let repeated = state.send_approved(None);
        let conflicting = state.send_cancelled(Some("different".into()));
        assert_eq!(first.decision_id, repeated.decision_id);
        assert_eq!(first.outcome, repeated.outcome);
        assert_eq!(first.feedback, repeated.feedback);
        assert_eq!(first.submission_id, repeated.submission_id);
        assert_eq!(first.decision_id, conflicting.decision_id);
        assert_eq!(first.outcome, conflicting.outcome);
        assert_eq!(first.feedback, conflicting.feedback);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn test_send_deferred_with_feedback() {
        let (mut state, mut rx) = make_test_state();
        let response = state.send_deferred(Some("review later".into()));
        let raw = rx
            .try_recv()
            .expect("should receive response")
            .expect("should be Ok");
        let parsed: serde_json::Value =
            serde_json::from_str(raw.0.get()).expect("should be valid JSON");
        assert_eq!(parsed["outcome"], "deferred");
        assert_eq!(
            parsed["decisionId"].as_str(),
            Some(response.decision_id.as_str())
        );
        assert_eq!(parsed["feedback"], "review later");
        assert_eq!(parsed["submissionId"], 1);
        assert!(uuid::Uuid::parse_str(parsed["decisionId"].as_str().unwrap()).is_ok());
    }

    #[test]
    fn test_constructor_defaults() {
        let (state, _rx) = make_test_state();
        assert_eq!(state.tool_call_id, "call_123");
        assert_eq!(state.plan_content, "# Plan\n\n## Step 1\nDo something");
        assert_eq!(state.submission_id, 1);
        assert_eq!(state.artifact_revision, 2);
        assert_eq!(state.artifact_hash, "a".repeat(64));
        assert_eq!(state.stashed_prompt.text, "stashed text");
        assert!(state.response_tx.is_some());
        assert_eq!(state.focus, PlanApprovalFocus::Preview);
        assert!(state.comments.is_empty());
        assert_eq!(state.next_comment_id, 0);
        assert!(state.editing_comment_id.is_none());
        assert!(state.commenting_range.is_none());
        assert!(state.stashed_feedback_prompt.is_none());
    }

    #[test]
    fn inline_plan_feedback_quotes_selected_line_snippets() {
        let (mut state, _rx) = make_test_state();
        state.plan_content = "alpha\nbravo\ncharlie\ndelta".into();
        state.comments.push(PlanComment {
            id: 0,
            line_range: 2..3,
            text: "rewrite this".into(),
        });
        state.comments.push(PlanComment {
            id: 1,
            line_range: 3..5,
            text: "combine these".into(),
        });

        let feedback = state.format_feedback(Some("overall note"));

        assert_eq!(
            feedback,
            "Proposed plan line 2:\n> bravo\n\nComment:\nrewrite this\n\nProposed plan lines 3-4:\n> charlie\n> delta\n\nComment:\ncombine these\n\nAdditional feedback:\noverall note"
        );
    }

    #[test]
    fn inline_plan_feedback_handles_out_of_range_lines() {
        let (mut state, _rx) = make_test_state();
        state.plan_content = "alpha".into();
        state.comments.push(PlanComment {
            id: 0,
            line_range: 9..10,
            text: "where is this".into(),
        });

        assert_eq!(
            state.format_feedback(None),
            "Proposed plan line 9:\n> [selected lines unavailable]\n\nComment:\nwhere is this"
        );
    }
}
