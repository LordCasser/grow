use super::*;

#[tokio::test(flavor = "current_thread")]
async fn context_extensions_wait_for_load_publication_before_dispatch() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for method in ["grow/btw", "grow/memory/flush", "grow/compact_conversation"] {
                let agent = std::rc::Rc::new(build_minimal_agent_for_tests());
                let sid = acp::SessionId::new("aux-loading");
                let guard = agent.begin_session_load(&sid);
                let params = if method == "grow/btw" {
                    serde_json::json!({"sessionId":sid.0.as_ref(), "question":"What happened?"})
                } else if method == "grow/memory/flush" {
                    serde_json::json!({"session_id":sid.0.as_ref()})
                } else {
                    serde_json::json!({"sessionId":sid.0.as_ref()})
                };
                let request = acp::ExtRequest::new(
                    method,
                    serde_json::value::to_raw_value(&params).unwrap().into(),
                );
                let other = agent.clone();
                let task = tokio::task::spawn_local(async move {
                    if method == "grow/btw" {
                        crate::extensions::feedback::handle(&other, &request).await
                    } else {
                        crate::extensions::memory::handle(&other, &request).await
                    }
                });
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                assert!(!task.is_finished(), "{method} must wait for session/load");
                let (handle, mut commands) = make_test_handle_with_receiver("test-model", None);
                agent.sessions.borrow_mut().insert(sid, handle);
                assert!(commands.try_recv().is_err());
                drop(guard);
                let command =
                    tokio::time::timeout(std::time::Duration::from_secs(5), commands.recv())
                        .await
                        .unwrap()
                        .unwrap();
                match command {
                    crate::session::SessionCommand::SideQuestion { respond_to, .. } => {
                        respond_to.send(Ok("restored answer".into())).unwrap();
                    }
                    crate::session::SessionCommand::FlushMemory { respond_to } => {
                        respond_to.send(Ok(true)).unwrap();
                    }
                    crate::session::SessionCommand::CompactSession { respond_to, .. } => {
                        respond_to
                            .send(Ok(crate::session::CompactConversationStatus::Completed))
                            .unwrap();
                    }
                    _ => panic!("unexpected command after publication"),
                }
                task.await.unwrap().unwrap();
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn context_extensions_report_closed_command_admission() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for method in ["grow/btw", "grow/memory/flush", "grow/compact_conversation"] {
                let agent = build_minimal_agent_for_tests();
                let sid = acp::SessionId::new("aux-closed");
                let (handle, commands) = make_test_handle_with_receiver("test-model", None);
                drop(commands);
                agent.sessions.borrow_mut().insert(sid.clone(), handle);
                let params = if method == "grow/btw" {
                    serde_json::json!({"sessionId":sid.0.as_ref(), "question":"Explain"})
                } else if method == "grow/memory/flush" {
                    serde_json::json!({"session_id":sid.0.as_ref()})
                } else {
                    serde_json::json!({"sessionId":sid.0.as_ref()})
                };
                let request = acp::ExtRequest::new(
                    method,
                    serde_json::value::to_raw_value(&params).unwrap().into(),
                );
                let result = if method == "grow/btw" {
                    crate::extensions::feedback::handle(&agent, &request).await
                } else {
                    crate::extensions::memory::handle(&agent, &request).await
                };
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("session command channel closed"),
                    "{method}"
                );
            }
        })
        .await;
}

#[tokio::test(flavor = "current_thread")]
async fn memory_flush_extension_reports_failed_checkpoint() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let agent = std::rc::Rc::new(build_minimal_agent_for_tests());
            let sid = acp::SessionId::new("aux-failed-checkpoint");
            let (handle, mut commands) = make_test_handle_with_receiver("test-model", None);
            agent.sessions.borrow_mut().insert(sid.clone(), handle);
            let request = acp::ExtRequest::new(
                "grow/memory/flush",
                serde_json::value::to_raw_value(&serde_json::json!({"session_id":sid.0.as_ref()}))
                    .unwrap()
                    .into(),
            );
            let other = agent.clone();
            let task = tokio::task::spawn_local(async move {
                crate::extensions::memory::handle(&other, &request).await
            });
            let crate::session::SessionCommand::FlushMemory { respond_to } =
                commands.recv().await.unwrap()
            else {
                panic!("wrong command")
            };
            respond_to.send(Ok(false)).unwrap();
            assert!(
                task.await
                    .unwrap()
                    .unwrap_err()
                    .to_string()
                    .contains("memory flush was skipped or failed")
            );
        })
        .await;
}
