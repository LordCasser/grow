use super::{
    build_minimal_agent_for_tests, make_test_handle_with_receiver, run_local_for_bridge_test,
};
use crate::coordination::{
    CoordinationErrorCode, CoordinationRuntime, InquiryOutcome, LocalSessionSnapshot,
};
use tool_runtime::Tool;
use tools::implementations::grow_build::coordination::{AskSessionInput, AskSessionTool};
use tools::types::resources::{NotificationHandle, Resources};
use tools::types::tool_metadata::test_ctx;

#[test]
fn ask_session_tool_blank_ids_create_inquiries_and_explicit_ids_remain_strict() {
    run_local_for_bridge_test(|| async {
        let home = tempfile::tempdir().unwrap();
        let mut agent = build_minimal_agent_for_tests();
        agent.coordination = CoordinationRuntime::new(home.path().to_owned());
        agent.coordination.ensure_started().await.unwrap();
        agent.coordination.publish_sessions(
            ["test", "target"]
                .into_iter()
                .map(|id| LocalSessionSnapshot {
                    session_id: id.into(),
                    canonical_cwd: "/repo".into(),
                    main_agent: "test-agent".into(),
                    activity: crate::agent::roster::RosterActivity::Idle,
                    subagents: Default::default(),
                })
                .collect(),
        );
        let mut incoming = agent.coordination.take_inquiry_receiver().unwrap();
        let (source, mut commands) = make_test_handle_with_receiver("test/default", None);
        let mut resources = Resources::new();
        resources.insert(agent.coordination_backend_resource(source));
        resources.insert(NotificationHandle(
            tools::notification::ToolNotificationHandle::noop(),
        ));
        let resources = resources.into_shared();
        // Acknowledge the source audit boundary without touching a live session.
        let audit_task = tokio::spawn(async move {
            while let Some(command) = commands.recv().await {
                let crate::session::SessionCommand::RecordCoordinationInquiry {
                    respond_to, ..
                } = command
                else {
                    panic!("unexpected source command");
                };
                respond_to.send(Ok(())).unwrap();
            }
        });
        let delivered = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = delivered.clone();
        let answer_task = tokio::spawn(async move {
            while let Some(inquiry) = incoming.recv().await {
                observed.lock().unwrap().push(inquiry.inquiry_id.clone());
                inquiry
                    .respond_to
                    .send(InquiryOutcome::answered(
                        inquiry.inquiry_id,
                        "test answer".into(),
                    ))
                    .unwrap();
            }
        });
        let mut ids = std::collections::HashSet::new();
        let mut last_id = String::new();
        for field in [
            None,
            Some(serde_json::Value::Null),
            Some("".into()),
            Some(" \t\r\n".into()),
            Some("\u{3000}".into()),
        ] {
            let mut wire = serde_json::json!({"target_session_id":"target", "question":"work?"});
            if let Some(value) = field {
                wire["inquiry_id"] = value;
            }
            let input: AskSessionInput = serde_json::from_value(wire).unwrap();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                AskSessionTool.run(test_ctx(resources.clone()), input),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(result.status, "answered", "{result:?}");
            assert_eq!(result.answer.as_deref(), Some("test answer"));
            assert_eq!(
                uuid::Uuid::parse_str(&result.inquiry_id)
                    .unwrap()
                    .get_version(),
                Some(uuid::Version::SortRand)
            );
            assert!(
                ids.insert(result.inquiry_id.clone()),
                "new calls need distinct identities"
            );
            last_id = result.inquiry_id;
        }
        let retry = AskSessionTool
            .run(
                test_ctx(resources.clone()),
                AskSessionInput {
                    inquiry_id: Some(last_id.clone()),
                    target_session_id: "target".into(),
                    question: "work?".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(retry.inquiry_id, last_id);
        assert_eq!(retry.status, "answered");
        let conflict = AskSessionTool
            .run(
                test_ctx(resources.clone()),
                AskSessionInput {
                    inquiry_id: Some(last_id.clone()),
                    target_session_id: "target".into(),
                    question: "changed?".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            conflict.error.unwrap().code,
            CoordinationErrorCode::Conflict
        );
        for id in [
            "not-a-uuid".into(),
            uuid::Uuid::new_v4().to_string(),
            format!(" {last_id} "),
        ] {
            let result = AskSessionTool
                .run(
                    test_ctx(resources.clone()),
                    AskSessionInput {
                        inquiry_id: Some(id.clone()),
                        target_session_id: "target".into(),
                        question: "work?".into(),
                    },
                )
                .await
                .unwrap();
            assert_eq!(result.inquiry_id, id, "nonblank IDs must remain unchanged");
            assert_eq!(result.status, "failed");
            assert_eq!(
                result.error.unwrap().code,
                CoordinationErrorCode::InvalidRequest
            );
        }
        // External explicit-ID APIs do not inherit model-input normalization.
        let handle = agent.coordination.handle();
        let error = handle
            .ask_session("", "test", "target", "work?", None, Default::default())
            .await
            .unwrap_err();
        assert_eq!(error.code, CoordinationErrorCode::InvalidRequest);
        assert_eq!(
            handle.get_inquiry("", "test").await.unwrap_err().code,
            CoordinationErrorCode::InvalidRequest
        );
        assert_eq!(
            handle
                .cancel_session("", "test", "target")
                .await
                .unwrap_err()
                .code,
            CoordinationErrorCode::InvalidRequest
        );
        // Every fresh call was delivered once; retries and invalid IDs deliver nothing.
        let delivered = delivered.lock().unwrap();
        assert_eq!(delivered.len(), 5);
        assert_eq!(
            delivered
                .iter()
                .cloned()
                .collect::<std::collections::HashSet<_>>(),
            ids
        );
        answer_task.abort();
        audit_task.abort();
    });
}
