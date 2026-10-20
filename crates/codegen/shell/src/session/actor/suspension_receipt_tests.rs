use super::*;
use chat_state::session_runtime::{
    CommandSettlementStatus, RuntimeCommandSettlement, SessionRuntimeEvent,
};

const OPERATION_ID: &str = "suspension-receipt-test";
const COMMAND_ID: &str = "completed-background-command";

fn with_session_runtime<F: std::future::Future<Output = ()>>(test: impl FnOnce() -> F + Send) {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn_scoped(scope, move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(tokio::task::LocalSet::new().run_until(test()));
            })
            .unwrap()
            .join()
            .unwrap();
    });
}

fn settled_command() -> RuntimeCommandSettlement {
    RuntimeCommandSettlement {
        command_id: COMMAND_ID.into(),
        call_id: "command-call".into(),
        outcome: CommandSettlementStatus::Completed,
        output_file: "/tmp/completed-command.log".into(),
        exit_code: Some(0),
        signal: None,
        output: "canonical command output".into(),
        is_backgrounded: true,
        owner: chat_state::NotificationOwner::Session,
        task_kind: chat_state::NotificationTaskKind::Task,
    }
}

fn command_source(command: &RuntimeCommandSettlement) -> chat_state::NotificationSource {
    chat_state::NotificationSource::TaskCompleted {
        task_id: command.command_id.clone(),
        task_kind: command.task_kind,
        owner: command.owner.clone(),
    }
}

async fn seed_suspending_command(actor: &SessionActor, command: RuntimeCommandSettlement) {
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
            SessionRuntimeEvent::SuspendRequested {
                operation_id: OPERATION_ID.into(),
                root_session_id: actor.session_id_string(),
            },
        ))
        .await
        .expect("suspension request must be durable");
    actor
        .chat_state_handle
        .record_timeline_event_durably(chat_state::TimelineEventKind::Runtime(
            SessionRuntimeEvent::CommandsSettled {
                operation_id: OPERATION_ID.into(),
                commands: vec![command],
            },
        ))
        .await
        .expect("command settlement must be durable");
}

async fn request_suspension(
    actor: &std::sync::Arc<SessionActor>,
) -> chat_state::session_runtime::RuntimeNodeRef {
    request_suspension_until(
        actor,
        tokio::time::Instant::now() + std::time::Duration::from_secs(10),
    )
    .await
    .expect("suspension must checkpoint")
}

async fn request_suspension_until(
    actor: &std::sync::Arc<SessionActor>,
    deadline: tokio::time::Instant,
) -> Result<chat_state::session_runtime::RuntimeNodeRef, String> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .request_node_suspension(
            OPERATION_ID.into(),
            actor.session_id_string(),
            deadline,
            respond_to,
        )
        .await;
    tokio::time::timeout(std::time::Duration::from_secs(12), response)
        .await
        .expect("suspension owner must finish")
        .expect("suspension response must be delivered")
}

async fn received_command_ids(actor: &SessionActor) -> Vec<String> {
    actor
        .chat_state_handle
        .timeline_events()
        .await
        .expect("Timeline is available")
        .iter()
        .filter_map(|event| match &event.kind {
            chat_state::TimelineEventKind::Notification(
                chat_state::NotificationEvent::Received {
                    id,
                    source: chat_state::NotificationSource::TaskCompleted { task_id, .. },
                    ..
                },
            ) if task_id == COMMAND_ID => Some(id.clone()),
            _ => None,
        })
        .collect()
}

async fn test_completed_receipt_is_reused(consumed: bool) {
    let (actor, _gateway_rx) = crate::session::actor::tests::support::build_actor().await;
    let command = settled_command();
    let source = command_source(&command);
    let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
    let original_body = "the normal terminal completion body";

    let turn = chat_state::TurnId(71);
    if consumed {
        actor
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Started {
                    prompt_id: "receipt-consumer".into(),
                    id: turn,
                    input_ids: Vec::new(),
                    identity: chat_state::TurnIdentity {
                        origin: "test".into(),
                        turn_kind: "internal".into(),
                        goal_id: None,
                        goal_definition_revision: None,
                        stage_id: None,
                    },
                    model_id: "test".into(),
                    input_message_count: 0,
                    prompt_index: 0,
                    prompt_text: "consume completion".into(),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    redirect_kind: None,
                },
            ))
            .await
            .expect("consumer turn must start");
    }
    let receipt_id = actor
        .receive_notification(source.clone(), source_version.clone(), original_body.into())
        .await
        .expect("normal completion receipt must be durable");
    if consumed {
        actor
            .consume_notifications_durably(vec![receipt_id.clone()], turn, None)
            .await
            .expect("normal completion receipt must be consumed");
        actor
            .chat_state_handle
            .record_timeline_event_durably(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Ended {
                    id: turn,
                    outcome: "completed".into(),
                    duration_ms: 0,
                    tool_count: 0,
                    terminal: chat_state::TurnTerminal {
                        source: chat_state::TurnTerminalSource::Host,
                        stop_reason: "stop".into(),
                        completion_kind: "test".into(),
                    },
                    cancellation_category: None,
                    details: None,
                },
            ))
            .await
            .expect("consumer turn must end");
    }
    seed_suspending_command(&actor, command).await;

    request_suspension(&actor).await;

    assert_eq!(received_command_ids(&actor).await, vec![receipt_id.clone()]);
    assert_eq!(
        actor
            .chat_state_handle
            .received_notification_id(source, source_version)
            .await,
        Some(Some(receipt_id.clone())),
        "the immutable receipt remains authoritative after suspension"
    );
    let pending = actor
        .chat_state_handle
        .pending_notifications()
        .await
        .expect("pending notification projection is available");
    assert_eq!(pending.iter().any(|item| item.id == receipt_id), !consumed);
}

#[test]
fn suspension_reuses_pending_normal_completion_receipt() {
    with_session_runtime(|| async { test_completed_receipt_is_reused(false).await });
}

#[test]
fn suspension_reuses_consumed_normal_completion_receipt() {
    with_session_runtime(|| async { test_completed_receipt_is_reused(true).await });
}

#[test]
fn suspension_retry_and_timeline_restore_keep_one_command_receipt() {
    with_session_runtime(|| async {
        let (actor, _gateway_rx) = crate::session::actor::tests::support::build_actor().await;
        let command = settled_command();
        let source = command_source(&command);
        let source_version = chat_state::NotificationSourceVersion::Ordinal { value: 1 };
        seed_suspending_command(&actor, command.clone()).await;

        let lease = actor
            .session_activities
            .try_start("suspension-receipt-test-owner")
            .expect("test owner is admitted before suspension closes admission");
        let first_deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(250);
        let first_actor = actor.clone();
        let first_attempt = tokio::task::spawn_local(async move {
            request_suspension_until(&first_actor, first_deadline).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !actor.background_service_shutdown.is_cancelled() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first attempt reaches auxiliary owner drain");
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while received_command_ids(&actor).await.is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first attempt durably admits the command receipt before draining");
        tokio::time::sleep_until(first_deadline + std::time::Duration::from_millis(10)).await;
        drop(lease);
        let first_error = first_attempt
            .await
            .expect("first suspension task joins")
            .expect_err("first attempt must report the elapsed owner drain deadline");
        assert!(
            first_error.contains("owners settled after the deadline"),
            "{first_error}"
        );
        let receipt_ids = received_command_ids(&actor).await;
        assert_eq!(
            receipt_ids.len(),
            1,
            "first attempt durably admits one receipt before checkpoint"
        );
        let receipt_id = receipt_ids[0].clone();
        let pending = actor
            .chat_state_handle
            .pending_notifications()
            .await
            .expect("pending notification projection is available");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            crate::session::notification_inbox::read_payload(
                &actor.session_directory,
                &pending[0].payload_ref,
            )
            .expect("suspension result payload remains readable"),
            command_suspension_result(&command).expect("command result serializes")
        );

        assert_eq!(
            actor
                .chat_state_handle
                .runtime_state()
                .await
                .unwrap()
                .checkpoint_seq,
            None,
            "expired first attempt must leave the durable notification without a checkpoint"
        );
        let retry = request_suspension(&actor).await;
        assert_eq!(received_command_ids(&actor).await, vec![receipt_id.clone()]);
        let repeated_request = request_suspension(&actor).await;
        assert_eq!(
            repeated_request, retry,
            "same operation reuses its checkpoint"
        );

        let (completion_tx, mut completion_rx) = tokio::sync::mpsc::unbounded_channel();
        actor
            .prepare_runtime_restore(OPERATION_ID, "receipt-restore", completion_tx)
            .await
            .expect("restore preparation must retain the completed task receipt");
        actor
            .commit_runtime_resume(OPERATION_ID.into(), "receipt-restore".into())
            .await
            .expect("restore must commit the original suspension operation");
        actor
            .release_runtime_restore("receipt-restore")
            .await
            .expect("restore must release the original checkpoint");
        assert!(
            completion_rx.try_recv().is_err(),
            "there is no suspended Turn"
        );
        assert_eq!(
            actor
                .chat_state_handle
                .received_notification_id(source, source_version)
                .await,
            Some(Some(receipt_id.clone())),
            "the production restore path retains the same terminal receipt identity"
        );
        assert_eq!(received_command_ids(&actor).await, vec![receipt_id]);
    });
}
