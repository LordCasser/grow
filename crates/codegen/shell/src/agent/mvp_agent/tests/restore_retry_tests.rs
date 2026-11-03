use super::*;
use acp_transport::AcpAgentHandler as _;
use chat_state::session_runtime::{RuntimePhase, SessionRuntimeEvent};

fn replace_checkpoint_source(path: &std::path::Path, source: &str) -> String {
    use crate::session::history::{JournalFact, decode_record, encode_domain};
    let mut result = String::new();
    let mut original = None;
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let record = decode_record(line.as_bytes()).unwrap();
        if let JournalFact::Domain {
            mut event,
            response_anchor,
        } = record.fact
            && let chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::Checkpoint {
                checkpoint,
                ..
            }) = &mut event.kind
        {
            assert!(original.is_none());
            original = Some(std::mem::replace(
                &mut checkpoint.runtime_source_hash,
                source.into(),
            ));
            result.push_str(
                std::str::from_utf8(
                    &encode_domain(
                        record.seq,
                        &serde_json::value::to_raw_value(&event).unwrap(),
                        response_anchor,
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }
    std::fs::write(path, result).unwrap();
    original.unwrap()
}

#[test]
fn diagnostic_source_drift_does_not_block_compatible_restore() {
    const HOME: &str = "GROW_TEST_RESTORE_RETRY_HOME";
    let Ok(home) = std::env::var(HOME) else {
        let temp = tempfile::tempdir().unwrap();
        for phase in ["suspend", "restore"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "agent::mvp_agent::tests::restore_retry_tests::diagnostic_source_drift_does_not_block_compatible_restore", "--nocapture"])
                .env("GROW_HOME", temp.path()).env(HOME, temp.path()).env("GROW_TEST_RESTORE_RETRY_PHASE", phase)
                .output().unwrap();
            assert!(
                output.status.success(),
                "{phase}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    };
    run_local_for_bridge_test(|| async move {
        let home = std::path::Path::new(&home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        drain_test_gateway_completions(rx);
        let agent = MvpAgent::new(GatewaySender::new(tx), &valid_agent_config()).unwrap();
        cold_recovery_stage("retry.initialize", agent.initialize(acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(serde_json::json!({"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        let phase = std::env::var("GROW_TEST_RESTORE_RETRY_PHASE").unwrap();
        if phase == "suspend" {
            let sid = cold_recovery_stage(
                "retry.new",
                agent.new_session(acp::NewSessionRequest::new(cwd.clone())),
            )
            .await
            .unwrap()
            .session_id;
            std::fs::write(home.join("session-id"), sid.0.as_ref()).unwrap();
            let (operation, nodes) = cold_recovery_stage(
                "retry.suspend",
                agent.suspend_session_tree(
                    &sid,
                    tokio::time::Instant::now() + std::time::Duration::from_secs(8),
                ),
            )
            .await
            .unwrap();
            std::fs::write(home.join("operation"), operation).unwrap();
            assert_eq!(nodes.len(), 1);
            let path = crate::session::persistence::session_dir(&crate::session::info::Info {
                id: sid,
                cwd: cwd.to_str().unwrap().to_owned(),
            })
            .join("timeline.jsonl");
            // The old process has retired its writer. Corrupt only the source binding,
            // maintaining a valid physical record so public load reaches admission.
            replace_checkpoint_source(&path, &"0".repeat(64));
        } else {
            let sid =
                acp::SessionId::new(std::fs::read_to_string(home.join("session-id")).unwrap());
            let info = crate::session::info::Info {
                id: sid.clone(),
                cwd: cwd.to_str().unwrap().to_owned(),
            };
            let path = crate::session::persistence::session_dir(&info).join("timeline.jsonl");
            cold_recovery_stage(
                "retry.restored",
                agent.load_session(acp::LoadSessionRequest::new(sid.clone(), cwd.clone())),
            )
            .await
            .unwrap();
            let handle = agent.control_session_handle(&sid).unwrap();
            let runtime = handle.chat_state_handle.runtime_state().await.unwrap();
            assert_eq!(runtime.phase, RuntimePhase::Running);
            assert_eq!(
                runtime.checkpoint.as_ref().unwrap().runtime_source_hash,
                "0".repeat(64),
                "diagnostic fingerprints are never rewritten to manufacture admission"
            );
            assert_eq!(
                runtime.operation_id.as_deref(),
                Some(
                    std::fs::read_to_string(home.join("operation"))
                        .unwrap()
                        .as_str()
                )
            );
            let events = handle.chat_state_handle.timeline_events().await.unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Runtime(
                            SessionRuntimeEvent::RestorePrepared { .. }
                        )
                    ))
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        chat_state::TimelineEventKind::Runtime(
                            SessionRuntimeEvent::ExecutionReleased { .. }
                        )
                    ))
                    .count(),
                1
            );
            cold_recovery_stage(
                "retry.live-reconnect",
                agent.load_session(acp::LoadSessionRequest::new(sid.clone(), cwd)),
            )
            .await
            .unwrap();
            assert!(
                agent
                    .control_session_handle(&sid)
                    .unwrap()
                    .cmd_tx
                    .same_channel(&handle.cmd_tx)
            );
        }
    });
}

#[tokio::test(flavor = "current_thread")]
async fn restore_retry_preserves_prepared_and_running_owner() {
    tokio::task::LocalSet::new().run_until(async {
        let agent = build_minimal_agent_for_tests();
        let (events, _) = tokio::sync::mpsc::unbounded_channel();
        let config = serde_json::from_value(serde_json::json!({"base_url":"http://localhost", "model":"test", "context_window":256000})).unwrap();
        let chat = chat_state::ChatStateActor::spawn(vec![], config, Box::new(chat_state::NullTimelinePersistence), events, tokio_util::sync::CancellationToken::new());
        let (mut handle, mut commands) = make_test_handle_with_receiver("test", None);
        handle.chat_state_handle = chat.clone();
        agent.sessions.borrow_mut().insert(handle.info.id.clone(), handle.clone());
        assert!(!agent.retire_failed_restore_candidate(&handle).await.unwrap());
        for event in [SessionRuntimeEvent::SuspendRequested { operation_id:"op".into(), root_session_id: handle.info.id.0.to_string() }, SessionRuntimeEvent::CommandsSettled { operation_id:"op".into(), commands:vec![] }] {
            chat.record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(event)).await.unwrap();
        }
        chat.checkpoint_runtime("op".into(), "a".repeat(64), None, None, tokio::time::Instant::now() + std::time::Duration::from_secs(2)).await.unwrap();
        chat.record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::RestorePrepared { operation_id:"op".into(), instance_id:"instance".into() })).await.unwrap();
        assert!(!agent.retire_failed_restore_candidate(&handle).await.unwrap());
        assert!(commands.try_recv().is_err(), "no retirement command may reach a prepared/live owner");
        assert!(agent.control_session_handle(&handle.info.id).unwrap().cmd_tx.same_channel(&handle.cmd_tx));
    }).await;
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_retirement_never_turns_into_logical_teardown() {
    tokio::task::LocalSet::new().run_until(async {
        for (acknowledge_before_cancel, panic_exit) in [(false, false), (false, true), (true, false), (true, true)] {
            let agent = build_minimal_agent_for_tests();
            let (events, _) = tokio::sync::mpsc::unbounded_channel();
            let config = serde_json::from_value(serde_json::json!({"base_url":"http://localhost", "model":"test", "context_window":256000})).unwrap();
            let chat = chat_state::ChatStateActor::spawn(vec![], config, Box::new(chat_state::NullTimelinePersistence), events, tokio_util::sync::CancellationToken::new());
            let (mut handle, mut commands) = make_test_handle_with_receiver("test", None);
            handle.chat_state_handle = chat.clone();
            let id = handle.info.id.clone();
            agent.sessions.borrow_mut().insert(id.clone(), handle.clone());
            for event in [SessionRuntimeEvent::SuspendRequested { operation_id:"op".into(), root_session_id: id.0.to_string() }, SessionRuntimeEvent::CommandsSettled { operation_id:"op".into(), commands:vec![] }] {
                chat.record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(event)).await.unwrap();
            }
            chat.checkpoint_runtime("op".into(), "a".repeat(64), None, None, tokio::time::Instant::now() + std::time::Duration::from_secs(2)).await.unwrap();
            let (release, writer_exit) = std::sync::mpsc::channel();
            let thread = std::thread::spawn(move || { writer_exit.recv().unwrap(); assert!(!panic_exit, "injected panic after checkpoint retirement"); });
            agent.session_threads.borrow_mut().insert(id.clone(), crate::session::SessionThread::from_handle(thread));
            let cleanup = tokio::time::timeout(std::time::Duration::from_millis(20), agent.retire_failed_restore_candidate(&handle));
            let driver = async {
                let SessionCommand::RetireSuspendedRuntime { respond_to, .. } = commands.recv().await.unwrap() else { panic!("expected physical retirement"); };
                if acknowledge_before_cancel { respond_to.send(Ok(())).unwrap(); None } else { Some(respond_to) }
            };
            let (cancelled, late_ack) = tokio::join!(cleanup, driver);
            assert!(cancelled.is_err(), "load future is cancelled while its owner is draining");
            if let Some(late_ack) = late_ack { assert!(late_ack.send(Ok(())).is_err()); }
            if acknowledge_before_cancel {
                assert!(agent.control_session_handle(&id).is_none(), "ACK immediately leaves residency");
            } else {
                assert!(agent.resident_resources.borrow().get(&id).unwrap().failed_suspended_restore, "unconfirmed ownership survives waiter cancellation");
            }
            assert!(agent.session_threads.borrow().contains_key(&id), "draining writer stays owned");
            release.send(()).unwrap();
            while !agent.session_threads.borrow().get(&id).unwrap().is_finished() { tokio::task::yield_now().await; }
            agent.sweep_dead_sessions().await;
            assert!(agent.control_session_handle(&id).is_none());
            assert!(!agent.session_threads.borrow().contains_key(&id));
            let mut coordinator = agent.subagent_event_rx.borrow_mut().take().unwrap();
            assert!(coordinator.try_recv().is_err(), "physical retirement must not send logical TeardownSession");
            assert_eq!(chat.runtime_state().await.unwrap().phase, RuntimePhase::Suspended);
        }
    }).await;
}
