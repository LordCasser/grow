//! Joint execution/context journeys through actual tools and providers.
use super::*;
use tools::implementations::grow_build::task::backend::{SubagentBackend, SubagentBackendResource};
use tools::implementations::grow_build::task::types::{
    MaxSubagentDepth, SessionIdResource, SubagentCancelOutcome, SubagentDepthCounter,
    SubagentObservationError, SubagentRequest, SubagentResult, SubagentSnapshot,
    SubagentValidateTypeOutcome,
};

#[path = "message_delivery_goal_tests.rs"]
mod goal;

struct WaitingChild {
    started: mpsc::UnboundedSender<String>,
    release: tokio::sync::Notify,
    cancellations: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl SubagentBackend for WaitingChild {
    async fn spawn(
        &self,
        request: SubagentRequest,
    ) -> Result<SubagentResult, tool_runtime::ToolError> {
        self.started.send(request.id.clone()).unwrap();
        self.release.notified().await;
        Ok(SubagentResult {
            success: true,
            output: Arc::from("The original child completed its investigation."),
            subagent_id: request.id.clone(),
            child_session_id: request.id,
            ..Default::default()
        })
    }

    async fn query(
        &self,
        _id: &str,
        _block: bool,
        _timeout_ms: Option<u64>,
    ) -> Result<Option<SubagentSnapshot>, SubagentObservationError> {
        Ok(None)
    }

    async fn cancel(&self, _id: &str) -> Result<SubagentCancelOutcome, SubagentObservationError> {
        self.cancellations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(SubagentCancelOutcome::NotFound)
    }

    async fn validate_type(
        &self,
        _subagent_type: &str,
        _parent_session_id: &str,
    ) -> SubagentValidateTypeOutcome {
        SubagentValidateTypeOutcome::Ok
    }
}

#[test]
fn actual_child_tool_wait_survives_consultation_until_original_result_settles() {
    run_with_session_stack(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(tokio::task::LocalSet::new().run_until(async {
            use tools::implementations::grow_build::{ListSubagentTool, GetSubagentStatusTool, WaitSubagentTool, KillTaskTool};
            use tools::implementations::grow_build::task::TaskTool;
            use tools::registry::types::ToolConfig;
            let server = MockInferenceServer::start().await.unwrap();
            let first = server.expect_response("start real waiting tool",
                test_support::InferenceRequestMatcher::foreground(test_support::InferenceEndpoint::Messages),
                messages_turn(&[tool_use_block("waiting-child-tool", "spawn_subagent",
                    r#"{"parent_step_id":"waiting-child-step","prompt":"inspect the target","description":"inspect target","subagent_type":"explore","background":false,"capability_mode":"read-only"}"#)], "tool_use"));
            let successor = server.expect_response("after original child result",
                test_support::InferenceRequestMatcher::foreground(test_support::InferenceEndpoint::Messages),
                messages_turn(&[text_block("Both the child result and consultation are visible.")], END_TURN));
            let (actor, _) = actor_with_sampler(&server, sampling_types::ApiBackend::Messages).await;
            *actor.agent.borrow_mut() = test_agent_with_tools(vec![
                ToolConfig::for_tool::<TaskTool>().with_name("spawn_subagent").with_param_rename("run_in_background", "background"),
                ToolConfig::for_tool::<ListSubagentTool>(), ToolConfig::for_tool::<GetSubagentStatusTool>(), ToolConfig::for_tool::<WaitSubagentTool>(), ToolConfig::for_tool::<KillTaskTool>(),
            ]).await;
            let (started, mut starts) = mpsc::unbounded_channel();
            let backend = Arc::new(WaitingChild { started, release: tokio::sync::Notify::new(), cancellations: Default::default() });
            let bridge = actor.agent.borrow().tool_bridge().clone();
            bridge.update_resource(SubagentBackendResource(backend.clone())).await;
            bridge.update_resource(SubagentDepthCounter(0)).await;
            bridge.update_resource(MaxSubagentDepth(1)).await;
            bridge.update_resource(SessionIdResource(actor.session_id_string())).await;
            let mut plan = tool_types::LocalPlan::new(actor.session_id_string(), "work:waiting-child");
            plan.steps.push(tool_types::PlanStep {
                id: "waiting-child-step".into(), content: "Inspect the target".into(),
                priority: tool_types::PlanPriority::Normal, status: tool_types::PlanStepStatus::InProgress,
                requirements: None, removed: false,
            });
            actor.chat_state_handle.record_timeline_event_durably(chat_state::TimelineEventKind::Planning(
                chat_state::timeline::PlanningEvent { previous: None, plan: Box::new(plan) },
            )).await.unwrap();
            let (planning_tx, mut planning_rx) = mpsc::unbounded_channel();
            bridge.update_resource(tools::implementations::grow_build::todo::runtime::PlanningRuntimeHandle(planning_tx)).await;
            bridge.update_resource(tools::implementations::grow_build::task::types::CurrentPromptIdResource("waiting-child-consultation".into())).await;
            let planning_actor = Arc::downgrade(&actor);
            tokio::task::spawn_local(async move {
                while let Some(command) = planning_rx.recv().await {
                    let Some(actor) = planning_actor.upgrade() else { break };
                    actor.handle_planning_command(command).await;
                }
            });
            let (result, ()) = tokio::join!(run_user_turn(&actor, "waiting-child-consultation"), async {
                let child = tokio::time::timeout(std::time::Duration::from_secs(5), starts.recv()).await.unwrap().unwrap();
                assert!(!child.is_empty());
                assert!(first.is_satisfied());
                let outcome = answer_live_consultation(&actor, &server, "ask-while-child-waits", "Should the child keep its original assignment?", "Yes, keep the original child assignment.").await;
                assert_eq!(outcome.status, crate::coordination::InquiryStatus::Answered);
                assert_eq!(backend.cancellations.load(std::sync::atomic::Ordering::SeqCst), 0);
                assert!(!*actor.parent_message_interrupt.borrow());
                assert!(actor.pending_interjections.is_empty());
                assert!(!successor.is_satisfied(), "consultation cannot finish the original child wait");
                let events = actor.chat_state_handle.timeline_events().await.unwrap();
                assert!(!events.iter().any(|event| matches!(&event.kind,
                    chat_state::TimelineEventKind::Messages(message) if message.items.iter().any(|item| matches!(item,
                        ConversationItem::ToolResult(result) if result.tool_call_id == "waiting-child-tool")))));
                assert_eq!(actor.chat_state_handle.pending_notifications().await.unwrap().len(), 1);
                backend.release.notify_one();
            });
            result.unwrap();
            assert!(successor.is_satisfied());
            assert_eq!(backend.cancellations.load(std::sync::atomic::Ordering::SeqCst), 0);
            let wire = server.request_bodies().last().unwrap().to_string();
            assert!(wire.contains("The original child completed its investigation."));
            assert!(wire.contains("Should the child keep its original assignment?"));
            assert!(wire.contains("Yes, keep the original child assignment."));
            assert!(actor.chat_state_handle.pending_notifications().await.unwrap().is_empty());
            let timeline = actor.chat_state_handle.timeline_events().await.unwrap();
            let tool_result = timeline.iter().position(|event| matches!(&event.kind,
                chat_state::TimelineEventKind::Messages(message) if message.items.iter().any(|item| matches!(item,
                    ConversationItem::ToolResult(result) if result.tool_call_id == "waiting-child-tool")))).unwrap();
            let consumed = timeline.iter().position(|event| matches!(event.kind,
                chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Consumed { .. }))).unwrap();
            assert!(tool_result < consumed, "original tool result must settle before context consumption");
        }));
    });
}
