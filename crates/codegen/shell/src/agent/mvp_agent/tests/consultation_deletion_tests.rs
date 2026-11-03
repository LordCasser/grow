//! Public permanent deletion joins the actual SessionThread and JSONL writer.
use super::*;
use crate::coordination::{
    InboundInquiry, InquiryAuthority, InquiryCancellation, InquiryDirection, InquiryPhase,
    InquiryStatus,
};
use crate::session::SessionCommand;
use acp_transport::AcpAgentHandler as _;
use test_support::{InferenceEndpoint, InferenceRequestMatcher, MockInferenceServer};

fn deletion_inquiry(
    info: &crate::session::info::Info,
    id: &str,
) -> (
    InboundInquiry,
    tokio::sync::watch::Receiver<InquiryPhase>,
    tokio::sync::oneshot::Receiver<crate::coordination::InquiryOutcome>,
) {
    let (progress, phases) = tokio::sync::watch::channel(InquiryPhase::Receiving);
    let (respond_to, response) = tokio::sync::oneshot::channel();
    (
        InboundInquiry {
            authority: InquiryAuthority::Peer,
            direction: InquiryDirection::Peer,
            inquiry_id: id.into(),
            source_peer_id: "delete-source-peer".into(),
            source_session_id: "delete-source-session".into(),
            source_cwd: info.cwd.clone(),
            delegated_subagent_task_name: None,
            target_session_id: info.id.to_string(),
            question: format!("Question owned by {id}"),
            cancellation: InquiryCancellation::new(),
            progress,
            respond_to,
        },
        phases,
        response,
    )
}

#[test]
#[cfg(unix)]
fn permanent_delete_drains_active_and_queued_consultations_before_removing_real_history() {
    const CHILD: &str = "GROW_DELETE_CONSULTATION_TEST_HOME";
    let Ok(home) = std::env::var(CHILD) else {
        let home = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "agent::mvp_agent::tests::consultation_deletion_tests::permanent_delete_drains_active_and_queued_consultations_before_removing_real_history", "--nocapture"])
            .env(CHILD, home.path()).env("GROW_HOME", home.path())
            .env("RUST_MIN_STACK", "16777216").output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    };
    run_local_for_bridge_test(|| async move {
        use crate::session::actor::tests::truncation_recovery_tests::{messages_turn, text_block};
        use std::io::Read;
        let home = std::path::Path::new(&home);
        let cwd = home.join("workspace");
        std::fs::create_dir_all(&cwd).unwrap();
        let cwd = cwd.canonicalize().unwrap();
        let server = MockInferenceServer::start().await.unwrap();
        let mut answer = server.expect_response_blocked(
            "active inquiry at deletion",
            InferenceRequestMatcher::auxiliary(InferenceEndpoint::Messages),
            messages_turn(
                &[text_block(
                    "A late HTTP response cannot write deleted history.",
                )],
                "end_turn",
            ),
        );
        let raw = format!(
            "[models]\ndefault = \"test/default\"\n[provider.test]\napi_backend = \"messages\"\n[provider.test.options]\nbase_url = \"{}\"\napi_key = \"test-only\"\n[provider.test.models.default]\nmodel = \"test-model\"\ncontext_window = 128000\n",
            server.url()
        );
        std::fs::write(home.join("config.toml"), &raw).unwrap();
        let mut cfg =
            crate::agent::config::Config::new_from_toml_cfg(&toml::from_str(&raw).unwrap())
                .unwrap();
        cfg.default_model_override = None;
        cfg.remote_settings = Some(crate::util::config::RemoteSettings::default());
        let (gateway_tx, gateway_rx) = tokio::sync::mpsc::unbounded_channel();
        drain_test_gateway_completions(gateway_rx);
        let agent = MvpAgent::new(GatewaySender::new(gateway_tx), &cfg).unwrap();
        cold_recovery_stage("consultation-delete.initialize", agent.initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1).meta(
                serde_json::json!({"startupHints":{"nonInteractive":true,"skipGitStatus":true,"skipProjectLayout":true}}).as_object().cloned()
            ))).await.unwrap();
        agent.set_auth_method(acp::AuthMethodId::new("provider.api_key"));
        let sid = cold_recovery_stage(
            "consultation-delete.new",
            agent.new_session(acp::NewSessionRequest::new(cwd)),
        )
        .await
        .unwrap()
        .session_id;
        let handle = agent.control_session_handle(&sid).unwrap();
        let info = handle.info.clone();
        let directory = handle.session_directory.display_path().to_path_buf();
        let (active, _, active_result) = deletion_inquiry(&info, "delete-active-inquiry");
        handle
            .cmd_tx
            .send(SessionCommand::RunCoordinationInquiry { inquiry: active })
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(10), answer.wait_blocked())
            .await
            .unwrap();
        let (queued, mut queued_phases, queued_result) =
            deletion_inquiry(&info, "delete-queued-inquiry");
        handle
            .cmd_tx
            .send(SessionCommand::RunCoordinationInquiry { inquiry: queued })
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while *queued_phases.borrow_and_update() != InquiryPhase::Queued {
                queued_phases.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(
            server.requests().len(),
            1,
            "the second inquiry must still be queued"
        );

        // The production replay fence pins the real writer frontier and keeps
        // the real target mailbox from consuming Cancel/Shutdown until release.
        let (respond_to, paused) = tokio::sync::oneshot::channel();
        let (release, wait) = tokio::sync::oneshot::channel();
        handle
            .cmd_tx
            .send(SessionCommand::PauseForReplay {
                respond_to,
                release: wait,
            })
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), paused)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let mut journal = std::fs::File::open(directory.join("timeline.jsonl")).unwrap();
        let deletion = agent.ext_method(acp::ExtRequest::new(
            "grow/session/delete",
            serde_json::value::to_raw_value(
                &serde_json::json!({"sessionId":info.id.0,"cwd":info.cwd}),
            )
            .unwrap()
            .into(),
        ));
        tokio::pin!(deletion);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::select! {
                result = &mut deletion => panic!("deletion cannot complete before its writer drains: {result:?}"),
                _ = async { while agent.sessions.borrow().contains_key(&sid) { tokio::task::yield_now().await; } } => {}
            }
        }).await.unwrap();
        assert!(
            directory.is_dir(),
            "preflight cannot remove history before the real writer drains"
        );
        assert!(agent.has_live_or_draining_session(&sid));
        release.send(false).unwrap();
        let active = tokio::time::timeout(std::time::Duration::from_secs(10), active_result)
            .await
            .unwrap()
            .unwrap();
        let queued = tokio::time::timeout(std::time::Duration::from_secs(10), queued_result)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(active.status, InquiryStatus::Cancelled);
        assert_eq!(queued.status, InquiryStatus::Cancelled);
        let response = tokio::time::timeout(std::time::Duration::from_secs(15), &mut deletion)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(response.0.get()).unwrap()["success"],
            true
        );
        assert!(!agent.has_live_or_draining_session(&sid));
        assert!(!directory.exists());
        assert_eq!(server.requests().len(), 1);

        // A retained Unix file descriptor observes the actual final append-only
        // journal after unlink, so these assertions cannot race physical deletion.
        let mut bytes = Vec::new();
        journal.read_to_end(&mut bytes).unwrap();
        let events = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .filter_map(
                |line| match crate::session::history::decode_record(line).unwrap().fact {
                    crate::session::history::JournalFact::Domain { event, .. } => Some(event),
                    _ => None,
                },
            )
            .collect::<Vec<_>>();
        chat_state::Timeline::from_events(events.clone()).unwrap();
        let terminals = events
            .iter()
            .filter_map(|event| crate::coordination::InquiryEvent::from_timeline(event).unwrap())
            .filter_map(|event| match event {
                crate::coordination::InquiryEvent::Incoming { inquiry_id, audit }
                    if audit.outcome.is_some() =>
                {
                    Some((inquiry_id, audit))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(terminals.len(), 2);
        for (id, audit) in terminals {
            assert!(["delete-active-inquiry", "delete-queued-inquiry"].contains(&id.as_str()));
            assert_eq!(audit.outcome.unwrap().status, InquiryStatus::Cancelled);
            assert!(audit.answer_context.is_none());
        }
        assert!(!events.iter().any(|event| matches!(
            &event.kind,
            chat_state::TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
                source: chat_state::NotificationSource::InquiryAnswered { .. },
                ..
            })
        )));
        answer.release();
        let (late, _, late_result) = deletion_inquiry(&info, "late-after-delete");
        assert!(
            handle
                .cmd_tx
                .send(SessionCommand::RunCoordinationInquiry { inquiry: late })
                .is_err()
        );
        assert!(late_result.await.is_err());
        assert!(
            !directory.exists(),
            "a retired producer cannot recreate the deleted directory"
        );
    });
}
