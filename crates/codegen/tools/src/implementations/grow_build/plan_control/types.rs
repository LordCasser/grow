//! Wire types for the `grow/plan_approval` ACP reverse request.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanApprovalExtRequest {
    pub session_id: String,
    pub tool_call_id: String,
    pub plan_content: String,
    pub submission_id: u64,
    pub artifact_revision: u64,
    pub artifact_hash: String,
}

/// Read-only lookup of the session's currently submitted immutable plan.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanArtifactQueryRequest {
    pub session_id: String,
}

/// Canonical artifact snapshot returned by `grow/plan/artifact`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanArtifactQueryResponse {
    pub session_id: String,
    pub submission_id: u64,
    pub retained: bool,
    pub artifact_revision: u64,
    pub artifact_hash: String,
    pub plan_content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_report: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanReviewRequest {
    pub session_id: String,
    pub submission_id: u64,
    pub decision_id: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanDecisionQuery {
    pub session_id: String,
    pub decision_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalOutcome {
    Approved,
    Cancelled,
    Abandoned,
    Deferred,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanApprovalExtResponse {
    pub outcome: PlanApprovalOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    pub submission_id: u64,
    pub decision_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_acp_camel_case() {
        let request = PlanApprovalExtRequest {
            session_id: "session".into(),
            tool_call_id: "call".into(),
            plan_content: "# Plan".into(),
            submission_id: 1,
            artifact_revision: 2,
            artifact_hash: "a".repeat(64),
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["sessionId"], "session");
        assert_eq!(value["toolCallId"], "call");
        assert_eq!(value["planContent"], "# Plan");
        assert_eq!(value["submissionId"], 1);
        assert_eq!(value["artifactRevision"], 2);
        assert_eq!(value["artifactHash"], "a".repeat(64));
    }

    #[test]
    fn response_rejects_unknown_outcome() {
        assert!(
            serde_json::from_value::<PlanApprovalExtResponse>(serde_json::json!({
                "outcome": "maybe",
                "submissionId": 1,
                "decisionId": "decision-1"
            }))
            .is_err()
        );
    }

    #[test]
    fn response_preserves_all_wire_outcomes() {
        for (wire, outcome) in [
            ("approved", PlanApprovalOutcome::Approved),
            ("cancelled", PlanApprovalOutcome::Cancelled),
            ("abandoned", PlanApprovalOutcome::Abandoned),
            ("deferred", PlanApprovalOutcome::Deferred),
        ] {
            let response: PlanApprovalExtResponse = serde_json::from_value(serde_json::json!({
                "outcome": wire,
                "submissionId": 1,
                "decisionId": "decision-1"
            }))
            .unwrap();
            assert_eq!(response.outcome, outcome);
            let value = serde_json::to_value(response).unwrap();
            assert_eq!(value["outcome"], serde_json::Value::String(wire.into()));
            assert_eq!(value["submissionId"], 1);
            assert_eq!(value["decisionId"], "decision-1");
        }
    }

    #[test]
    fn response_requires_decision_identity_and_rejects_unknown_fields() {
        let missing_identity = serde_json::json!({
            "outcome": "approved"
        });
        assert!(serde_json::from_value::<PlanApprovalExtResponse>(missing_identity).is_err());

        let unknown_field = serde_json::json!({
            "outcome": "approved",
            "submissionId": 1,
            "decisionId": "decision-1",
            "unexpected": true
        });
        assert!(serde_json::from_value::<PlanApprovalExtResponse>(unknown_field).is_err());
    }
}
