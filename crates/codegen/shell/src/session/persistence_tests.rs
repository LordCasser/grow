use super::*;
use crate::session::storage::jsonl::AppendDurability;
use chat_state::AdmittedResponse;

struct ActorGuard {
    handle: PersistenceHandle,
    task: tokio::task::JoinHandle<()>,
}

impl ActorGuard {
    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }

    async fn stop_gracefully(self) {
        self.handle.tx.send(PersistenceMsg::Stop).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), self.task)
            .await
            .unwrap()
            .unwrap();
    }

    async fn close_sender_gracefully(self) {
        drop(self.handle);
        tokio::time::timeout(std::time::Duration::from_secs(2), self.task)
            .await
            .unwrap()
            .unwrap();
    }
}

fn test_actor(info: Info, storage: Arc<dyn StorageAdapter>) -> ActorGuard {
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(
        SessionPersistence {
            info,
            storage,
            pending_notification: None,
            sampling_anchor: None,
            sampling_attempt: None,
            terminal_sampling_attempt: None,
            rx,
            gateway: None,
        }
        .run(),
    );
    ActorGuard {
        handle: PersistenceHandle {
            tx,
            noop: false,
            task: Some(task.abort_handle()),
            session_directory: None,
        },
        task,
    }
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn corrupt_workflow_repair_ack_reports_durable_write_and_stale_snapshot() {
    use crate::session::workflow::store::{
        WORKFLOW_RUN_MANIFEST_VERSION, WorkflowManifestFingerprint, WorkflowRunManifest,
    };
    use crate::session::workflow::tracker::{WorkflowRuntimeRoute, WorkflowTracker};

    let root = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("corrupt-workflow-repair"),
        cwd: "/tmp/workflow-repair".into(),
    };
    let storage =
        crate::session::storage::jsonl::JsonlStorageAdapter::with_root(root.path().to_path_buf());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .load_session_for_write_without_updates(&info)
        .await
        .unwrap();

    let run_id = "wf_repair_ack";
    let session_dir = root
        .path()
        .join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname(&info.cwd))
        .join(info.id.to_string());
    let run_dir = session_dir.join("workflows").join(run_id);
    std::fs::create_dir_all(&run_dir).unwrap();
    let state_path = run_dir.join("state.json");
    let broken = b"{broken workflow json";
    std::fs::write(&state_path, broken).unwrap();
    let state = WorkflowTracker::default().start_run(
        run_id.into(),
        "demo".into(),
        "objective".into(),
        Vec::new(),
        None,
        Some(format!("workflows/{run_id}/journal.jsonl")),
        WorkflowRuntimeRoute::for_test(
            "test-model",
            None,
            sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
        )
        .unwrap(),
    );
    let manifest = WorkflowRunManifest {
        version: WORKFLOW_RUN_MANIFEST_VERSION,
        state,
        script_revision: 0,
    };
    let actor = test_actor(info, Arc::new(storage));

    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::WorkflowRunStateAndAck {
            manifest: manifest.clone(),
            corrupt_sidecar_fingerprint: Some(WorkflowManifestFingerprint::of(broken)),
            respond_to,
        })
        .unwrap();
    response.await.unwrap().unwrap();
    assert_eq!(
        crate::session::workflow::store::decode_workflow_manifest(
            &std::fs::read(&state_path).unwrap()
        )
        .unwrap(),
        manifest
    );

    let changed_broken = b"{another broken version";
    std::fs::write(&state_path, changed_broken).unwrap();
    let mut newer = manifest.clone();
    newer.state.revision += 1;
    let newer_bytes = serde_json::to_vec(&newer).unwrap();
    std::fs::write(&state_path, &newer_bytes).unwrap();
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::WorkflowRunStateAndAck {
            manifest,
            corrupt_sidecar_fingerprint: Some(WorkflowManifestFingerprint::of(changed_broken)),
            respond_to,
        })
        .unwrap();
    let error = response.await.unwrap().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(std::fs::read(&state_path).unwrap(), newer_bytes);
    actor.stop_gracefully().await;
}

fn notification(info: &Info, text: &str) -> acp::SessionNotification {
    acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(text),
        ))),
    )
}

fn neutral_update(info: &Info, text: &str) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(notification(info, text)))
}

fn sampling_update(info: &Info, request_id: &str, attempt: u32, text: &str) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(
        notification(info, text).meta(
            serde_json::json!({
                "samplingRequestId": request_id,
                "samplingAttempt": attempt,
            })
            .as_object()
            .cloned(),
        ),
    ))
}

fn candidate_marker(request_id: &str, attempt: u32) -> PersistenceMsg {
    PersistenceMsg::SamplingCandidate {
        request_id: request_id.to_owned(),
        attempt,
        event_id: format!("anchor-{request_id}-{attempt}"),
    }
}

#[test]
fn repeated_sampling_candidates_keep_constant_persistence_state() {
    let info = Info {
        id: acp::SessionId::new("sampling-candidate-staging-bound"),
        cwd: "/tmp/sampling-candidate-staging-bound".into(),
    };
    let (_tx, rx) = mpsc::unbounded_channel();
    let storage = Arc::new(JsonlStorageAdapter::with_root(std::path::PathBuf::from(
        "/tmp/sampling-candidate-staging-bound",
    )));
    let mut persistence = SessionPersistence {
        info: info.clone(),
        storage,
        pending_notification: None,
        sampling_anchor: None,
        sampling_attempt: None,
        terminal_sampling_attempt: None,
        rx,
        gateway: None,
    };
    let key = SamplingAttemptKey {
        request_id: "request".into(),
        attempt: 1,
    };
    for _ in 0..10_000 {
        persistence.mark_sampling_anchor(key.clone());
    }
    let window = persistence.sampling_anchor.unwrap();
    assert_eq!(window.key, key);
    assert!(window.write_error.is_none());
}

#[test]
fn ordinary_text_coalescing_stops_at_the_record_budget() {
    let mut prior =
        acp::ContentBlock::Text(acp::TextContent::new("a".repeat(MAX_MERGED_ACP_TEXT_BYTES)));
    let incoming = acp::ContentBlock::Text(acp::TextContent::new("b"));
    assert!(!SessionPersistence::try_merge_text(&mut prior, &incoming));
    assert!(
        matches!(prior, acp::ContentBlock::Text(text) if text.text.len() == MAX_MERGED_ACP_TEXT_BYTES)
    );
}

fn neutral_update_with_event(info: &Info, text: &str, event_id: &str) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(
        notification(info, text).meta(Some(
            serde_json::json!({ "eventId": event_id })
                .as_object()
                .cloned()
                .unwrap(),
        )),
    ))
}

fn response_projection(
    info: &Info,
    request_id: &str,
    attempt: u32,
    text: &str,
) -> crate::session::response_projection::ResponseReplayProjection {
    crate::session::response_projection::project_admitted_response(
        &info.id,
        &AdmittedResponse {
            event_seq: chat_state::EventSeq::new(7),
            identity: chat_state::ResponseAdmissionIdentity {
                request_id: request_id.into(),
                attempt,
            },
            items: vec![sampling_types::ConversationItem::assistant(text)],
            quarantined_tool_exchanges: 0,
        },
    )
    .unwrap()
}

fn physical_updates(path: &std::path::Path) -> Vec<SessionUpdate> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn projected_replay_texts(updates: &[SessionUpdate]) -> Vec<String> {
    fn notification_text(notification: &acp::SessionNotification) -> Option<String> {
        let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
            return None;
        };
        let acp::ContentBlock::Text(text) = &chunk.content else {
            return None;
        };
        Some(text.text.clone())
    }

    updates
        .iter()
        .flat_map(|update| match update {
            SessionUpdate::Acp(notification) => {
                notification_text(notification).into_iter().collect()
            }
            SessionUpdate::ResponseReplayProjection(projection) => projection
                .clone()
                .into_notifications()
                .iter()
                .filter_map(notification_text)
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

fn sampling_boundary(
    request_id: &str,
    attempt: u32,
    state: crate::extensions::notification::SamplingAttemptState,
) -> PersistenceMsg {
    PersistenceMsg::SamplingAttempt {
        request_id: request_id.to_owned(),
        attempt,
        state,
    }
}

fn replay_texts(root: &std::path::Path, session_id: &str) -> Vec<String> {
    crate::session::storage::load_updates_for_replay_at(session_id, root)
        .unwrap()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|update| {
            let acp::SessionUpdate::AgentMessageChunk(chunk) = update else {
                return None;
            };
            let acp::ContentBlock::Text(text) = chunk.content else {
                return None;
            };
            Some(text.text)
        })
        .collect()
}

fn break_summary_writes(dir: &std::path::Path) {
    let summary = dir.join("summary.json");
    std::fs::remove_file(&summary).unwrap();
    std::fs::create_dir(summary).unwrap();
}

#[test]
fn retryable_projection_error_classifies_io_kinds() {
    use crate::session::storage::AppendUpdateError;

    let invalid_data = AppendUpdateError::NotCommitted(io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid projection",
    ));
    let unsupported =
        AppendUpdateError::Committed(io::Error::new(io::ErrorKind::Unsupported, "unsupported"));
    let ordinary = AppendUpdateError::NotCommitted(io::Error::other("temporary failure"));

    assert!(!SessionPersistence::retryable_projection_error(
        &invalid_data
    ));
    assert!(!SessionPersistence::retryable_projection_error(
        &unsupported
    ));
    assert!(SessionPersistence::retryable_projection_error(&ordinary));
}

#[test]
fn committed_error_returns_sync_disposition() {
    let info = Info {
        id: acp::SessionId::new("committed-update"),
        cwd: "/test".into(),
    };
    let notification = notification(&info, "committed");
    let PendingAppendOutcome::CommittedErr(sync_notification, error) =
        SessionPersistence::finish_pending_append(
            notification,
            Err(crate::session::storage::AppendUpdateError::Committed(
                io::Error::other("summary patch failed"),
            )),
        )
    else {
        panic!("expected committed failure");
    };
    assert_eq!(sync_notification.session_id, info.id);
    assert_eq!(error.to_string(), "summary patch failed");
}

#[test]
fn uncommitted_error_returns_restore_disposition() {
    let info = Info {
        id: acp::SessionId::new("uncommitted-update"),
        cwd: "/test".into(),
    };
    let notification = notification(&info, "pending");
    let PendingAppendOutcome::NotCommittedErr(pending_notification, error) =
        SessionPersistence::finish_pending_append(
            notification,
            Err(crate::session::storage::AppendUpdateError::NotCommitted(
                io::Error::other("append failed"),
            )),
        )
    else {
        panic!("expected uncommitted failure");
    };
    assert_eq!(pending_notification.session_id, info.id);
    assert_eq!(error.to_string(), "append failed");
}

#[tokio::test]
async fn noop_handle_rejects_durable_append() {
    let info = Info {
        id: acp::SessionId::new("noop-durable-update"),
        cwd: "/test".into(),
    };
    assert!(matches!(
        PersistenceHandle::noop()
            .append_update_durably(neutral_update(&info, "durable"))
            .await,
        Err(DurableAppendError::NotCommitted(error))
            if error.kind() == io::ErrorKind::Unsupported
    ));
}

#[tokio::test]
async fn failed_pending_drain_retains_record_and_skips_durable_update() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("durable-drain-failure"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = attempts.clone();
    let storage = JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("durable-drain-failure"),
        move |durability| {
            observed.lock().unwrap().push(durability);
            Err(io::Error::other("pending append failed"))
        },
    );
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), Arc::new(storage));
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "pending")))
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            actor
                .handle
                .append_update_durably(neutral_update(&info, "durable"))
                .await
                .unwrap_err()
                .to_string(),
            "pending append failed"
        );
    }
    assert!(matches!(
        attempts.lock().unwrap().as_slice(),
        [AppendDurability::Buffered, AppendDurability::Buffered]
    ));
    actor.stop().await;
}

#[tokio::test]
async fn durable_append_drains_pending_update_in_fifo_order() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("durable-update"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage.clone());
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "before")))
        .unwrap();
    actor
        .handle
        .append_update_durably(neutral_update(&info, "durable"))
        .await
        .unwrap();
    let summary = storage.load_summary(&info).await.unwrap();
    assert_eq!(summary.num_messages, 2);

    let updates = storage.load_session(&info).await.unwrap().updates;
    let texts = updates
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Acp(notification) = update else {
                return None;
            };
            let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                return None;
            };
            let acp::ContentBlock::Text(text) = &chunk.content else {
                return None;
            };
            Some(text.text.clone())
        })
        .collect::<Vec<_>>();
    assert_eq!(texts, ["before", "durable"]);
    actor.stop().await;
}

#[tokio::test]
async fn accepted_lifecycle_without_projection_discards_all_candidates() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-replay-retry"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Discarded,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            2,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 2))
        .unwrap();
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            2,
            SamplingAttemptState::Accepted,
        ))
        .unwrap();
    actor.stop_gracefully().await;

    assert_eq!(
        replay_texts(dir.path(), info.id.0.as_ref()),
        Vec::<String>::new()
    );
}

async fn commit_projection_request(
    actor: &ActorGuard,
    projection: crate::session::response_projection::ResponseReplayProjection,
) -> Result<(), crate::session::storage::AppendUpdateError> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::CommitResponseProjection {
            projection,
            respond_to,
        })
        .unwrap();
    response.await.unwrap()
}

async fn sampling_barrier(actor: &ActorGuard, request_id: &str, attempt: u32) -> io::Result<()> {
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::SamplingBarrier {
            request_id: request_id.to_owned(),
            attempt,
            respond_to,
        })
        .unwrap();
    response.await.unwrap()
}

async fn stage_projection_window(actor: &ActorGuard, info: &Info, request_id: &str) {
    use crate::extensions::notification::SamplingAttemptState;
    actor
        .handle
        .tx
        .send(sampling_boundary(
            request_id,
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update_with_event(
            info,
            "pre",
            "event-pre",
        )))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker(request_id, 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker(request_id, 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update_with_event(
            info,
            "post",
            "event-post",
        )))
        .unwrap();
}

#[tokio::test]
async fn projected_sampling_window_replaces_first_candidate_anchor() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-projection-anchor"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage.clone());
    stage_projection_window(&actor, &info, "request").await;
    sampling_barrier(&actor, "request", 1).await.unwrap();
    commit_projection_request(
        &actor,
        response_projection(&info, "request", 1, "canonical"),
    )
    .await
    .unwrap();
    actor.stop_gracefully().await;

    let updates_path = storage
        .open_session(&info)
        .unwrap()
        .directory()
        .display_path()
        .join("updates.jsonl");
    let updates = physical_updates(&updates_path);
    assert!(
        matches!(&updates[0], SessionUpdate::Acp(notification) if notification.meta.as_ref().and_then(|m| m.get("eventId")).and_then(serde_json::Value::as_str) == Some("event-pre"))
    );
    assert!(
        matches!(&updates[1], SessionUpdate::Acp(notification) if notification.meta.as_ref().is_some_and(|meta| meta.contains_key("samplingRequestId")))
    );
    assert!(
        matches!(&updates[2], SessionUpdate::Acp(notification) if notification.meta.as_ref().and_then(|m| m.get("eventId")).and_then(serde_json::Value::as_str) == Some("event-post"))
    );
    assert!(matches!(
        &updates[3],
        SessionUpdate::ResponseReplayProjection(_)
    ));
    assert_eq!(updates.len(), 4);
    assert_eq!(storage.load_summary(&info).await.unwrap().num_messages, 3);
    assert_eq!(
        projected_replay_texts(&updates),
        ["pre", "", "post", "canonical"]
    );
}

#[tokio::test]
async fn projected_sampling_window_retries_projection_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-projection-retry"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let storage = JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("sampling-projection-retry"),
        move |_| {
            if observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 3 {
                Err(io::Error::other("projection append failed"))
            } else {
                Ok(())
            }
        },
    );
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let storage = Arc::new(storage);
    let actor = test_actor(info.clone(), storage.clone());
    stage_projection_window(&actor, &info, "request").await;
    sampling_barrier(&actor, "request", 1).await.unwrap();
    commit_projection_request(
        &actor,
        response_projection(&info, "request", 1, "canonical"),
    )
    .await
    .unwrap();
    actor.stop_gracefully().await;
    let updates = physical_updates(
        &dir.path()
            .join("sampling-projection-retry")
            .join("updates.jsonl"),
    );
    assert_eq!(updates.len(), 4);
    assert!(matches!(
        updates[3],
        SessionUpdate::ResponseReplayProjection(_)
    ));
}

#[tokio::test]
async fn failed_independent_update_blocks_sampling_admission() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-post-retry"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let storage = JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("sampling-post-retry"),
        move |_| {
            if observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 2 {
                Err(io::Error::other("post append failed"))
            } else {
                Ok(())
            }
        },
    );
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let storage = Arc::new(storage);
    let actor = test_actor(info.clone(), storage.clone());
    stage_projection_window(&actor, &info, "request").await;
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
    let updates = physical_updates(&dir.path().join("sampling-post-retry").join("updates.jsonl"));
    assert_eq!(updates.len(), 2);
    assert_eq!(projected_replay_texts(&updates), ["pre", ""]);
}

#[tokio::test]
async fn failed_preview_gateway_reservation_blocks_sampling_admission() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-gateway-overflow"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info, storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::SamplingPreviewFailure {
            request_id: "request".into(),
            attempt: 1,
            reason: "gateway budget exhausted".into(),
        })
        .unwrap();
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn failed_exact_independent_append_blocks_sampling_admission() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-independent-failure"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("sampling-independent-failure"),
        |_| Err(io::Error::other("independent append failed")),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    let (respond_to, response) = tokio::sync::oneshot::channel();
    let notification = neutral_update_with_event(&info, "independent", "event-independent");
    let SessionUpdate::Acp(notification) = notification else {
        panic!("expected ACP notification");
    };
    actor
        .handle
        .tx
        .send(PersistenceMsg::PreviewIndependent {
            request_id: "request".into(),
            attempt: 1,
            update: SessionUpdate::Acp(notification),
            respond_to,
        })
        .unwrap();
    assert!(response.await.unwrap().is_err());
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn failed_auxiliary_grow_append_blocks_sampling_admission() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-auxiliary-failure"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("sampling-auxiliary-failure"),
        |_| Err(io::Error::other("auxiliary append failed")),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    let permit = sampler::PreviewEventBudget::default()
        .try_acquire_bytes(1)
        .unwrap();
    let (respond_to, acknowledgement) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::AuxiliaryGrow {
            notification: crate::extensions::notification::SessionNotification {
                session_id: info.id.clone(),
                update: crate::extensions::notification::SessionUpdate::MemoryFlushStarted,
                meta: None,
            },
            permit,
            preview: Some(("request".into(), 1)),
            respond_to: Some(respond_to),
        })
        .unwrap();
    assert!(acknowledgement.await.unwrap().is_err());
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn auxiliary_reservation_failure_blocks_sampling_admission() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-auxiliary-budget"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info, storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::AuxiliaryPreviewFailure {
            request_id: "request".into(),
            attempt: 1,
            reason: "preview credits exhausted".into(),
        })
        .unwrap();
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn stale_auxiliary_failure_cannot_poison_another_attempt() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-auxiliary-stale"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info, storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "current",
            2,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::AuxiliaryPreviewFailure {
            request_id: "previous".into(),
            attempt: 1,
            reason: "late budget failure".into(),
        })
        .unwrap();
    sampling_barrier(&actor, "current", 2).await.unwrap();
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn independent_grow_append_drains_earlier_buffered_acp_update() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("independent-grow-order"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage.clone());
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "before")))
        .unwrap();
    let (respond_to, response) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::PreviewIndependent {
            request_id: "request".into(),
            attempt: 1,
            update: SessionUpdate::Grow(Box::new(
                crate::extensions::notification::SessionNotification {
                    session_id: info.id.clone(),
                    update: crate::extensions::notification::SessionUpdate::MemoryFlushStarted,
                    meta: None,
                },
            )),
            respond_to,
        })
        .unwrap();
    response.await.unwrap().unwrap();
    actor.stop_gracefully().await;
    let updates = storage.load_session(&info).await.unwrap().updates;
    assert!(matches!(updates[0], SessionUpdate::Acp(_)));
    assert!(matches!(updates[1], SessionUpdate::Grow(_)));
    assert_eq!(projected_replay_texts(&updates), ["before"]);
}

#[tokio::test]
async fn failed_anchor_blocks_sampling_admission_without_preview_body() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-anchor-failure"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("sampling-anchor-failure"),
        |_| Err(io::Error::other("anchor append failed")),
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
    assert_eq!(
        replay_texts(dir.path(), info.id.0.as_ref()),
        Vec::<String>::new()
    );
}

#[tokio::test]
async fn failed_pre_anchor_drain_remains_a_sticky_admission_error() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-pre-anchor-failure"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let storage = Arc::new(JsonlStorageAdapter::with_update_append_probe(
        dir.path().join("sampling-pre-anchor-failure"),
        move |_| {
            if observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Err(io::Error::other("pre-anchor append failed"))
            } else {
                Ok(())
            }
        },
    ));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update_with_event(
            &info,
            "pre",
            "event-pre",
        )))
        .unwrap();
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn superseded_attempt_cannot_pass_the_preview_barrier() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-stale-barrier"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info, storage);
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            2,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    assert!(sampling_barrier(&actor, "request", 1).await.is_err());
    assert!(sampling_barrier(&actor, "request", 2).await.is_ok());
    actor.stop_gracefully().await;
}

#[tokio::test]
async fn failed_projection_commit_keeps_anchor_state_for_idempotent_retry() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-projection-restore"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let fail_projection_writes = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let fail_writes = fail_projection_writes.clone();
    let session_dir = dir.path().join("sampling-projection-restore");
    let storage = JsonlStorageAdapter::with_update_append_probe(session_dir.clone(), move |_| {
        let call = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if fail_writes.load(std::sync::atomic::Ordering::SeqCst) && matches!(call, 0 | 1) {
            Err(io::Error::other("projection append failed"))
        } else {
            Ok(())
        }
    });
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();

    let (_tx, rx) = mpsc::unbounded_channel();
    let mut persistence = SessionPersistence {
        info: info.clone(),
        storage: Arc::new(storage),
        pending_notification: None,
        sampling_anchor: None,
        sampling_attempt: None,
        terminal_sampling_attempt: None,
        rx,
        gateway: None,
    };
    let key = SamplingAttemptKey {
        request_id: "request".into(),
        attempt: 1,
    };
    persistence.mark_sampling_anchor(key);

    let projection = response_projection(&info, "request", 1, "canonical");
    assert!(
        persistence
            .commit_response_projection(projection.clone())
            .await
            .is_err()
    );
    let restored = persistence.sampling_anchor.as_ref().unwrap();
    assert_eq!(restored.key.request_id, "request");
    assert!(restored.write_error.is_none());

    fail_projection_writes.store(false, std::sync::atomic::Ordering::SeqCst);
    persistence
        .commit_response_projection(projection)
        .await
        .unwrap();
    assert!(persistence.sampling_anchor.is_none());
    assert!(persistence.terminal_sampling_attempt.is_some());

    let updates = physical_updates(&session_dir.join("updates.jsonl"));
    assert_eq!(updates.len(), 1);
    assert!(matches!(
        updates[0],
        SessionUpdate::ResponseReplayProjection(_)
    ));
    assert_eq!(projected_replay_texts(&updates), ["canonical"]);
}

#[tokio::test]
async fn unaccepted_sampling_candidate_is_absent_after_shutdown() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-replay-shutdown"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(sampling_update(
            &info,
            "request",
            1,
            "candidate",
        )))
        .unwrap();
    actor.stop_gracefully().await;

    assert!(replay_texts(dir.path(), info.id.0.as_ref()).is_empty());
}

#[tokio::test]
async fn untagged_interleaving_survives_sampling_discard() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-replay-interleave"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "untagged")))
        .unwrap();
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Discarded,
        ))
        .unwrap();
    actor.stop_gracefully().await;

    assert_eq!(replay_texts(dir.path(), info.id.0.as_ref()), ["untagged"]);
}

#[tokio::test]
async fn untagged_interleaving_survives_channel_close() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-replay-close"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);

    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "untagged")))
        .unwrap();
    actor.close_sender_gracefully().await;

    assert_eq!(replay_texts(dir.path(), info.id.0.as_ref()), ["untagged"]);
}

#[tokio::test]
async fn accepted_lifecycle_without_projection_preserves_untagged_event_order() {
    use crate::extensions::notification::SamplingAttemptState;

    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("sampling-replay-order"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage.clone());

    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Started,
        ))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update_with_event(
            &info, "untagged", "event-2",
        )))
        .unwrap();
    actor
        .handle
        .tx
        .send(candidate_marker("request", 1))
        .unwrap();
    actor
        .handle
        .tx
        .send(sampling_boundary(
            "request",
            1,
            SamplingAttemptState::Accepted,
        ))
        .unwrap();
    actor.stop_gracefully().await;

    let loaded = storage.load_session(&info).await.unwrap();
    let event_ids = loaded
        .updates
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Acp(notification) = update else {
                return None;
            };
            notification
                .meta
                .as_ref()
                .and_then(|meta| meta.get("eventId"))
                .and_then(serde_json::Value::as_str)
        })
        .collect::<Vec<_>>();
    assert_eq!(event_ids, ["event-2"]);
    assert_eq!(replay_texts(dir.path(), info.id.0.as_ref()), ["untagged"]);
}

/// A `CurrentModel` mutation with omitted metadata replaces only the canonical
/// catalog ID and preserves the persisted effort and agent name.
#[tokio::test]
async fn current_model_write_preserves_omitted_metadata() {
    use sampling_types::ReasoningEffort;
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("catalog-id-write"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().to_path_buf()));
    let previous = crate::agent::models::ModelId::new("deepseek/deepseek-v4-flash");
    let replacement = crate::agent::models::ModelId::new("anthropic/claude-sonnet");
    storage.init_session(&info, previous.clone()).await.unwrap();
    storage
        .update_current_model_and_agent(
            &info,
            &previous,
            Some("grow-build"),
            Some(Some(ReasoningEffort::High)),
        )
        .await
        .unwrap();
    assert_eq!(
        storage.load_summary(&info).await.unwrap().current_model_id,
        previous,
        "precondition: summary holds the previous catalog ID"
    );

    let actor = test_actor(info.clone(), storage.clone());
    actor
        .handle
        .tx
        .send(PersistenceMsg::CurrentModel {
            model_id: replacement.clone(),
            agent_name: None,
            reasoning_effort: None,
        })
        .unwrap();
    // Ordering barrier: the actor processes messages in order, so once this
    // ack returns the CurrentModel patch above has been applied.
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
    actor
        .handle
        .tx
        .send(PersistenceMsg::FlushAndAck { respond_to: ack_tx })
        .unwrap();
    ack_rx.await.unwrap();

    let summary = storage.load_summary(&info).await.unwrap();
    assert_eq!(summary.current_model_id, replacement);
    assert_eq!(
        summary.reasoning_effort,
        Some(ReasoningEffort::High),
        "the write-back must not clear the persisted reasoning effort"
    );
    assert_eq!(
        summary.agent_name.as_deref(),
        Some("grow-build"),
        "the write-back must not clear the persisted agent name"
    );
    actor.stop().await;
}

#[tokio::test]
async fn stop_drains_accepted_updates_with_retained_sender() {
    let dir = tempfile::tempdir().unwrap();
    let info = Info {
        id: acp::SessionId::new("stop-drain"),
        cwd: dir.path().to_string_lossy().into_owned(),
    };
    let storage = Arc::new(JsonlStorageAdapter::with_root(dir.path().into()));
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let actor = test_actor(info.clone(), storage);
    let retained = actor.handle.clone();
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(&info, "before stop")))
        .unwrap();
    actor.handle.tx.send(PersistenceMsg::Stop).unwrap();
    // Accepted before the current-thread runtime polls the receiver closure.
    actor
        .handle
        .tx
        .send(PersistenceMsg::Update(neutral_update(
            &info,
            "already queued",
        )))
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), actor.task)
        .await
        .unwrap()
        .unwrap();
    assert!(retained.tx.send(PersistenceMsg::Flush).is_err());
    assert!(retained.task_completion().unwrap().is_finished());
    let replacement = JsonlStorageAdapter::with_root(dir.path().into());
    let loaded = replacement
        .load_session_for_write_without_updates(&info)
        .await
        .unwrap();
    assert_eq!(loaded.summary.num_messages, 1); // Adjacent text chunks merge.
    let updates = replacement.load_session(&info).await.unwrap().updates;
    let texts: Vec<_> = updates
        .iter()
        .filter_map(|update| {
            let SessionUpdate::Acp(notification) = update else {
                return None;
            };
            let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                return None;
            };
            let acp::ContentBlock::Text(text) = &chunk.content else {
                return None;
            };
            Some(text.text.as_str())
        })
        .collect();
    assert_eq!(texts, ["before stopalready queued"]);
}
