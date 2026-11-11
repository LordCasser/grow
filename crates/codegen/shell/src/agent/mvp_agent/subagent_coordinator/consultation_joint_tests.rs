//! Real ask tools, authority routing, source results and target context.
use super::super::tests::{make_test_handle_with_receiver, run_local_for_bridge_test};
use crate::session::actor::tests::support::{serve_coordination_commands, test_agent_with_tools};
use crate::session::actor::tests::truncation_recovery_tests::{
    actor_with_sampler_named, messages_turn, run_user_turn, text_block, tool_use_block,
};
use crate::session::{SessionHandle, actor::SessionActor};
use std::{collections::HashMap, rc::Rc, sync::Arc};
use tools::implementations::grow_build::task::backend::{ChannelBackend, SubagentBackend};
use tools::implementations::grow_build::task::coordinator::*;
use tools::implementations::grow_build::task::interaction::*;
use tools::implementations::grow_build::task::types::*;

#[path = "peer_consultation_joint_tests.rs"]
mod peer;

struct TestControl(tokio_util::sync::CancellationToken);
impl ChildControl for TestControl {
    type SecurityContext = ();
    fn progress_snapshot(&self) -> Option<SubagentProgress> {
        None
    }
    fn security_context(&self) {}
    fn cancel(&self) {
        self.0.cancel();
    }
}

struct RoutedChild {
    handles: Rc<HashMap<String, SessionHandle>>,
    child_session_id: String,
    started: tokio::sync::mpsc::UnboundedSender<()>,
    release: Arc<tokio::sync::Notify>,
    cancelled: tokio_util::sync::CancellationToken,
}

impl ChildRunner for RoutedChild {
    type Control = TestControl;
    type CompletionData = ();
    type RunFuture = LocalBoxFuture<ChildRunOutcome<()>>;
    type ValidateFuture = LocalBoxFuture<SubagentValidateTypeOutcome>;

    fn interact(&self, request: AgentInteractionRequest, target_id: String, task: String) {
        let source = self
            .handles
            .get(&request.source_session_id)
            .unwrap()
            .clone();
        let target = self.handles.get(&target_id).cloned();
        tokio::task::spawn_local(super::interaction::run(
            request, source, target, target_id, task,
        ));
    }

    fn run(&self, run: ChildRunRequest<TestControl>) -> Self::RunFuture {
        let child_session_id = self.child_session_id.clone();
        let started = self.started.clone();
        let release = self.release.clone();
        let cancelled = self.cancelled.clone();
        Box::pin(async move {
            assert!(
                run.reporter
                    .started(StartedChild {
                        child_session_id: child_session_id.clone(),
                        resumed_from: None,
                        definition_background: false,
                        control: TestControl(cancelled.clone()),
                    })
                    .await
            );
            started.send(()).unwrap();
            tokio::select! { _ = release.notified() => {}, _ = cancelled.cancelled() => {} }
            ChildRunOutcome::Finished(ChildRunOutput {
                result: SubagentResult {
                    success: !cancelled.is_cancelled(),
                    subagent_id: run.request.id,
                    child_session_id,
                    output: Arc::from("original child finished"),
                    ..Default::default()
                },
                completion_data: (),
            })
        })
    }
    fn validate_type(&self, _: String, _: String) -> Self::ValidateFuture {
        Box::pin(async { SubagentValidateTypeOutcome::Ok })
    }
    fn on_completed(&self, _: ChildCompletion<()>) {}
}

fn route_handle(actor: &Arc<SessionActor>) -> SessionHandle {
    let (mut handle, commands) = make_test_handle_with_receiver("test-model", None);
    handle.info = actor.session_info.clone();
    handle.chat_state_handle = actor.chat_state_handle.clone();
    handle.current_prompt_id = actor.current_prompt_id.clone();
    handle.behavior = actor.behavior.clone();
    serve_coordination_commands(actor.clone(), commands);
    handle
}

async fn ask_agent(actor: &Arc<SessionActor>, backend: ChannelBackend) {
    use tools::implementations::grow_build::read_file::ReadFileTool;
    use tools::registry::types::ToolConfig;
    *actor.agent.borrow_mut() = test_agent_with_tools(vec![
        ToolConfig::for_tool::<AskParentTool>(),
        ToolConfig::for_tool::<AskSubagentTool>(),
        ToolConfig::for_tool::<ReadFileTool>(),
    ])
    .await;
    actor
        .agent
        .borrow()
        .tool_bridge()
        .update_resource(backend.into_resource())
        .await;
}

fn assert_actor_inquiry(
    actor: &SessionActor,
    events: &[chat_state::TimelineEvent],
    source: &str,
    direction: crate::coordination::InquiryDirection,
    own_tool_call: &str,
    own_answer: &str,
    incoming_question: &str,
    incoming_answer: &str,
) {
    let mut incoming = 0;
    let mut outgoing = 0;
    for event in events {
        match crate::coordination::InquiryEvent::from_timeline(event).unwrap() {
            Some(crate::coordination::InquiryEvent::Incoming { audit, .. })
                if audit.answer_context.is_some() =>
            {
                incoming += 1;
                assert_eq!(audit.direction, direction);
                assert_eq!(audit.source_session_id, source);
                assert_eq!(
                    audit.answer_context.unwrap().target_session_id,
                    actor.session_info.id.to_string()
                );
                assert_eq!(audit.question, incoming_question);
                assert_eq!(
                    audit.outcome.unwrap().answer.as_deref(),
                    Some(incoming_answer)
                );
            }
            Some(crate::coordination::InquiryEvent::OutgoingCompleted { audit }) => {
                outgoing += 1;
                assert_eq!(audit.outcome.answer.as_deref(), Some(own_answer));
            }
            _ => {}
        }
    }
    assert_eq!((incoming, outgoing), (1, 1));
    assert!(events.iter().any(|event| matches!(&event.kind,
        chat_state::TimelineEventKind::Messages(message) if message.items.iter().any(|item| matches!(item,
            sampling_types::ConversationItem::ToolResult(result) if result.tool_call_id == own_tool_call && item.text_content().contains(own_answer))))));
    assert_eq!(events.iter().filter(|event| matches!(&event.kind,
        chat_state::TimelineEventKind::Messages(message) if message.items.iter().any(|item| matches!(item,
            sampling_types::ConversationItem::ToolResult(result) if result.tool_call_id == own_tool_call)))).count(), 1);
    assert!(
        events
            .iter()
            .filter(|event| matches!(
                &event.kind,
                chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started { .. })
            ))
            .count()
            <= 2
    );
}

#[test]
fn opposite_parent_child_tools_use_real_authority_routes_and_independent_sidebands() {
    run_local_for_bridge_test(|| async {
        use crate::coordination::InquiryDirection;
        use test_support::{InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer};
        let parent_server = MockInferenceServer::start().await.unwrap();
        let child_server = MockInferenceServer::start().await.unwrap();
        let task_id = uuid::Uuid::now_v7().to_string();
        parent_server.enqueue_response("/v1/messages", messages_turn(&[
            tool_use_block("parent-asks", "ask_subagent", &serde_json::json!({"subagent_id":task_id,"question":"Parent asks child about investigation."}).to_string())
        ], "tool_use"));
        child_server.enqueue_response(
            "/v1/messages",
            messages_turn(
                &[tool_use_block(
                    "child-asks",
                    "ask_parent",
                    r#"{"question":"Child asks parent about scope."}"#,
                )],
                "tool_use",
            ),
        );
        let mut parent_answer = parent_server.expect_response_blocked(
            "parent answers child",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_turn(
                &[text_block("Parent consultation answer: preserve scope.")],
                "end_turn",
            ),
        );
        let mut child_answer = child_server.expect_response_blocked(
            "child answers parent",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_turn(
                &[text_block(
                    "Child consultation answer: investigation is ready.",
                )],
                "end_turn",
            ),
        );
        parent_server.enqueue_response(
            "/v1/messages",
            messages_turn(&[text_block("Parent continued.")], "end_turn"),
        );
        child_server.enqueue_response(
            "/v1/messages",
            messages_turn(&[text_block("Child continued.")], "end_turn"),
        );
        let parent = actor_with_sampler_named(&parent_server, "route-parent").await;
        let child = actor_with_sampler_named(&child_server, "route-child").await;
        let handles: Rc<HashMap<String, SessionHandle>> = Rc::new(
            [
                ("route-parent".into(), route_handle(&parent)),
                ("route-child".into(), route_handle(&child)),
            ]
            .into_iter()
            .collect(),
        );
        let (commands, receiver) = tokio::sync::mpsc::unbounded_channel();
        let (started, mut starts) = tokio::sync::mpsc::unbounded_channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let cancelled = tokio_util::sync::CancellationToken::new();
        let coordinator = SubagentCoordinator::new(
            receiver,
            RoutedChild {
                handles: handles.clone(),
                child_session_id: "route-child".into(),
                started,
                release: release.clone(),
                cancelled: cancelled.clone(),
            },
            CoordinatorConfig::default(),
        );
        tokio::task::spawn_local(coordinator.run());
        let parent_backend = ChannelBackend::for_session(commands.clone(), "route-parent");
        let child_backend = ChannelBackend::for_session(commands, "route-child");
        let seeding_backend = parent_backend.clone();
        let seeded = tokio::task::spawn_local(async move {
            seeding_backend
                .spawn(SubagentRequest {
                    id: task_id,
                    origin_tool_call_id: None,
                    prompt: "original work".into(),
                    description: "investigation".into(),
                    subagent_type: "explore".into(),
                    parent_session_id: "route-parent".into(),
                    parent_step_id: None,
                    parent_prompt_id: None,
                    resume_from: None,
                    cwd: None,
                    runtime_overrides: Default::default(),
                    run_in_background: true,
                    surface_completion: false,
                    await_to_completion: false,
                    fork_context: false,
                    owner: Default::default(),
                    goal_context: None,
                    cancel_token: Default::default(),
                })
                .await
                .unwrap()
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), starts.recv())
            .await
            .unwrap()
            .unwrap();
        ask_agent(&parent, parent_backend).await;
        ask_agent(&child, child_backend).await;
        let (parent_result, child_result, ()) = tokio::join!(
            run_user_turn(&parent, "parent-tool-route"),
            run_user_turn(&child, "child-tool-route"),
            async {
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    tokio::join!(parent_answer.wait_blocked(), child_answer.wait_blocked());
                })
                .await
                .unwrap();
                assert!(
                    parent
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()
                        .is_empty()
                );
                assert!(
                    child
                        .chat_state_handle
                        .pending_notifications()
                        .await
                        .unwrap()
                        .is_empty()
                );
                parent_answer.release();
                child_answer.release();
            }
        );
        parent_result.unwrap();
        child_result.unwrap();
        // A completed independent Sideband may legally miss the other actor's
        // final snapshot. Only an explicit new user Turn can consume that late
        // pending context; the inquiry must never resurrect the old Turn.
        for (actor, server, prompt) in [
            (&parent, &parent_server, "parent-explicit-followup"),
            (&child, &child_server, "child-explicit-followup"),
        ] {
            if !actor
                .chat_state_handle
                .pending_notifications()
                .await
                .unwrap()
                .is_empty()
            {
                server.enqueue_response(
                    "/v1/messages",
                    messages_turn(&[text_block("Explicit next user work.")], "end_turn"),
                );
                run_user_turn(actor, prompt).await.unwrap();
            }
        }

        assert!(!cancelled.is_cancelled());
        assert_actor_inquiry(
            &parent,
            &parent.chat_state_handle.timeline_events().await.unwrap(),
            "route-child",
            InquiryDirection::ChildToParent,
            "parent-asks",
            "Child consultation answer: investigation is ready.",
            "Child asks parent about scope.",
            "Parent consultation answer: preserve scope.",
        );
        assert_actor_inquiry(
            &child,
            &child.chat_state_handle.timeline_events().await.unwrap(),
            "route-parent",
            InquiryDirection::ParentToChild,
            "child-asks",
            "Parent consultation answer: preserve scope.",
            "Parent asks child about investigation.",
            "Child consultation answer: investigation is ready.",
        );
        for (actor, server, question, answer) in [
            (
                &parent,
                &parent_server,
                "Child asks parent about scope.",
                "Parent consultation answer: preserve scope.",
            ),
            (
                &child,
                &child_server,
                "Parent asks child about investigation.",
                "Child consultation answer: investigation is ready.",
            ),
        ] {
            assert!((3..=4).contains(&server.requests().len()));
            let last = server.request_bodies().last().unwrap().to_string();
            assert!(last.contains(question) && last.contains(answer));
            let surface = actor.chat_state_handle.get_conversation().await;
            let quoted = surface
                .iter()
                .filter_map(|item| match item {
                    sampling_types::ConversationItem::AgentMessage(batch) => Some(&batch.messages),
                    _ => None,
                })
                .flatten()
                .filter(|message| message.message.contains(question))
                .collect::<Vec<_>>();
            assert_eq!(quoted.len(), 1);
            assert!(quoted[0].message.contains(answer));
            assert!(quoted[0].reply_to.is_none());
            assert!(
                actor
                    .chat_state_handle
                    .pending_notifications()
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        release.notify_one();
        assert_eq!(seeded.await.unwrap().child_session_id, "route-child");
    });
}
