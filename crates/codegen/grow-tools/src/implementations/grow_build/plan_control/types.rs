//! Wire types for the `grow/plan_approval` ACP reverse request.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanApprovalExtRequest {
    pub session_id: String,
    pub tool_call_id: String,
    pub plan_content: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlanApprovalExtResponse {
    /// `approved`, `cancelled`, or `abandoned`.
    pub outcome: String,
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
}
