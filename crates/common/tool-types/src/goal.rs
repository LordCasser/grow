use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Canonical status encoded by both the checkbox and status token in a Goal task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalTaskStatus {
    Pending,
    InProgress,
    Blocked,
    Done,
}

impl GoalTaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }
}

/// Read-only task projection derived from the durable Markdown blackboard.
/// It is never persisted independently and therefore cannot diverge from the
/// document that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalTaskProjection {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: u8,
    pub status: GoalTaskStatus,
    pub summary: String,
    pub completed_descendants: u32,
    pub total_descendants: u32,
}

/// Fields the primary Agent may change without changing task structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalProgressUpdate {
    pub task_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<GoalTaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
}

/// One planner task in a structured Goal plan. The spec is pure data: task
/// ids, indentation, and every piece of Markdown syntax are derived by the
/// host during assembly, so a planner can never smuggle document structure
/// through a plan submission. `status` legality is enforced by the
/// [`GoalTaskStatus`] type itself; invalid status tokens are rejected at
/// deserialization time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalPlanTaskSpec {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<GoalTaskStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<GoalPlanTaskSpec>,
}

/// Tagged section of a structured Goal plan. A [`GoalPlanSpec`] carries
/// exactly one `plan_tasks` and one `goal_acceptance` section plus at most
/// one optional `open_gaps` section; anything else fails assembly validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GoalPlanSectionPayload {
    PlanTasks { tasks: Vec<GoalPlanTaskSpec> },
    GoalAcceptance { items: Vec<String> },
    OpenGaps { items: Vec<String> },
}

/// Structured planner submission for a Goal blackboard. The canonical
/// Markdown document and all task ids are derived from this spec by the
/// host-side assembler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalPlanSpec {
    #[serde(default)]
    pub sections: Vec<GoalPlanSectionPayload>,
}

/// One structured Goal plan assembly failure, addressed at the offending
/// spec entry via a dotted path such as `tasks[2].children[0].summary`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalPlanAssemblyIssue {
    pub path: String,
    pub reason: String,
}

/// Every rule violation found in one structured plan submission, aggregated
/// so a planner can fix all of them in a single retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalPlanAssemblyError {
    pub items: Vec<GoalPlanAssemblyIssue>,
}

impl std::fmt::Display for GoalPlanAssemblyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Goal plan assembly failed")?;
        for issue in &self.items {
            write!(f, "\n{}: {}", issue.path, issue.reason)?;
        }
        Ok(())
    }
}

impl std::error::Error for GoalPlanAssemblyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_spec_deserializes_snake_case_sections_with_data_only_defaults() {
        let spec: GoalPlanSpec = serde_json::from_str(
            r#"{"sections": [
                {"plan_tasks": {"tasks": [
                    {"summary": "Ship", "children": [{"summary": "Child"}]}
                ]}},
                {"goal_acceptance": {"items": ["tests pass"]}}
            ]}"#,
        )
        .unwrap();
        let GoalPlanSectionPayload::PlanTasks { tasks } = &spec.sections[0] else {
            panic!("first section must be plan_tasks");
        };
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].status, None);
        assert_eq!(
            tasks[0].status.unwrap_or(GoalTaskStatus::Pending),
            GoalTaskStatus::Pending
        );
        assert!(tasks[0].scope.is_none());
        assert_eq!(tasks[0].children[0].summary, "Child");
        assert!(tasks[0].children[0].children.is_empty());
    }

    #[test]
    fn plan_spec_rejects_document_syntax_and_unknown_fields() {
        for payload in [
            r#"{"summary": "s", "id": "T9"}"#,
            r#"{"summary": "s", "indent": 4}"#,
            r#"{"summary": "s", "markdown": "- [ ] **T1**"}"#,
        ] {
            assert!(
                serde_json::from_str::<GoalPlanTaskSpec>(payload).is_err(),
                "task spec must reject document syntax fields: {payload}"
            );
        }
        assert!(
            serde_json::from_str::<GoalPlanSpec>(
                r##"{"sections": [], "board_markdown": "# Goal"}"##
            )
            .is_err()
        );
    }

    #[test]
    fn invalid_status_token_is_rejected_at_deserialization() {
        let bad =
            serde_json::from_str::<GoalPlanTaskSpec>(r#"{"summary": "s", "status": "finnished"}"#);
        assert!(bad.is_err());
        let good = serde_json::from_str::<GoalPlanTaskSpec>(
            r#"{"summary": "s", "status": "in_progress"}"#,
        )
        .unwrap();
        assert_eq!(good.status, Some(GoalTaskStatus::InProgress));
    }

    #[test]
    fn assembly_error_serializes_as_structured_items() {
        let error = GoalPlanAssemblyError {
            items: vec![GoalPlanAssemblyIssue {
                path: "tasks[0].summary".into(),
                reason: "task summary must not be empty".into(),
            }],
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "items": [{"path": "tasks[0].summary", "reason": "task summary must not be empty"}]
            })
        );
    }
}
