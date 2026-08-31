//! Wire types for the `grow/plan_approval` ACP reverse request.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanApprovalExtRequest {
    pub session_id: String,
    pub tool_call_id: String,
    pub plan_content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanApprovalOutcome {
    Approved,
    Cancelled,
    Abandoned,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanApprovalExtResponse {
    pub outcome: PlanApprovalOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
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
        };
        let value = serde_json::to_value(request).unwrap();
        assert_eq!(value["sessionId"], "session");
        assert_eq!(value["toolCallId"], "call");
        assert_eq!(value["planContent"], "# Plan");
    }

    #[test]
    fn response_rejects_unknown_outcome() {
        assert!(
            serde_json::from_value::<PlanApprovalExtResponse>(serde_json::json!({
                "outcome": "maybe"
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
        ] {
            let response: PlanApprovalExtResponse = serde_json::from_value(serde_json::json!({
                "outcome": wire
            }))
            .unwrap();
            assert_eq!(response.outcome, outcome);
            assert_eq!(
                serde_json::to_value(response).unwrap()["outcome"],
                serde_json::Value::String(wire.into())
            );
        }
    }
}
