//! Peer tool and external API share the real runtime, not delegation routing.
use super::*;
use crate::agent::mvp_agent::tests::build_minimal_agent_for_tests;
use acp_transport::{AcpAgentHandler, protocol as acp};
use test_support::{InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer};

async fn peer_journey(external_api: bool) {
    use tools::implementations::grow_build::{
        coordination::{AskSessionTool, ListActiveSessionsTool},
        read_file::ReadFileTool,
    };
    use tools::registry::types::ToolConfig;
    let source_server = MockInferenceServer::start().await.unwrap();
    let target_server = MockInferenceServer::start().await.unwrap();
    let source = actor_with_sampler_named(&source_server, "peer-source").await;
    let target = actor_with_sampler_named(&target_server, "peer-target").await;
    let source_handle = route_handle(&source);
    let target_handle = route_handle(&target);
    let home = tempfile::tempdir().unwrap();
    let mut agent = build_minimal_agent_for_tests();
    agent.coordination = crate::coordination::CoordinationRuntime::new(home.path().to_owned());
    agent
        .sessions
        .borrow_mut()
        .insert(source_handle.info.id.clone(), source_handle.clone());
    agent
        .sessions
        .borrow_mut()
        .insert(target_handle.info.id.clone(), target_handle);
    let agent = Rc::new(agent);
    assert!(agent.ensure_coordination_started().await);
    *source.agent.borrow_mut() = test_agent_with_tools(vec![
        ToolConfig::for_tool::<AskSessionTool>(),
        ToolConfig::for_tool::<ListActiveSessionsTool>(),
        ToolConfig::for_tool::<ReadFileTool>(),
    ])
    .await;
    source
        .agent
        .borrow()
        .tool_bridge()
        .update_resource(agent.coordination_backend_resource(source_handle))
        .await;
    *target.agent.borrow_mut() = test_agent_with_tools(vec![
        ToolConfig::for_tool::<ReadFileTool>(),
        ToolConfig::for_tool::<tools::implementations::grow_build::todo::TodoWriteTool>(),
    ])
    .await;
    let question = if external_api {
        "External caller asks peer state."
    } else {
        "Model tool asks peer state."
    };
    let answer = "The peer consultation answered from its own frozen context.";
    let inquiry_id = uuid::Uuid::now_v7().to_string();
    if !external_api {
        source_server.enqueue_response("/v1/messages", messages_turn(&[tool_use_block("peer-tool-call", "ask_session",
            &serde_json::json!({"inquiry_id":inquiry_id,"target_session_id":"peer-target","question":question}).to_string())], "tool_use"));
        source_server.enqueue_response(
            "/v1/messages",
            messages_turn(
                &[text_block("Source used its original tool result.")],
                "end_turn",
            ),
        );
    }
    let mut foreground = target_server.expect_response_blocked(
        "peer foreground in flight",
        InferenceRequestMatcher::foreground(InferenceEndpoint::Messages),
        messages_turn(
            &[text_block("Original peer foreground response.")],
            "end_turn",
        ),
    );
    let mut sideband = target_server.expect_response_blocked(
        "real peer InfoRequest",
        InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
        messages_turn(&[text_block(answer)], "end_turn"),
    );
    target_server.enqueue_response(
        "/v1/messages",
        messages_turn(
            &[text_block("Peer foreground now knows the consultation.")],
            "end_turn",
        ),
    );
    let target_turn_actor = target.clone();
    let target_turn = tokio::task::spawn_local(async move {
        run_user_turn(&target_turn_actor, "peer-foreground").await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), foreground.wait_blocked())
        .await
        .unwrap();
    let before_source =
        serde_json::to_value(source.chat_state_handle.get_conversation().await).unwrap();
    let source_turn_actor = source.clone();
    let call_agent = agent.clone();
    let request_id = inquiry_id.clone();
    let source_call = tokio::task::spawn_local(async move {
        if external_api {
            let response = call_agent.ext_method(acp::ExtRequest::new("grow/coordination/ask",
                serde_json::value::to_raw_value(&serde_json::json!({"inquiryId":request_id,
                    "sourceSessionId":"peer-source","targetSessionId":"peer-target","question":question})).unwrap().into())).await.unwrap();
            let result: crate::coordination::InquiryOutcome =
                serde_json::from_str(response.0.get()).unwrap();
            assert_eq!(result.status, crate::coordination::InquiryStatus::Answered);
            assert_eq!(result.answer.as_deref(), Some(answer));
        } else {
            run_user_turn(&source_turn_actor, "peer-source-tool")
                .await
                .unwrap();
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), sideband.wait_blocked())
        .await
        .unwrap();
    assert!(!foreground.is_satisfied());
    sideband.release();
    tokio::time::timeout(std::time::Duration::from_secs(10), source_call)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(target.sampler_handle.active_count().await, 1);
    assert!(!*target.parent_message_interrupt.borrow());
    assert!(target.pending_interjections.is_empty());
    assert_eq!(
        target
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap()
            .len(),
        1
    );
    foreground.release();
    target_turn.await.unwrap().unwrap();
    assert_eq!(target_server.requests().len(), 3);
    let target_wire = target_server.request_bodies().last().unwrap().to_string();
    assert!(target_wire.contains(question) && target_wire.contains(answer));
    assert!(target_wire.contains("peer-source"));
    let source_surface = source.chat_state_handle.get_conversation().await;
    assert!(
        !source_surface
            .iter()
            .any(|item| item.text_content().contains("Consultation record:"))
    );
    let source_results = source_surface.iter().filter(|item| matches!(item,
        sampling_types::ConversationItem::ToolResult(result) if result.tool_call_id == "peer-tool-call")).collect::<Vec<_>>();
    if external_api {
        assert_eq!(
            serde_json::to_value(&source_surface).unwrap(),
            before_source
        );
        assert!(source_results.is_empty());
        assert!(source_server.requests().is_empty());
    } else {
        assert_eq!(source_results.len(), 1);
        assert!(source_results[0].text_content().contains(answer));
        assert_eq!(source_server.requests().len(), 2);
    }
    let source_events = source.chat_state_handle.timeline_events().await.unwrap();
    assert_eq!(source_events.iter().filter_map(|event| crate::coordination::InquiryEvent::from_timeline(event).unwrap())
        .filter(|event| matches!(event, crate::coordination::InquiryEvent::OutgoingCompleted { audit } if audit.outcome.inquiry_id == inquiry_id)).count(), 1);
    let target_events = target.chat_state_handle.timeline_events().await.unwrap();
    assert!(target_events.iter().filter_map(|event| crate::coordination::InquiryEvent::from_timeline(event).unwrap()).any(|event|
        matches!(event, crate::coordination::InquiryEvent::Incoming { inquiry_id: id, audit } if id == inquiry_id
            && audit.direction == crate::coordination::InquiryDirection::Peer && audit.source_session_id == "peer-source" && audit.answer_context.is_some())));
    assert!(
        target
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap()
            .is_empty()
    );
    let surface = target.chat_state_handle.get_conversation().await;
    let quoted = surface
        .iter()
        .filter_map(|item| match item {
            sampling_types::ConversationItem::User(user)
                if item.text_content().contains("Consultation record:") =>
            {
                Some(user)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(quoted.len(), 1);
    assert!(quoted[0].permission_evidence.is_none());
}

#[test]
fn peer_tool_and_external_api_reach_actual_target_without_source_synthetic_input() {
    run_local_for_bridge_test(|| async {
        peer_journey(false).await;
        peer_journey(true).await;
    });
}
