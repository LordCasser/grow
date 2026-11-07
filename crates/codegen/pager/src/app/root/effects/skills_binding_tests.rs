use super::*;
use acp_transport::AcpAgentMessage;

#[tokio::test]
async fn skills_binding_effects_send_exact_session_and_preserve_completion_owner() {
    for toggle in [false, true] {
        for success in [false, true] {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let mut tasks = JoinSet::new();
            let sid = acp::SessionId::new("skills-owner-A");
            let effect = if toggle {
                Effect::ToggleSkill {
                    agent_id: AgentId(7),
                    session_id: sid.clone(),
                    binding_epoch: 19,
                    skill_name: "deploy".into(),
                    enabled: false,
                }
            } else {
                Effect::FetchSkillsList {
                    agent_id: AgentId(7),
                    session_id: sid.clone(),
                    binding_epoch: 19,
                }
            };
            execute(
                effect,
                &mut tasks,
                &tx,
                Path::new("/unrelated-process-cwd"),
                &SessionFlags::default(),
            );
            let AcpAgentMessage::ExtMethod(args) = rx.recv().await.unwrap() else {
                panic!("expected Skills RPC");
            };
            assert_eq!(
                args.request.method.as_ref(),
                if toggle {
                    "grow/skills/toggle"
                } else {
                    "grow/skills/list"
                }
            );
            let params: serde_json::Value =
                serde_json::from_str(args.request.params.get()).unwrap();
            let expected = if toggle {
                serde_json::json!({"sessionId":"skills-owner-A", "name":"deploy", "enabled":false})
            } else {
                serde_json::json!({"sessionId":"skills-owner-A"})
            };
            assert_eq!(params, expected, "session requests must not include cwd");
            if success {
                args.response_tx
                    .send(Ok(acp::ExtResponse::new(
                        serde_json::value::to_raw_value(
                            &serde_json::json!({"result":{"skills":[]}}),
                        )
                        .unwrap()
                        .into(),
                    )))
                    .unwrap();
            } else {
                args.response_tx
                    .send(Err(acp::Error::internal_error().data("owner unavailable")))
                    .unwrap();
            }
            if toggle && success {
                let AcpAgentMessage::ExtMethod(refresh) = rx.recv().await.unwrap() else {
                    panic!("expected global baseline refresh");
                };
                assert_eq!(
                    refresh.request.method.as_ref(),
                    "grow/skills/refresh-baseline"
                );
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(refresh.request.params.get())
                        .unwrap(),
                    serde_json::json!({})
                );
                refresh
                    .response_tx
                    .send(Ok(acp::ExtResponse::new(
                        serde_json::value::to_raw_value(&serde_json::json!({}))
                            .unwrap()
                            .into(),
                    )))
                    .unwrap();
            }
            let completion = tasks.join_next().await.unwrap().unwrap();
            let (agent_id, session_id, binding_epoch, result) = match completion {
                TaskResult::SkillsListLoaded {
                    agent_id,
                    session_id,
                    binding_epoch,
                    result,
                } if !toggle => (agent_id, session_id, binding_epoch, result),
                TaskResult::SkillsToggleDone {
                    agent_id,
                    session_id,
                    binding_epoch,
                    result,
                } if toggle => (agent_id, session_id, binding_epoch, result),
                other => panic!("unexpected completion: {other:?}"),
            };
            assert_eq!((agent_id, session_id, binding_epoch), (AgentId(7), sid, 19));
            assert_eq!(result.is_ok(), success);
            assert!(
                rx.try_recv().is_err(),
                "only successful toggle broadcasts baseline refresh"
            );
        }
    }
}
