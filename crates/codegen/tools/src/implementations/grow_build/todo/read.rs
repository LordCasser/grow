//! Bounded observations of the session-owned or immediate-parent local plan.

use std::time::Duration;

use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use tool_types::planning::{
    PlanReadError, PlanReadInput, PlanReadOutput, PlanReadQuery, PlanReadTarget, project_plan_read,
};

use super::runtime::{PlanningCommand, PlanningRuntimeHandle};
use crate::implementations::grow_build::task::{
    backend::SubagentBackendResource, types::SessionIdResource,
};
use crate::types::tool::{ToolKind, ToolNamespace};
use crate::types::tool_metadata::{ToolMetadata, shared_resources};

pub const TODO_READ_TOOL_NAME: &str = "todo_read";
const READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct TodoReadTool;

impl ToolMetadata for TodoReadTool {
    fn kind(&self) -> ToolKind {
        ToolKind::PlanRead
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::Grow
    }

    fn description_template(&self) -> &str {
        "Read the complete local plan for this agent (target self, the default) or its immediate parent (target parent). Returns stable steps including removed items, delegation links, waits, replans and delivery reviews without changing either plan. Optionally filter by step_id. Records share one page limit, default 20 and maximum 50, and a 256 KiB output budget. Follow next_cursor to continue; if the snapshot changes, restart without a cursor. A cursor selects a page and grants no authority. plan:null means the available owner has no plan; no_parent and unavailable are distinct errors."
    }
}

impl tool_runtime::Tool for TodoReadTool {
    type Args = PlanReadInput;
    type Output = PlanReadOutput;

    fn id(&self) -> tool_protocol::ToolId {
        tool_protocol::ToolId::new(TODO_READ_TOOL_NAME).expect("valid tool id")
    }

    fn description(&self, _ctx: &tool_runtime::ListToolsContext) -> tool_types::ToolDescription {
        tool_types::ToolDescription::new(TODO_READ_TOOL_NAME, self.sanitized_description_template())
    }

    fn capabilities(&self) -> tool_protocol::ToolCapabilities {
        tool_protocol::ToolCapabilities {
            max_access: tool_protocol::ToolAccess::None,
            ..Default::default()
        }
    }

    async fn run(
        &self,
        ctx: tool_runtime::ToolCallContext,
        input: Self::Args,
    ) -> Result<Self::Output, tool_runtime::ToolError> {
        let query = PlanReadQuery {
            step_id: input.step_id,
            cursor: input.cursor,
            limit: input.limit,
        };
        query.validate().map_err(projection_error)?;

        // Only this observation is cancelled on completion, timeout or future
        // drop. The inherited session/tool token is never cancelled here.
        let cancellation = ctx
            .get::<tool_runtime::Cancellation>()
            .map_or_else(CancellationToken::new, |token| token.0.child_token());
        let _cancel_on_drop = cancellation.clone().drop_guard();
        let read = async {
            let resources = shared_resources(&ctx)
                .map_err(|_| unavailable("Planning resources are unavailable."))?;
            let resources = resources.lock().await;
            let reader = resources
                .get::<SessionIdResource>()
                .map(|resource| resource.0.clone())
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| unavailable("The bound reader identity is unavailable."))?;

            let snapshot = match input.target {
                PlanReadTarget::SelfPlan => {
                    let sender = resources
                        .get::<PlanningRuntimeHandle>()
                        .map(|handle| handle.0.clone())
                        .ok_or_else(|| unavailable("The local planning owner is unavailable."))?;
                    drop(resources);
                    let (respond_to, response) = oneshot::channel();
                    sender
                        .send(PlanningCommand::Read { respond_to })
                        .map_err(|_| unavailable("The local planning owner is closed."))?;
                    response
                        .await
                        .map_err(|_| unavailable("The local planning owner dropped the query."))?
                        .map_err(unavailable)?
                }
                PlanReadTarget::Parent => {
                    let backend = resources
                        .get::<SubagentBackendResource>()
                        .cloned()
                        .ok_or_else(|| unavailable("The parent planning route is unavailable."))?;
                    drop(resources);
                    backend
                        .backend()
                        .read_parent_plan(cancellation.clone())
                        .await
                        .map_err(|error| {
                            tool_runtime::ToolError::custom(error.code(), error.to_string())
                        })?
                }
            };

            let plan = match snapshot {
                Some(snapshot) => Some(
                    project_plan_read(&reader, input.target, &snapshot, &query)
                        .map_err(projection_error)?,
                ),
                None if query.cursor.is_some() => {
                    return Err(projection_error(PlanReadError::StaleCursor));
                }
                None => None,
            };
            Ok(PlanReadOutput {
                target: input.target,
                plan,
            })
        };

        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(unavailable("The planning observation was cancelled.")),
            result = tokio::time::timeout(READ_TIMEOUT, read) => {
                result.map_err(|_| {
                    tool_runtime::ToolError::custom("timed_out", "The planning observation exceeded five seconds.")
                })?
            }
        }
    }
}

fn unavailable(message: impl Into<String>) -> tool_runtime::ToolError {
    tool_runtime::ToolError::custom("unavailable", message)
}

fn projection_error(error: PlanReadError) -> tool_runtime::ToolError {
    tool_runtime::ToolError::custom(error.code(), error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::grow_build::task::{
        backend::ChannelBackend, parent_planning::ParentPlanningReadError, types::SubagentEvent,
    };
    use crate::types::{
        resources::{Resources, SharedResources},
        tool_metadata::test_ctx,
    };
    use tokio::sync::mpsc;
    use tool_types::planning::{
        DeliveryReview, DependencyKind, LocalPlan, PlanDelegation, PlanLink, PlanPriority,
        PlanReadCounts, PlanReadPage, PlanStep, PlanStepStatus, PlanWait, PlanningReadSnapshot,
        ReplanChange, ReplanProgress, ReviewDecision, WaitCondition, WaitDependency,
    };

    const READER: &str = "bound-reader";

    struct Owners {
        resources: SharedResources,
        local: mpsc::UnboundedReceiver<PlanningCommand>,
        parent: mpsc::UnboundedReceiver<SubagentEvent>,
    }

    impl Owners {
        fn new() -> Self {
            let (local_tx, local) = mpsc::unbounded_channel();
            let (parent_tx, parent) = mpsc::unbounded_channel();
            let mut resources = Resources::new();
            resources.insert(SessionIdResource(READER.into()));
            resources.insert(PlanningRuntimeHandle(local_tx));
            resources.insert(ChannelBackend::for_session(parent_tx, READER).into_resource());
            // These projections must never supply authoritative read facts.
            resources.insert(super::super::runtime::LocalPlanProjection(Some(
                LocalPlan::new("wrong-projection-owner", "wrong-scope"),
            )));
            resources.insert(super::super::TodoState::default());
            Self {
                resources: resources.into_shared(),
                local,
                parent,
            }
        }

        fn context(&self) -> tool_runtime::ToolCallContext {
            // No CurrentPromptIdResource: observation is not a plan mutation.
            test_ctx(self.resources.clone())
        }

        fn assert_quiet(&mut self) {
            assert!(matches!(
                self.local.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
            assert!(matches!(
                self.parent.try_recv(),
                Err(mpsc::error::TryRecvError::Empty)
            ));
        }

        async fn reply(
            &mut self,
            input: PlanReadInput,
            result: Result<Option<PlanningReadSnapshot>, ParentPlanningReadError>,
        ) -> Result<PlanReadOutput, tool_runtime::ToolError> {
            let target = input.target;
            let call = tool_runtime::Tool::run(&TodoReadTool, self.context(), input);
            tokio::pin!(call);
            match target {
                PlanReadTarget::SelfPlan => {
                    let command = tokio::select! {
                        command = self.local.recv() => command.expect("owner command"),
                        result = &mut call => panic!("read must await its owner: {result:?}"),
                    };
                    let PlanningCommand::Read { respond_to } = command else {
                        panic!("read must not write or link: {command:?}");
                    };
                    respond_to
                        .send(result.map_err(|error| error.to_string()))
                        .unwrap();
                }
                PlanReadTarget::Parent => {
                    let event = tokio::select! {
                        event = self.parent.recv() => event.expect("owner event"),
                        result = &mut call => panic!("read must await its parent: {result:?}"),
                    };
                    let SubagentEvent::ReadParentPlanning(request) = event else {
                        panic!("read must not send a message or mutate lifecycle");
                    };
                    assert_eq!(request.source_session_id, READER);
                    request.respond_to.send(result).unwrap();
                }
            }
            let result = call.await;
            self.assert_quiet();
            result
        }
    }

    fn input(target: PlanReadTarget) -> PlanReadInput {
        PlanReadInput {
            target,
            ..Default::default()
        }
    }

    fn snapshot(target: PlanReadTarget) -> PlanningReadSnapshot {
        let owner = match target {
            PlanReadTarget::SelfPlan => READER,
            PlanReadTarget::Parent => "immediate-parent",
        };
        let mut plan = LocalPlan::new(owner, "scope-文");
        plan.revision = 7;
        plan.steps = ["live", "removed", "other"]
            .map(|id| PlanStep {
                id: id.into(),
                content: format!("Complete requirements for {id}: 文\n\"quoted\""),
                priority: PlanPriority::High,
                status: PlanStepStatus::InProgress,
                requirements: Some(vec!["Preserve decision facts".into()]),
                removed: false,
            })
            .into();
        plan.steps[1].removed = true;
        plan.steps[1].status = PlanStepStatus::Cancelled;
        plan.links = vec![
            PlanLink {
                id: "task-live".into(),
                parent_step_id: "live".into(),
                child_session_id: if target == PlanReadTarget::Parent {
                    READER.into()
                } else {
                    "other-child".into()
                },
                terminal: false,
                required: true,
                pending_source_revision: Some(6),
                review_required_revision: 6,
                result_id: None,
                dispatch_failure: None,
            },
            PlanLink {
                id: "task-removed".into(),
                parent_step_id: "removed".into(),
                child_session_id: "delivered-child".into(),
                terminal: true,
                required: true,
                pending_source_revision: None,
                review_required_revision: 5,
                result_id: Some(
                    r#"{"timeline_id":"delivered-child","first_seq":42,"last_seq":42}"#.into(),
                ),
                dispatch_failure: None,
            },
        ];
        plan.waits = vec![
            PlanWait {
                id: "scope-wait".into(),
                step_ids: vec![],
                dependencies: vec![WaitDependency {
                    kind: DependencyKind::Permission,
                    id: "permission-1".into(),
                }],
                condition: WaitCondition::All,
            },
            PlanWait {
                id: "task-wait".into(),
                step_ids: vec!["other".into()],
                dependencies: vec![WaitDependency {
                    kind: DependencyKind::Task,
                    id: "task-removed".into(),
                }],
                condition: WaitCondition::Any,
            },
        ];
        plan.replans = plan
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| {
                let step = &plan.steps[index];
                ReplanChange {
                    id: format!("change-{index}"),
                    link_id: link.id.clone(),
                    child_session_id: link.child_session_id.clone(),
                    step_id: step.id.clone(),
                    source_revision: if index == 0 { 6 } else { 5 },
                    content: step.content.clone(),
                    requirements: step.requirements.clone(),
                    priority: step.priority,
                    status: step.status,
                    removed: step.removed,
                    progress: if index == 0 {
                        ReplanProgress::Received
                    } else {
                        ReplanProgress::Unapplied {
                            reason: "Child already terminal".into(),
                        }
                    },
                }
            })
            .collect();
        plan.reviews = vec![DeliveryReview {
            link_id: plan.links[1].id.clone(),
            result_id: plan.links[1].result_id.clone(),
            reviewed_source_revision: 5,
            decision: ReviewDecision::Accepted,
            reason: Some("Verified the exact result".into()),
            verification_refs: vec!["checks/文:42".into()],
        }];
        plan.validate().unwrap();
        PlanningReadSnapshot {
            timeline_seq: 123,
            plan,
            delegation: (target == PlanReadTarget::Parent).then(|| PlanDelegation {
                parent_session_id: owner.into(),
                parent_scope_id: "scope-文".into(),
                parent_step_id: "live".into(),
                task_id: "task-live".into(),
                child_session_id: READER.into(),
            }),
        }
    }

    fn assert_code(error: tool_runtime::ToolError, code: &str) {
        assert_eq!(error.details.unwrap()["code"], code);
    }

    fn record_count(page: &PlanReadPage) -> usize {
        page.steps.len()
            + page.links.len()
            + page.waits.len()
            + page.replans.len()
            + page.reviews.len()
    }

    #[test]
    fn tool_declares_independent_read_kind_and_no_execution_access() {
        let tool = TodoReadTool;
        assert_eq!(tool_runtime::Tool::id(&tool).as_str(), TODO_READ_TOOL_NAME);
        assert_eq!(tool.kind(), ToolKind::PlanRead);
        assert_eq!(tool.tool_namespace(), ToolNamespace::Grow);
        assert_eq!(
            tool_runtime::Tool::capabilities(&tool).max_access,
            tool_protocol::ToolAccess::None
        );
        assert!(tool.emitted_notifications().is_empty());
    }

    #[tokio::test]
    async fn both_owners_return_complete_mixed_pages_without_mutation() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let snapshot = snapshot(target);
            let before = serde_json::to_vec(&snapshot).unwrap();
            let mut combined = LocalPlan::new(&snapshot.plan.session_id, &snapshot.plan.scope_id);
            combined.revision = snapshot.plan.revision;
            let mut cursor = None;
            let mut pages = 0;
            loop {
                let limit = if pages % 2 == 0 { 2 } else { 3 };
                let output = owners
                    .reply(
                        PlanReadInput {
                            target,
                            cursor,
                            limit: Some(limit),
                            step_id: None,
                        },
                        Ok(Some(snapshot.clone())),
                    )
                    .await
                    .unwrap();
                assert!(serde_json::to_vec(&output).unwrap().len() <= 256 * 1024);
                assert_eq!(output.target, target);
                let page = output.plan.unwrap();
                assert!(record_count(&page) <= limit as usize);
                assert_eq!(page.session_id, snapshot.plan.session_id);
                assert_eq!(page.scope_id, snapshot.plan.scope_id);
                assert_eq!(page.revision, snapshot.plan.revision);
                assert_eq!(page.timeline_seq, snapshot.timeline_seq);
                assert_eq!(page.delegation, snapshot.delegation);
                assert_eq!(
                    page.counts,
                    PlanReadCounts {
                        steps: 3,
                        links: 2,
                        waits: 2,
                        replans: 2,
                        reviews: 1
                    }
                );
                assert_eq!(page.matched_counts, page.counts);
                combined.steps.extend(page.steps);
                combined.links.extend(page.links);
                combined.waits.extend(page.waits);
                combined.replans.extend(page.replans);
                combined.reviews.extend(page.reviews);
                cursor = page.next_cursor;
                pages += 1;
                assert!(pages <= 10, "pagination must advance");
                if cursor.is_none() {
                    break;
                }
            }
            assert_eq!(combined, snapshot.plan);
            assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
            let resources = owners.resources.lock().await;
            assert_eq!(
                resources
                    .get::<super::super::runtime::LocalPlanProjection>()
                    .unwrap()
                    .0
                    .as_ref()
                    .unwrap()
                    .session_id,
                "wrong-projection-owner"
            );
            assert!(
                resources
                    .get::<super::super::TodoState>()
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[tokio::test]
    async fn both_owners_filter_tombstones_and_related_decision_facts() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let source = snapshot(target);
            let output = owners
                .reply(
                    PlanReadInput {
                        target,
                        step_id: Some("removed".into()),
                        limit: Some(50),
                        ..Default::default()
                    },
                    Ok(Some(source.clone())),
                )
                .await
                .unwrap();
            let page = output.plan.unwrap();
            assert_eq!(page.steps, vec![source.plan.steps[1].clone()]);
            assert_eq!(page.links, vec![source.plan.links[1].clone()]);
            assert_eq!(page.waits, source.plan.waits);
            assert_eq!(page.replans, vec![source.plan.replans[1].clone()]);
            assert_eq!(page.reviews, source.plan.reviews);
            assert_eq!(
                page.matched_counts,
                PlanReadCounts {
                    steps: 1,
                    links: 1,
                    waits: 2,
                    replans: 1,
                    reviews: 1
                }
            );
            let empty = owners
                .reply(
                    PlanReadInput {
                        target,
                        step_id: Some("absent-step".into()),
                        ..Default::default()
                    },
                    Ok(Some(source)),
                )
                .await
                .unwrap()
                .plan
                .unwrap();
            assert_eq!(record_count(&empty), 0);
            assert_eq!(empty.matched_counts, PlanReadCounts::default());
        }
    }

    #[tokio::test]
    async fn shared_query_errors_are_rejected_before_either_owner_is_called() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            for (mut query, code) in [
                (
                    PlanReadInput {
                        limit: Some(0),
                        ..Default::default()
                    },
                    "invalid_query",
                ),
                (
                    PlanReadInput {
                        limit: Some(51),
                        ..Default::default()
                    },
                    "invalid_query",
                ),
                (
                    PlanReadInput {
                        step_id: Some(" ".into()),
                        ..Default::default()
                    },
                    "invalid_query",
                ),
                (
                    PlanReadInput {
                        cursor: Some("malformed".into()),
                        ..Default::default()
                    },
                    "invalid_cursor",
                ),
                (
                    PlanReadInput {
                        cursor: Some("x".repeat(4097)),
                        ..Default::default()
                    },
                    "invalid_cursor",
                ),
            ] {
                query.target = target;
                assert_code(
                    tool_runtime::Tool::run(&TodoReadTool, owners.context(), query)
                        .await
                        .unwrap_err(),
                    code,
                );
                owners.assert_quiet();
            }
        }
    }

    #[tokio::test]
    async fn absent_plan_is_null_but_an_existing_cursor_becomes_stale() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let output = owners.reply(input(target), Ok(None)).await.unwrap();
            assert_eq!(
                serde_json::to_value(output).unwrap(),
                serde_json::json!({"target": target, "plan": null})
            );
            let cursor = owners
                .reply(
                    PlanReadInput {
                        target,
                        limit: Some(1),
                        ..Default::default()
                    },
                    Ok(Some(snapshot(target))),
                )
                .await
                .unwrap()
                .plan
                .unwrap()
                .next_cursor;
            assert!(cursor.is_some());
            assert_code(
                owners
                    .reply(
                        PlanReadInput {
                            target,
                            cursor,
                            ..Default::default()
                        },
                        Ok(None),
                    )
                    .await
                    .unwrap_err(),
                "stale_cursor",
            );
        }
    }

    #[tokio::test]
    async fn every_page_reauthorizes_owner_before_validating_cursor_binding() {
        let mut owners = Owners::new();
        let parent = snapshot(PlanReadTarget::Parent);
        let cursor = owners
            .reply(
                PlanReadInput {
                    target: PlanReadTarget::Parent,
                    limit: Some(1),
                    ..Default::default()
                },
                Ok(Some(parent.clone())),
            )
            .await
            .unwrap()
            .plan
            .unwrap()
            .next_cursor;
        for error in [
            ParentPlanningReadError::NoParent,
            ParentPlanningReadError::Unavailable,
            ParentPlanningReadError::TimedOut,
            ParentPlanningReadError::InvalidEvidence,
        ] {
            assert_code(
                owners
                    .reply(
                        PlanReadInput {
                            target: PlanReadTarget::Parent,
                            cursor: cursor.clone(),
                            ..Default::default()
                        },
                        Err(error),
                    )
                    .await
                    .unwrap_err(),
                error.code(),
            );
        }
        owners
            .resources
            .lock()
            .await
            .insert(SessionIdResource("different-bound-reader".into()));
        assert_code(
            owners
                .reply(
                    PlanReadInput {
                        target: PlanReadTarget::Parent,
                        cursor,
                        ..Default::default()
                    },
                    Ok(Some(parent)),
                )
                .await
                .unwrap_err(),
            "cursor_mismatch",
        );
    }

    #[tokio::test]
    async fn both_owners_reject_changed_snapshot_target_and_filter() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let source = snapshot(target);
            let cursor = owners
                .reply(
                    PlanReadInput {
                        target,
                        limit: Some(1),
                        ..Default::default()
                    },
                    Ok(Some(source.clone())),
                )
                .await
                .unwrap()
                .plan
                .unwrap()
                .next_cursor;
            let mut changed = source.clone();
            changed.timeline_seq += 1;
            assert_code(
                owners
                    .reply(
                        PlanReadInput {
                            target,
                            cursor: cursor.clone(),
                            ..Default::default()
                        },
                        Ok(Some(changed)),
                    )
                    .await
                    .unwrap_err(),
                "stale_cursor",
            );
            assert_code(
                owners
                    .reply(
                        PlanReadInput {
                            target,
                            cursor: cursor.clone(),
                            step_id: Some("live".into()),
                            ..Default::default()
                        },
                        Ok(Some(source)),
                    )
                    .await
                    .unwrap_err(),
                "cursor_mismatch",
            );
            let other = if target == PlanReadTarget::SelfPlan {
                PlanReadTarget::Parent
            } else {
                PlanReadTarget::SelfPlan
            };
            assert_code(
                owners
                    .reply(
                        PlanReadInput {
                            target: other,
                            cursor,
                            ..Default::default()
                        },
                        Ok(Some(snapshot(other))),
                    )
                    .await
                    .unwrap_err(),
                "cursor_mismatch",
            );
        }
    }

    #[tokio::test]
    async fn byte_budget_preserves_complete_records_and_shared_error() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let mut source = snapshot(target);
            // Each field is valid, while escaping makes a single record too large.
            source.plan.steps[0].content = "\"".repeat(64 * 1024);
            source.plan.steps[0].requirements = Some(vec!["\"".repeat(64 * 1024); 2]);
            source.plan.validate().unwrap();
            assert_code(
                owners
                    .reply(input(target), Ok(Some(source)))
                    .await
                    .unwrap_err(),
                "item_too_large",
            );
        }
    }

    #[tokio::test]
    async fn missing_resources_reader_or_route_are_unavailable() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            assert_code(
                tool_runtime::Tool::run(
                    &TodoReadTool,
                    tool_runtime::ToolCallContext::default(),
                    input(target),
                )
                .await
                .unwrap_err(),
                "unavailable",
            );
            for reader in [None, Some(""), Some(" ")] {
                let (sender, mut receiver) = mpsc::unbounded_channel();
                let mut resources = Resources::new();
                resources.insert(PlanningRuntimeHandle(sender));
                if let Some(reader) = reader {
                    resources.insert(SessionIdResource(reader.into()));
                }
                assert_code(
                    tool_runtime::Tool::run(
                        &TodoReadTool,
                        test_ctx(resources.into_shared()),
                        input(target),
                    )
                    .await
                    .unwrap_err(),
                    "unavailable",
                );
                assert!(receiver.try_recv().is_err());
            }
            let mut resources = Resources::new();
            resources.insert(SessionIdResource(READER.into()));
            assert_code(
                tool_runtime::Tool::run(
                    &TodoReadTool,
                    test_ctx(resources.into_shared()),
                    input(target),
                )
                .await
                .unwrap_err(),
                "unavailable",
            );
        }
    }

    #[tokio::test]
    async fn closed_routes_and_dropped_replies_never_look_like_absent_plans() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            match target {
                PlanReadTarget::SelfPlan => owners.local.close(),
                PlanReadTarget::Parent => owners.parent.close(),
            }
            assert_code(
                tool_runtime::Tool::run(&TodoReadTool, owners.context(), input(target))
                    .await
                    .unwrap_err(),
                "unavailable",
            );
            let mut owners = Owners::new();
            let call = tool_runtime::Tool::run(&TodoReadTool, owners.context(), input(target));
            tokio::pin!(call);
            match target {
                PlanReadTarget::SelfPlan => {
                    let command = tokio::select! { command = owners.local.recv() => command.unwrap(), result = &mut call => panic!("owner must reply: {result:?}") };
                    let PlanningCommand::Read { respond_to } = command else {
                        panic!("expected read");
                    };
                    drop(respond_to);
                }
                PlanReadTarget::Parent => {
                    let event = tokio::select! { event = owners.parent.recv() => event.unwrap(), result = &mut call => panic!("owner must reply: {result:?}") };
                    let SubagentEvent::ReadParentPlanning(request) = event else {
                        panic!("expected parent read");
                    };
                    drop(request.respond_to);
                }
            }
            assert_code(call.await.unwrap_err(), "unavailable");
        }
        let mut owners = Owners::new();
        assert_code(
            owners
                .reply(
                    input(PlanReadTarget::SelfPlan),
                    Err(ParentPlanningReadError::Unavailable),
                )
                .await
                .unwrap_err(),
            "unavailable",
        );
    }

    #[tokio::test(start_paused = true)]
    async fn both_observations_timeout_at_five_seconds_without_cancelling_session() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let session = CancellationToken::new();
            let mut ctx = owners.context();
            ctx.insert(tool_runtime::Cancellation(session.clone()));
            let call = tool_runtime::Tool::run(&TodoReadTool, ctx, input(target));
            tokio::pin!(call);
            let (local_reply, parent_request) = match target {
                PlanReadTarget::SelfPlan => {
                    let command = tokio::select! { command = owners.local.recv() => command.unwrap(), result = &mut call => panic!("read must wait: {result:?}") };
                    let PlanningCommand::Read { respond_to } = command else {
                        panic!("expected read");
                    };
                    (Some(respond_to), None)
                }
                PlanReadTarget::Parent => {
                    let event = tokio::select! { event = owners.parent.recv() => event.unwrap(), result = &mut call => panic!("read must wait: {result:?}") };
                    let SubagentEvent::ReadParentPlanning(request) = event else {
                        panic!("expected parent read");
                    };
                    (None, Some(request))
                }
            };
            tokio::time::advance(Duration::from_secs(4)).await;
            assert!(futures::poll!(&mut call).is_pending());
            tokio::time::advance(Duration::from_secs(1)).await;
            assert_code(call.await.unwrap_err(), "timed_out");
            assert!(!session.is_cancelled());
            if let Some(reply) = local_reply {
                assert!(reply.is_closed());
            }
            if let Some(request) = parent_request {
                assert!(request.respond_to.is_closed());
                assert!(request.cancellation.is_cancelled());
            }
            owners.assert_quiet();
        }
    }

    #[tokio::test(start_paused = true)]
    async fn resource_lock_wait_is_included_in_observation_deadline() {
        let owners = Owners::new();
        let _lock = owners.resources.lock().await;
        let call = tool_runtime::Tool::run(
            &TodoReadTool,
            owners.context(),
            input(PlanReadTarget::SelfPlan),
        );
        tokio::pin!(call);
        assert!(futures::poll!(&mut call).is_pending());
        tokio::time::advance(READ_TIMEOUT).await;
        assert_code(call.await.unwrap_err(), "timed_out");
    }

    #[tokio::test]
    async fn cancellation_closes_only_current_observation() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let session = CancellationToken::new();
            let observation = session.child_token();
            let mut ctx = owners.context();
            ctx.insert(tool_runtime::Cancellation(observation.clone()));
            let call = tool_runtime::Tool::run(&TodoReadTool, ctx, input(target));
            tokio::pin!(call);
            let (local_reply, parent_request) = match target {
                PlanReadTarget::SelfPlan => {
                    let command = tokio::select! { command = owners.local.recv() => command.unwrap(), result = &mut call => panic!("read must wait: {result:?}") };
                    let PlanningCommand::Read { respond_to } = command else {
                        panic!("expected read");
                    };
                    (Some(respond_to), None)
                }
                PlanReadTarget::Parent => {
                    let event = tokio::select! { event = owners.parent.recv() => event.unwrap(), result = &mut call => panic!("read must wait: {result:?}") };
                    let SubagentEvent::ReadParentPlanning(request) = event else {
                        panic!("expected parent read");
                    };
                    (None, Some(request))
                }
            };
            observation.cancel();
            assert_code(call.await.unwrap_err(), "unavailable");
            assert!(!session.is_cancelled());
            if let Some(reply) = local_reply {
                assert!(reply.is_closed());
            }
            if let Some(request) = parent_request {
                assert!(request.respond_to.is_closed());
                assert!(request.cancellation.is_cancelled());
            }
            owners.assert_quiet();
            assert!(
                owners
                    .reply(input(target), Ok(Some(snapshot(target))))
                    .await
                    .is_ok()
            );
        }
    }

    #[tokio::test]
    async fn dropping_observation_releases_reply_and_cancels_only_query() {
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let mut owners = Owners::new();
            let session = CancellationToken::new();
            let mut ctx = owners.context();
            ctx.insert(tool_runtime::Cancellation(session.clone()));
            let mut call = Box::pin(tool_runtime::Tool::run(&TodoReadTool, ctx, input(target)));
            match target {
                PlanReadTarget::SelfPlan => {
                    let command = tokio::select! {
                        command = owners.local.recv() => command.unwrap(),
                        result = &mut call => panic!("read must wait: {result:?}"),
                    };
                    let PlanningCommand::Read { respond_to } = command else {
                        panic!("expected read");
                    };
                    drop(call);
                    assert!(respond_to.is_closed());
                }
                PlanReadTarget::Parent => {
                    let event = tokio::select! {
                        event = owners.parent.recv() => event.unwrap(),
                        result = &mut call => panic!("read must wait: {result:?}"),
                    };
                    let SubagentEvent::ReadParentPlanning(request) = event else {
                        panic!("expected parent read");
                    };
                    drop(call);
                    assert!(request.respond_to.is_closed());
                    assert!(request.cancellation.is_cancelled());
                }
            }
            assert!(!session.is_cancelled());
            owners.assert_quiet();
        }
    }

    #[tokio::test]
    async fn already_cancelled_call_never_reaches_either_owner() {
        let mut owners = Owners::new();
        for target in [PlanReadTarget::SelfPlan, PlanReadTarget::Parent] {
            let cancellation = CancellationToken::new();
            cancellation.cancel();
            let mut ctx = owners.context();
            ctx.insert(tool_runtime::Cancellation(cancellation));
            assert_code(
                tool_runtime::Tool::run(&TodoReadTool, ctx, input(target))
                    .await
                    .unwrap_err(),
                "unavailable",
            );
            owners.assert_quiet();
        }
    }
}
