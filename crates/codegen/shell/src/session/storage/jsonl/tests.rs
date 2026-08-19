#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;
use std::collections::BTreeMap;
use crate::session::info::Info;
use crate::session::persistence::{SessionLineage, Summary, default_model_id};
use crate::session::storage::{CopySessionOptions, SessionUpdate};
use agent_client_protocol as acp;
use tempfile::TempDir;
fn create_test_info() -> Info {
    Info {
        id: acp::SessionId::new("test-session-123"),
        cwd: "/test/workspace".to_string(),
    }
}
fn create_test_chat_messages() -> Vec<ConversationItem> {
    vec![
            ConversationItem::user("Hello world"),
            ConversationItem::user("How are you?"),
            ConversationItem::user("Test message"),
        ]
}
async fn append_timeline_seed(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    items: Vec<ConversationItem>,
) {
    let timeline = chat_state::Timeline::from_seed(items).unwrap();
    for event in timeline.events() {
        adapter.append_timeline_event(info, event).await.unwrap();
    }
}
fn loaded_surface(events: &[chat_state::TimelineEvent]) -> Vec<ConversationItem> {
    chat_state::Timeline::from_events(events.to_vec())
        .unwrap()
        .surface()
        .to_vec()
}
async fn append_control_snapshot(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    snapshot: &crate::session::control::SessionControlSnapshot,
) {
    let events = adapter.read_timeline_events_sync(info).unwrap();
    let mut timeline = chat_state::Timeline::from_events(events).unwrap();
    let event = timeline.record(snapshot.timeline_kind().unwrap()).unwrap();
    adapter.append_timeline_event(info, &event).await.unwrap();
}
async fn append_completed_prompt_turn(
    adapter: &JsonlStorageAdapter,
    info: &Info,
    id: u64,
    prompt_index: usize,
    prompt: &str,
) {
    let events = adapter.read_timeline_events_sync(info).unwrap();
    let mut timeline = chat_state::Timeline::from_events(events).unwrap();
    let turn = chat_state::TurnId(id);
    let started = timeline
        .record(chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Started {
            id: turn,
            identity: chat_state::TurnIdentity {
                origin: "user".into(),
                turn_kind: "interactive".into(),
                goal_id: None,
                stage_id: None,
            },
            model_id: "model".into(),
            input_message_count: timeline.surface().len(),
            prompt_index,
            prompt_text: prompt.into(),
            input_kind: chat_state::TurnInputKind::Prompt,
            redirect_kind: None,
        }))
        .unwrap();
    adapter.append_timeline_event(info, &started).await.unwrap();
    let mut user = ConversationItem::user(prompt);
    user.set_prompt_index(prompt_index);
    let message = timeline.append(user, chat_state::MessageCause::User).unwrap();
    adapter.append_timeline_event(info, &message).await.unwrap();
    let ended = timeline
        .record(chat_state::TimelineEventKind::Turn(chat_state::TurnEvent::Ended {
            id: turn,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 0,
            terminal: chat_state::TurnTerminal {
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }))
        .unwrap();
    adapter.append_timeline_event(info, &ended).await.unwrap();
}
fn create_test_notification() -> acp::SessionNotification {
    acp::SessionNotification::new(
        acp::SessionId::new("test-session-123"),
        acp::SessionUpdate::AgentMessageChunk(
            acp::ContentChunk::new(
                acp::ContentBlock::Text(
                    acp::TextContent::new("Test response".to_string()),
                ),
            ),
        ),
    )
}
#[tokio::test]
async fn prepared_session_is_published_once_with_lineage_and_timeline() {
    let temp_dir = TempDir::new().unwrap();
    let target = temp_dir.path().join("child-session");
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(target.clone());
    let info = create_test_info();
    let lineage = SessionLineage {
        session_kind: "subagent_fork".into(),
        context_source: "forked".into(),
        parent_session_id: "parent-session".into(),
        parent_prompt_id: Some("prompt-7".into()),
        subagent_seed: chat_state::SubagentSeedEvent {
            parent_timeline_id: "parent-session".into(),
            parent_spawn_seq: 1,
            subagent_id: "child-session".into(),
            context_source: chat_state::SubagentContextSource::Forked,
            source_ref: None,
            normalized: false,
        },
    };
    let mut summary = Summary::new(&info, default_model_id()).unwrap();
    lineage.apply_to(&mut summary);

    let published = adapter
        .init_session_with_summary(
            &info,
            summary,
            vec![ConversationItem::user("inherited")],
            Default::default(),
            vec![chat_state::TimelineEventKind::SubagentSeed(
                lineage.subagent_seed.clone(),
            )],
        )
        .await
        .unwrap();
    assert_eq!(published.0.session_kind.as_deref(), Some("subagent_fork"));
    assert_eq!(published.0.fork_context_source.as_deref(), Some("forked"));
    assert_eq!(
        published.0.parent_session_id.as_deref(),
        Some("parent-session")
    );
    assert_eq!(published.1.len(), 2);
    assert!(matches!(
        published.1[1].kind,
        chat_state::TimelineEventKind::SubagentSeed(_)
    ));
    assert!(target.join(super::super::TIMELINE_FILE).is_file());
    assert!(target.join(super::super::SUMMARY_FILE).is_file());
    assert!(!std::fs::read(target.join(super::super::TIMELINE_FILE)).unwrap().is_empty());
    assert!(
        std::fs::read_dir(temp_dir.path())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".staging"))
    );

    let duplicate = Summary::new(&info, default_model_id()).unwrap();
    let error = adapter
        .init_session_with_summary(
            &info,
            duplicate,
            Vec::new(),
            Default::default(),
            Vec::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(
        adapter
            .read_summary_sync(&info)
            .unwrap()
            .parent_session_id
            .as_deref(),
        Some("parent-session")
    );
}

#[tokio::test]
async fn prepared_session_atomically_publishes_the_exact_prompt_blob_set() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let content = b"inherited oversized prompt".to_vec();
    let hash = blake3::hash(&content).to_hex().to_string();
    let reference = format!(
        "{}{}",
        crate::session::persistence::PROMPT_BLOB_REF_PREFIX,
        hash
    );
    let surface = vec![ConversationItem::user(format!("read\n{reference}\nthen continue"))];
    let blobs = std::collections::BTreeMap::from([(hash.clone(), content.clone())]);
    let target = temp_dir.path().join("complete-child");
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(target.clone());

    adapter
        .init_session_with_summary(
            &info,
            Summary::new(&info, default_model_id()).unwrap(),
            surface.clone(),
            blobs,
            Vec::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(target.join("prompts").join(format!("{hash}.txt"))).unwrap(),
        content
    );

    let missing_target = temp_dir.path().join("missing-child");
    let missing_adapter =
        JsonlStorageAdapter::with_explicit_session_dir(missing_target.clone());
    let error = missing_adapter
        .init_session_with_summary(
            &info,
            Summary::new(&info, default_model_id()).unwrap(),
            surface,
            Default::default(),
            Vec::new(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(!missing_target.exists());
    assert!(
        std::fs::read_dir(temp_dir.path())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".staging"))
    );
}

#[tokio::test]
async fn session_load_rejects_missing_or_corrupt_prompt_blobs() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let content = b"canonical prompt blob";
    let hash = blake3::hash(content).to_hex().to_string();
    let reference = format!(
        "{}{}",
        crate::session::persistence::PROMPT_BLOB_REF_PREFIX,
        hash
    );
    let path = adapter
        .session_dir(&info)
        .join("prompts")
        .join(format!("{hash}.txt"));
    crate::session::persistence::write_immutable_blob(&path, content).unwrap();
    append_timeline_seed(
        &adapter,
        &info,
        vec![ConversationItem::user(format!("read\n{reference}\nthen continue"))],
    )
    .await;
    adapter.load_session_without_updates(&info).await.unwrap();

    std::fs::remove_file(&path).unwrap();
    let missing = adapter
        .load_session_without_updates(&info)
        .await
        .unwrap_err();
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);

    std::fs::write(&path, b"tampered").unwrap();
    let corrupt = adapter.load_session(&info).await.unwrap_err();
    assert_eq!(corrupt.kind(), std::io::ErrorKind::InvalidData);
}
#[tokio::test]
async fn update_current_model_persists_leaves_and_clears_reasoning_effort() {
    use sampling_types::ReasoningEffort;
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    let model = default_model_id();
    adapter.init_session(&info, model.clone()).await.unwrap();
    adapter
        .update_current_model_and_agent(
            &info,
            &model,
            None,
            Some(Some(ReasoningEffort::High)),
        )
        .await
        .unwrap();
    assert_eq!(
            adapter.read_summary_sync(&info).unwrap().reasoning_effort,
            Some(ReasoningEffort::High),
        );
    adapter
        .update_current_model_and_agent(&info, &model, None, None)
        .await
        .unwrap();
    assert_eq!(
            adapter.read_summary_sync(&info).unwrap().reasoning_effort,
            Some(ReasoningEffort::High),
            "model-only update must not wipe the persisted effort",
        );
    adapter
        .update_current_model_and_agent(&info, &model, None, Some(None))
        .await
        .unwrap();
    assert_eq!(
            adapter.read_summary_sync(&info).unwrap().reasoning_effort,
            None,
        );
}
#[tokio::test]
async fn test_jsonl_round_trip() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    let summary = adapter.init_session(&info, default_model_id()).await.unwrap();
    assert_eq!(summary.info.id, info.id);
    assert_eq!(summary.current_model_id, default_model_id());
    let messages = create_test_chat_messages();
    append_timeline_seed(&adapter, &info, messages.clone()).await;
    let notification = create_test_notification();
    adapter
        .append_update(&info, &SessionUpdate::Acp(Box::new(notification)))
        .await
        .unwrap();
    let new_model = acp::ModelId::new("grow-4.3");
    adapter
        .update_current_model_and_agent(&info, &new_model, None, None)
        .await
        .unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(loaded.summary.info.id, info.id);
    assert_eq!(loaded.summary.current_model_id, new_model);
    assert_eq!(loaded_surface(&loaded.timeline_events).len(), messages.len());
    assert_eq!(loaded.updates.len(), 1);
}

#[tokio::test]
async fn timeline_round_trip_folds_the_current_surface() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();

    let original = vec![
        ConversationItem::user("old question"),
        ConversationItem::assistant("old answer"),
    ];
    let mut timeline = chat_state::Timeline::from_seed(original).unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Started {
                id: "test-compaction".into(),
                source_items: 2,
                prompt_index: 0,
            },
        ))
        .unwrap();
    let input_ref = chat_state::TimelineRangeRef {
        timeline_id: info.id.to_string(),
        first_seq: 0,
        last_seq: timeline.next_seq().get() - 1,
    };
    let sideband_id = uuid::Uuid::now_v7().to_string();
    let target = chat_state::SurfaceRange {
        start: *timeline.surface_ids().first().unwrap(),
        end: *timeline.surface_ids().last().unwrap(),
        shadowed: timeline.surface_ids().to_vec(),
    };
    let spawn = timeline
        .record(chat_state::TimelineEventKind::Sideband(
            chat_state::SidebandSpawnEvent {
                sideband_id: sideband_id.clone(),
                purpose: chat_state::SidebandPurpose::CompactionSummary,
                source_refs: vec![input_ref.clone()],
            },
        ))
        .unwrap();
    let mut sideband = chat_state::SidebandTimeline::new(sideband_id.clone()).unwrap();
    for kind in [
        chat_state::SidebandEventKind::Request(chat_state::SidebandRequest {
            purpose: chat_state::SidebandPurpose::CompactionSummary,
            prompt: "summarize".into(),
            source_refs: vec![input_ref.clone()],
            route: chat_state::SidebandRoute {
                model: "test-model".into(),
                backend: "responses".into(),
            },
            initiator_ref: format!("t:{}/{}", info.id, spawn.seq.get()),
            executor: "main".into(),
            output_schema: None,
        }),
        chat_state::SidebandEventKind::Attempt(chat_state::SidebandAttempt {
            attempt_no: 1,
            input_refs: vec![input_ref.clone()],
            assembly_manifest: chat_state::SidebandAssemblyManifest {
                strategy: "all-sources".into(),
                strategy_version: 1,
                source_revision: Some(1),
                context_surface_ids: Vec::new(),
                selected_surface_ids: Vec::new(),
                materialized_input_tokens: 1,
                max_output_tokens: Some(1),
            },
            feedback: None,
        }),
        chat_state::SidebandEventKind::Result(chat_state::SidebandResult {
            raw_output: "compacted surface".into(),
            structured_output: None,
            usage: chat_state::SidebandUsage::default(),
            finish: "stop".into(),
            source_event_seqs: [0, 1],
            evidence_refs: Vec::new(),
        }),
        chat_state::SidebandEventKind::End(chat_state::SidebandEnd {
            outcome: chat_state::SidebandOutcome::Completed,
            error: None,
        }),
    ] {
        let event = sideband.prepare(kind).unwrap();
        sideband.accept(event).unwrap();
    }
    timeline
        .record(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Summary {
                id: "test-compaction".into(),
                input_ref,
                result_ref: chat_state::TimelineRangeRef {
                    timeline_id: sideband_id.clone(),
                    first_seq: 2,
                    last_seq: 2,
                },
                target: target.clone(),
                source_tokens: 100,
                summary_chars: 17,
            },
        ))
        .unwrap();
    timeline
        .replace_compaction_range(target, vec![ConversationItem::user("compacted surface")])
        .unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Compaction(
            chat_state::CompactionEvent::Completed {
                id: "test-compaction".into(),
                source_items: 2,
                result_items: 1,
                duration_ms: 1,
            },
        ))
        .unwrap();
    for event in timeline.events() {
        adapter.append_timeline_event(&info, event).await.unwrap();
    }
    for event in sideband.events() {
        adapter
            .append_sideband_event_durable(&info, event)
            .await
            .unwrap();
    }

    let loaded = adapter.load_session_without_updates(&info).await.unwrap();
    let replayed = chat_state::Timeline::from_events(loaded.timeline_events).unwrap();
    assert_eq!(replayed.surface().len(), 1);
    assert_eq!(replayed.surface()[0].text_content(), "compacted surface");
    assert_eq!(replayed.branch_transcript().len(), 2);
    let ledgers = adapter.read_sideband_ledgers_sync(&info, &replayed).unwrap();
    assert!(crate::session::storage::validate_sideband_ledgers(
        &info.id.to_string(),
        &replayed,
        &BTreeMap::new(),
    )
    .is_err());
    let mut tampered = ledgers;
    let result = tampered
        .get_mut(&sideband_id)
        .unwrap()
        .iter_mut()
        .find_map(|event| match &mut event.kind {
            chat_state::SidebandEventKind::Result(result) => Some(result),
            _ => None,
        })
        .unwrap();
    result.raw_output.push('!');
    assert!(crate::session::storage::validate_sideband_ledgers(
        &info.id.to_string(),
        &replayed,
        &tampered,
    )
    .is_err());
}
/// UI updates cannot synthesize conversation facts. Without Timeline events,
/// the restart Surface is empty even when updates contain transcript-looking
/// chunks.
#[tokio::test]
async fn updates_do_not_rebuild_timeline_surface() {
    use agent_client_protocol::{
        ContentBlock, ContentChunk, SessionUpdate as Acp, TextContent,
    };
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let text = |s: &str| ContentChunk::new(
        ContentBlock::Text(TextContent::new(s.to_string())),
    );
    let notify = |u| SessionUpdate::Acp(
        Box::new(acp::SessionNotification::new(info.id.clone(), u)),
    );
    adapter
        .append_update(&info, &notify(Acp::UserMessageChunk(text("ping"))))
        .await
        .unwrap();
    adapter
        .append_update(&info, &notify(Acp::AgentMessageChunk(text("pong"))))
        .await
        .unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert!(loaded.timeline_events.is_empty());
}
#[tokio::test]
async fn workflow_run_manifest_round_trips_and_clear_tombstone_wins() {
    use crate::session::workflow::store::{
        script_revision_path, WorkflowRunManifest, WORKFLOW_RUN_MANIFEST_VERSION,
    };
    use crate::session::workflow::tracker::WorkflowTracker;
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let mut tracker = WorkflowTracker::default();
    let state = tracker
        .start_run(
            "wf_restore".into(),
            "demo".into(),
            "ship".into(),
            Vec::new(),
            None,
            Some("workflows/wf_restore/journal.jsonl".into()),
        );
    let mut timeline = chat_state::Timeline::default();
    let spawn = timeline
        .record(chat_state::TimelineEventKind::Workflow(
            chat_state::WorkflowEvent::Spawned {
                run_id: "wf_restore".into(),
                execution_epoch: 0,
                name: "demo".into(),
                objective: "ship".into(),
                private: false,
            },
        ))
        .unwrap();
    adapter.append_timeline_event(&info, &spawn).await.unwrap();
    let run_dir = adapter.session_dir(&info).join("workflows/wf_restore");
    std::fs::create_dir_all(run_dir.join("scripts")).unwrap();
    std::fs::write(script_revision_path(&run_dir, 0), "complete(\"ok\");").unwrap();
    std::fs::write(run_dir.join("args.json"), r#"{"objective":"ship"}"#).unwrap();
    let manifest = WorkflowRunManifest {
        version: WORKFLOW_RUN_MANIFEST_VERSION,
        state,
        script_revision: 0,
    };
    adapter.write_workflow_run_state(&info, &manifest).await.unwrap();
    let loaded = adapter.load_session_without_updates(&info).await.unwrap();
    assert_eq!(loaded.workflow_runs.len(), 1);
    assert_eq!(loaded.workflow_runs[0].script, "complete(\"ok\");");
    assert_eq!(loaded.workflow_runs[0].args, serde_json::json!({"objective": "ship"}));
    let mut legacy = manifest.clone();
    legacy.version = 2;
    adapter.write_workflow_run_state(&info, &legacy).await.unwrap();
    let loaded_v2 = adapter.load_session_without_updates(&info).await.unwrap();
    assert!(loaded_v2.workflow_runs.is_empty());
    adapter.delete_workflow_run_state(&info, "wf_restore").await.unwrap();
    adapter.write_workflow_run_state(&info, &manifest).await.unwrap();
    assert!(run_dir.join("cleared").is_file());
    assert!(
            adapter
                .load_session_without_updates(&info)
                .await
                .unwrap()
                .workflow_runs
                .is_empty()
        );
}
#[tokio::test]
async fn workflow_restore_uses_timeline_ownership_and_caps_run_count() {
    use crate::session::workflow::store::{
        MAX_RESTORED_WORKFLOW_RUNS, WORKFLOW_RUN_MANIFEST_VERSION, WorkflowRunManifest,
        script_revision_path,
    };
    use crate::session::workflow::tracker::WorkflowTracker;
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let workflows = adapter.session_dir(&info).join("workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    let mut timeline = chat_state::Timeline::default();
    for index in 0..=MAX_RESTORED_WORKFLOW_RUNS {
        let run_id = format!("wf_{index:03}");
        let spawn = timeline
            .record(chat_state::TimelineEventKind::Workflow(
                chat_state::WorkflowEvent::Spawned {
                    run_id: run_id.clone(),
                    execution_epoch: 0,
                    name: "demo".into(),
                    objective: "ship".into(),
                    private: false,
                },
            ))
            .unwrap();
        adapter.append_timeline_event(&info, &spawn).await.unwrap();
        let run_dir = workflows.join(&run_id);
        std::fs::create_dir_all(run_dir.join("scripts")).unwrap();
        let mut tracker = WorkflowTracker::default();
        let state = tracker
            .start_run(
                run_id.clone(),
                "demo".into(),
                "ship".into(),
                Vec::new(),
                None,
                Some(format!("workflows/{run_id}/journal.jsonl")),
            );
        let manifest = WorkflowRunManifest {
            version: WORKFLOW_RUN_MANIFEST_VERSION,
            state,
            script_revision: 0,
        };
        std::fs::write(
                run_dir.join("state.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
        std::fs::write(script_revision_path(&run_dir, 0), "complete(\"ok\");").unwrap();
        std::fs::write(run_dir.join("args.json"), "{}").unwrap();
    }
    let loaded = adapter.load_session_without_updates(&info).await.unwrap();
    assert_eq!(loaded.workflow_runs.len(), MAX_RESTORED_WORKFLOW_RUNS);
    assert!(loaded.workflow_runs.iter().all(|run| {
        let index = run.manifest.state.run_id[3..].parse::<usize>().unwrap();
        index > 0
    }));
}
#[cfg(unix)]
#[tokio::test]
async fn workflow_restore_rejects_symlink_manifest() {
    use std::os::unix::fs::symlink;
    use crate::session::workflow::store::{
        WORKFLOW_RUN_MANIFEST_VERSION, WorkflowRunManifest, script_revision_path,
    };
    use crate::session::workflow::tracker::WorkflowTracker;
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let mut timeline = chat_state::Timeline::default();
    let spawn = timeline
        .record(chat_state::TimelineEventKind::Workflow(
            chat_state::WorkflowEvent::Spawned {
                run_id: "wf_symlink".into(),
                execution_epoch: 0,
                name: "demo".into(),
                objective: "ship".into(),
                private: false,
            },
        ))
        .unwrap();
    adapter.append_timeline_event(&info, &spawn).await.unwrap();
    let run_dir = adapter.session_dir(&info).join("workflows/wf_symlink");
    std::fs::create_dir_all(run_dir.join("scripts")).unwrap();
    let state = WorkflowTracker::default().start_run(
        "wf_symlink".into(),
        "demo".into(),
        "ship".into(),
        Vec::new(),
        None,
        Some("workflows/wf_symlink/journal.jsonl".into()),
    );
    let manifest = WorkflowRunManifest {
        version: WORKFLOW_RUN_MANIFEST_VERSION,
        state,
        script_revision: 0,
    };
    let outside = temp_dir.path().join("outside-state.json");
    std::fs::write(&outside, serde_json::to_vec(&manifest).unwrap()).unwrap();
    symlink(&outside, run_dir.join("state.json")).unwrap();
    std::fs::write(script_revision_path(&run_dir, 0), "complete(\"ok\");").unwrap();
    std::fs::write(run_dir.join("args.json"), "{}").unwrap();

    let loaded = adapter.load_session_without_updates(&info).await.unwrap();
    assert!(loaded.workflow_runs.is_empty());
}
/// `load_session_without_updates` always defers rewind points while the full
/// `load_session` / `load_rewind_points` still return them.
#[tokio::test]
async fn load_session_without_updates_defers_rewind_points() {
    use workspace::session::file_state::RewindPoint;
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    adapter.append_rewind_point(&info, &RewindPoint::new(0)).await.unwrap();
    adapter.append_rewind_point(&info, &RewindPoint::new(1)).await.unwrap();
    adapter.load_session_without_updates(&info).await.unwrap();
    let full = adapter.load_session(&info).await.unwrap();
    assert_eq!(full.rewind_points.len(), 2);
    assert_eq!(adapter.load_rewind_points(&info).await.unwrap().len(), 2);
    let path = adapter.rewind_points_file_path(&info).unwrap();
    assert!(path.ends_with("rewind_points.jsonl"));
}
/// The disk-authoritative ConversationOnly merge persists the correct
/// merged/truncated set.
#[tokio::test]
async fn merge_rewind_points_from_persists_merged_set() {
    use workspace::session::file_state::RewindPoint;
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    for i in 0..3 {
        adapter.append_rewind_point(&info, &RewindPoint::new(i)).await.unwrap();
    }
    adapter.merge_rewind_points_from(&info, 1).await.unwrap();
    let after = adapter.load_rewind_points(&info).await.unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].prompt_index, 0);
}
/// A malformed on-disk line makes the STRICT merge read abort BEFORE writing,
/// leaving `rewind_points.jsonl` untouched (never drop the line).
#[tokio::test]
async fn merge_rewind_points_from_aborts_on_malformed_without_writing() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let path = adapter.rewind_points_file_path(&info).unwrap();
    let original = "garbage{not json\n";
    tokio::fs::write(&path, original).await.unwrap();
    let res = adapter.merge_rewind_points_from(&info, 1).await;
    assert!(res.is_err(), "malformed read must abort the merge");
    assert_eq!(
            tokio::fs::read_to_string(&path).await.unwrap(),
            original,
            "rewind_points.jsonl must be preserved when the merge aborts"
        );
}
/// File-content `file_snapshots` must round-trip through the on-disk
/// read-modify-write merge (not just index/count).
#[tokio::test]
async fn merge_rewind_points_from_round_trips_file_snapshots() {
    use paths::RelPathBuf;
    use workspace::session::file_state::{FileSnapshot, RewindPoint};
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let mut p0 = RewindPoint::new(0);
    p0.add_snapshot(
        FileSnapshot::new(RelPathBuf::new("a.rs").unwrap(), Some("a-v0".into())),
    );
    let mut p1 = RewindPoint::new(1);
    p1.add_snapshot(
        FileSnapshot::new(RelPathBuf::new("b.rs").unwrap(), Some("b-v1".into())),
    );
    adapter.append_rewind_point(&info, &p0).await.unwrap();
    adapter.append_rewind_point(&info, &p1).await.unwrap();
    adapter.merge_rewind_points_from(&info, 1).await.unwrap();
    let after = adapter.load_rewind_points(&info).await.unwrap();
    assert_eq!(after.len(), 1);
    let m0 = &after[0];
    assert_eq!(m0.prompt_index, 0);
    assert_eq!(
            m0.get_snapshot(&RelPathBuf::new("a.rs").unwrap())
                .unwrap()
                .content,
            Some("a-v0".into())
        );
    assert_eq!(
            m0.get_snapshot(&RelPathBuf::new("b.rs").unwrap())
                .unwrap()
                .content,
            Some("b-v1".into())
        );
}
/// A `write_jsonl`-backed rewrite (here `truncate_rewind_points_from`) renames
/// the target into place and leaves NO `*.jsonl.tmp` behind.
#[tokio::test]
async fn write_jsonl_leaves_no_temp_and_renames_target() {
    use workspace::session::file_state::RewindPoint;
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    for i in 0..3 {
        adapter.append_rewind_point(&info, &RewindPoint::new(i)).await.unwrap();
    }
    adapter.truncate_rewind_points_from(&info, 2).await.unwrap();
    let kept = adapter.load_rewind_points(&info).await.unwrap();
    assert_eq!(
            kept.iter().map(|p| p.prompt_index).collect::<Vec<_>>(),
            vec![0, 1]
        );
    let path = adapter.rewind_points_file_path(&info).unwrap();
    let leftover_tmps: Vec<String> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    assert!(
            leftover_tmps.is_empty(),
            "no *.tmp should remain after write_jsonl: {leftover_tmps:?}"
        );
}
/// The resume/read paths must not mutate the on-disk `updates.jsonl` or
/// `rewind_points.jsonl`, and ACU lines stay on disk.
#[tokio::test]
async fn reads_never_modify_rewind_or_updates_files() {
    use workspace::session::file_state::{FileStateTracker, RewindPoint};
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    adapter.append_rewind_point(&info, &RewindPoint::new(0)).await.unwrap();
    adapter.append_rewind_point(&info, &RewindPoint::new(1)).await.unwrap();
    let updates_path = adapter.updates_file_path(&info).unwrap();
    let acu = r#"{"timestamp":0,"method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}"#;
    tokio::fs::write(&updates_path, format!("{acu}\n")).await.unwrap();
    let rewind_path = adapter.rewind_points_file_path(&info).unwrap();
    let rewind_before = std::fs::read(&rewind_path).unwrap();
    let updates_before = std::fs::read(&updates_path).unwrap();
    adapter.load_session_without_updates(&info).await.unwrap();
    let tracker = FileStateTracker::with_lazy_source(rewind_path.clone());
    assert_eq!(tracker.get_rewind_points().await.len(), 2);
    assert_eq!(
            std::fs::read(&rewind_path).unwrap(),
            rewind_before,
            "rewind_points.jsonl must be unchanged by reads"
        );
    assert_eq!(
            std::fs::read(&updates_path).unwrap(),
            updates_before,
            "updates.jsonl must be unchanged by reads"
        );
    let updates_str = String::from_utf8(updates_before).unwrap();
    assert!(
            updates_str.contains("available_commands_update"),
            "ACU stays persisted on disk (only skipped on forward)"
        );
}
#[tokio::test]
async fn delete_session_removes_dir_and_is_idempotent() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let dir = adapter.session_dir(&info);
    assert!(dir.exists(), "session dir should exist after init");
    adapter.delete_session(&info).await.unwrap();
    assert!(!dir.exists(), "session dir should be gone after delete");
    assert!(
            adapter.load_summary(&info).await.is_err(),
            "summary must not load after delete"
        );
    adapter.delete_session(&info).await.expect("second delete must succeed");
}
#[tokio::test]
async fn test_grow_session_update_round_trip() {
    use crate::extensions::notification::{
        DiffContent, SessionNotification as GrowSessionNotification,
        SessionUpdate as GrowSessionUpdateType,
    };
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let grow_notification = GrowSessionNotification {
        session_id: acp::SessionId::new("test-session-123"),
        update: GrowSessionUpdateType::DiffReview {
            content: vec![DiffContent {
                    diff: acp::Diff::new(
                        std::path::PathBuf::from("/test/file.rs"),
                        "new code".to_string(),
                    )
                    .old_text(Some("old code".to_string())),
                }],
        },
        meta: None,
    };
    adapter
        .append_update(&info, &SessionUpdate::Grow(Box::new(grow_notification.clone())))
        .await
        .unwrap();
    let acp_notification = create_test_notification();
    adapter
        .append_update(&info, &SessionUpdate::Acp(Box::new(acp_notification)))
        .await
        .unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(
            loaded.updates.len(),
            2,
            "Should have 2 updates (1 Grow + 1 ACP)"
        );
    match &loaded.updates[0] {
        SessionUpdate::Grow(notification) => {
            assert_eq!(notification.session_id.0.as_ref(), "test-session-123");
            match &notification.update {
                GrowSessionUpdateType::DiffReview { content } => {
                    assert_eq!(content.len(), 1);
                    assert_eq!(
                            content[0].diff.path,
                            std::path::PathBuf::from("/test/file.rs")
                        );
                }
                _ => {
                    panic!("Expected DiffReview, got different update type");
                }
            }
        }
        _ => panic!("Expected Grow update as first item"),
    }
    match &loaded.updates[1] {
        SessionUpdate::Acp(_) => {}
        _ => panic!("Expected ACP update as second item"),
    }
}
/// SubagentSpawned and SubagentFinished must survive JSONL round-trip
/// with exact field preservation.
#[tokio::test]
async fn test_subagent_notifications_round_trip() {
    use crate::extensions::notification::{
        SessionNotification as GrowSessionNotification,
        SessionUpdate as GrowSessionUpdateType,
    };
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let spawned = GrowSessionNotification {
        session_id: acp::SessionId::new("parent-session"),
        update: GrowSessionUpdateType::SubagentSpawned {
            subagent_id: "child-001".to_string(),
            parent_session_id: "parent-session".to_string(),
            parent_prompt_id: Some("turn-123".to_string()),
            child_session_id: "child-001".to_string(),
            subagent_type: "general-purpose".to_string(),
            description: "Read README.md".to_string(),
            effective_context_source: None,
            context_normalized: false,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            model: None,
            resumed_from: None,
            workflow_run_id: None,
            goal_id: None,
        },
        meta: None,
    };
    adapter.append_update(&info, &SessionUpdate::Grow(Box::new(spawned))).await.unwrap();
    let finished = GrowSessionNotification {
        session_id: acp::SessionId::new("parent-session"),
        update: GrowSessionUpdateType::SubagentFinished {
            subagent_id: "child-001".to_string(),
            child_session_id: "child-001".to_string(),
            status: "completed".to_string(),
            error: None,
            tool_calls: 5,
            turns: 2,
            duration_ms: 12345,
            tokens_used: 0,
            output: None,
        },
        meta: None,
    };
    adapter.append_update(&info, &SessionUpdate::Grow(Box::new(finished))).await.unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(loaded.updates.len(), 2);
    match &loaded.updates[0] {
        SessionUpdate::Grow(notification) => {
            match &notification.update {
                GrowSessionUpdateType::SubagentSpawned {
                    subagent_id,
                    child_session_id,
                    description,
                    subagent_type,
                    ..
                } => {
                    assert_eq!(subagent_id, "child-001");
                    assert_eq!(child_session_id, "child-001");
                    assert_eq!(description, "Read README.md");
                    assert_eq!(subagent_type, "general-purpose");
                }
                other => panic!("Expected SubagentSpawned, got {other:?}"),
            }
        }
        other => panic!("Expected Grow update, got {other:?}"),
    }
    match &loaded.updates[1] {
        SessionUpdate::Grow(notification) => {
            match &notification.update {
                GrowSessionUpdateType::SubagentFinished {
                    subagent_id,
                    status,
                    tool_calls,
                    turns,
                    duration_ms,
                    error,
                    ..
                } => {
                    assert_eq!(subagent_id, "child-001");
                    assert_eq!(status, "completed");
                    assert_eq!(*tool_calls, 5);
                    assert_eq!(*turns, 2);
                    assert_eq!(*duration_ms, 12345);
                    assert!(error.is_none());
                }
                other => panic!("Expected SubagentFinished, got {other:?}"),
            }
        }
        other => panic!("Expected Grow update, got {other:?}"),
    }
    let raw_jsonl = tokio::fs::read_to_string(
            adapter.session_dir(&info).join("updates.jsonl"),
        )
        .await
        .unwrap();
    let lines: Vec<&str> = raw_jsonl.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(
            lines.len(),
            2,
            "Expected 2 JSONL lines (spawned + finished), got {}",
            lines.len()
        );
    let spawned_json: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(spawned_json["method"], "_grow/session/update");
    let spawned_update = &spawned_json["params"]["update"];
    assert_eq!(spawned_update["sessionUpdate"], "subagent_spawned");
    assert_eq!(spawned_update["subagent_id"], "child-001");
    let finished_json: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(finished_json["method"], "_grow/session/update");
    let finished_update = &finished_json["params"]["update"];
    assert_eq!(finished_update["sessionUpdate"], "subagent_finished");
    assert_eq!(finished_update["tool_calls"], 5);
    assert_eq!(finished_update["duration_ms"], 12345);
}
#[tokio::test]
async fn test_subagent_spawned_resumed_roundtrip() {
    use crate::extensions::notification::{
        SessionNotification as GrowSessionNotification,
        SessionUpdate as GrowSessionUpdateType,
    };
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let spawned = GrowSessionNotification {
        session_id: acp::SessionId::new("resume-parent"),
        update: GrowSessionUpdateType::SubagentSpawned {
            subagent_id: "child-resumed".to_string(),
            parent_session_id: "resume-parent".to_string(),
            parent_prompt_id: Some("turn-5".to_string()),
            child_session_id: "child-resumed".to_string(),
            subagent_type: "general-purpose".to_string(),
            description: "fix review feedback".to_string(),
            effective_context_source: Some("resumed".to_string()),
            context_normalized: false,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            model: None,
            resumed_from: Some("source-agent-id".to_string()),
            workflow_run_id: None,
            goal_id: None,
        },
        meta: None,
    };
    adapter.append_update(&info, &SessionUpdate::Grow(Box::new(spawned))).await.unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(loaded.updates.len(), 1);
    match &loaded.updates[0] {
        SessionUpdate::Grow(notification) => {
            match &notification.update {
                GrowSessionUpdateType::SubagentSpawned {
                    subagent_id,
                    effective_context_source,
                    resumed_from,
                    ..
                } => {
                    assert_eq!(subagent_id, "child-resumed");
                    assert_eq!(effective_context_source.as_deref(), Some("resumed"),);
                    assert_eq!(
                        resumed_from.as_deref(),
                        Some("source-agent-id"),
                        "resumed_from should round-trip through JSONL persistence"
                    );
                }
                other => panic!("Expected SubagentSpawned, got {other:?}"),
            }
        }
        other => panic!("Expected Grow update, got {other:?}"),
    }
}
#[tokio::test]
async fn fork_copies_only_referenced_prompt_blobs_without_rewriting_identity() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("blob-src"),
        cwd: "/source/workspace".to_string(),
    };
    adapter
        .init_session(&source_info, default_model_id())
        .await
        .unwrap();
    let source_prompts = adapter.session_dir(&source_info).join("prompts");
    std::fs::create_dir_all(&source_prompts).unwrap();
    let referenced_content = "complete oversized prompt";
    let referenced_hash = blake3::hash(referenced_content.as_bytes()).to_hex().to_string();
    let unreferenced_content = "future prompt must not leak";
    let unreferenced_hash = blake3::hash(unreferenced_content.as_bytes())
        .to_hex()
        .to_string();
    let referenced = source_prompts.join(format!("{referenced_hash}.txt"));
    let unreferenced = source_prompts.join(format!("{unreferenced_hash}.txt"));
    std::fs::write(&referenced, referenced_content).unwrap();
    std::fs::write(&unreferenced, unreferenced_content).unwrap();
    let prompt_ref = format!(
        "{}{}",
        crate::session::persistence::PROMPT_BLOB_REF_PREFIX,
        referenced_hash
    );
    append_timeline_seed(
        &adapter,
        &source_info,
        vec![ConversationItem::user(format!(
            "[Full request offloaded to file]\n{prompt_ref}\nread it"
        ))],
    )
    .await;

    let target_info = Info {
        id: acp::SessionId::new("blob-dst"),
        cwd: "/target/workspace".to_string(),
    };
    let result = adapter
        .copy_session_data(&source_info, &target_info, CopySessionOptions::default())
        .await
        .unwrap();

    assert_eq!(result.prompt_blobs_copied, 1);
    let target_prompts = adapter.session_dir(&target_info).join("prompts");
    assert_eq!(
        std::fs::read_to_string(target_prompts.join(format!("{referenced_hash}.txt"))).unwrap(),
        referenced_content
    );
    assert!(!target_prompts.join(format!("{unreferenced_hash}.txt")).exists());
    let loaded = adapter
        .load_session_without_updates(&target_info)
        .await
        .unwrap();
    let surface = loaded_surface(&loaded.timeline_events);
    let text = surface[0].text_content();
    assert!(text.contains(&prompt_ref));
    assert!(!text.contains(&target_prompts.to_string_lossy().to_string()));
    assert!(!text.contains(&source_prompts.to_string_lossy().to_string()));
}

#[tokio::test]
async fn fork_fails_closed_when_a_prompt_blob_reference_is_missing() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("missing-blob-src"),
        cwd: "/source/workspace".to_string(),
    };
    adapter
        .init_session(&source_info, default_model_id())
        .await
        .unwrap();
    let missing_ref = format!(
        "{}{}",
        crate::session::persistence::PROMPT_BLOB_REF_PREFIX,
        "a".repeat(64)
    );
    append_timeline_seed(
        &adapter,
        &source_info,
        vec![ConversationItem::user(format!(
            "[Full request offloaded to file]\n{missing_ref}\nread it"
        ))],
    )
    .await;

    let target_info = Info {
        id: acp::SessionId::new("missing-blob-dst"),
        cwd: "/target/workspace".to_string(),
    };
    let error = adapter
        .copy_session_data(&source_info, &target_info, CopySessionOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(!adapter.session_dir(&target_info).exists());
}
#[tokio::test]
async fn test_copy_session_data_basic() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("source-session-123"),
        cwd: "/source/workspace".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let messages = create_test_chat_messages();
    append_timeline_seed(&adapter, &source_info, messages.clone()).await;
    let notification = create_test_notification();
    adapter
        .append_update(&source_info, &SessionUpdate::Acp(Box::new(notification)))
        .await
        .unwrap();
    let target_info = Info {
        id: acp::SessionId::new("fork-source-session-123-abcd1234"),
        cwd: "/target/workspace".to_string(),
    };
    let options = CopySessionOptions {
        parent_session_id: Some("source-session-123".to_string()),
        new_model_id: None,
        target_prompt_index: None,
        ..Default::default()
    };
    let result = adapter
        .copy_session_data(&source_info, &target_info, options)
        .await
        .unwrap();
    assert_eq!(result.surface_items_copied, 3);
    assert_eq!(result.updates_copied, 1);
    let loaded = adapter.load_session(&target_info).await.unwrap();
    assert_eq!(loaded.summary.info.id, target_info.id);
    assert_eq!(loaded.summary.info.cwd, "/target/workspace");
    assert_eq!(
            loaded.summary.parent_session_id,
            Some("source-session-123".to_string())
        );
    assert!(loaded.summary.forked_at.is_some());
    assert_eq!(loaded_surface(&loaded.timeline_events).len(), 3);
    assert_eq!(loaded.updates.len(), 1);
    match &loaded.updates[0] {
        SessionUpdate::Acp(notification) => {
            assert_eq!(
                    notification.session_id.0.as_ref(),
                    "fork-source-session-123-abcd1234"
                );
        }
        _ => panic!("Expected ACP update"),
    }
}

#[tokio::test]
async fn load_rejects_a_published_session_without_timeline() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    std::fs::remove_file(adapter.timeline_file(&info)).unwrap();

    let error = adapter.load_session_without_updates(&info).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("mandatory Timeline ledger is missing"));
}

#[tokio::test]
async fn load_rejects_a_non_current_session_format() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    let mut summary = adapter.init_session(&info, default_model_id()).await.unwrap();
    summary.session_format_version = crate::session::persistence::SESSION_FORMAT_VERSION - 1;
    adapter.write_summary_sync(&info, &summary).unwrap();

    let error = adapter.load_session_without_updates(&info).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("unsupported session format"));
}

#[tokio::test]
async fn copy_rejects_an_existing_target_without_touching_it() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source = Info {
        id: acp::SessionId::new("existing-target-source"),
        cwd: "/source".into(),
    };
    let target = Info {
        id: acp::SessionId::new("existing-target"),
        cwd: "/target".into(),
    };
    adapter.init_session(&source, default_model_id()).await.unwrap();
    append_timeline_seed(&adapter, &source, vec![ConversationItem::user("source")]).await;
    let target_dir = adapter.session_dir(&target);
    std::fs::create_dir_all(&target_dir).unwrap();
    let marker = target_dir.join("owned-by-caller");
    std::fs::write(&marker, b"untouched").unwrap();

    let error = adapter
        .copy_session_data(&source, &target, Default::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&marker).unwrap(), b"untouched");
}

#[tokio::test]
async fn failed_copy_publishes_no_target_or_staging_directory() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source = Info {
        id: acp::SessionId::new("broken-source"),
        cwd: "/source".into(),
    };
    let target = Info {
        id: acp::SessionId::new("never-published"),
        cwd: "/target".into(),
    };
    adapter.init_session(&source, default_model_id()).await.unwrap();
    std::fs::remove_file(adapter.timeline_file(&source)).unwrap();

    let error = adapter
        .copy_session_data(&source, &target, Default::default())
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    let target_dir = adapter.session_dir(&target);
    assert!(!target_dir.exists(), "failed fork must not publish a target");
    let parent = target_dir.parent().unwrap();
    let staging_prefix = format!(".{}.", target.id);
    assert!(
        std::fs::read_dir(parent)
            .unwrap()
            .all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(&staging_prefix)),
        "failed fork must clean its staging directory"
    );
}

#[tokio::test]
async fn test_copy_session_data_transforms_updates() {
    use crate::extensions::notification::{
        DiffContent, SessionNotification as GrowSessionNotification,
        SessionUpdate as GrowSessionUpdateType,
    };
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("source-grow"),
        cwd: "/source".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let grow_notification = GrowSessionNotification {
        session_id: acp::SessionId::new("source-grow"),
        update: GrowSessionUpdateType::DiffReview {
            content: vec![DiffContent {
                    diff: acp::Diff::new(
                        std::path::PathBuf::from("/test/file.rs"),
                        "new".to_string(),
                    )
                    .old_text(Some("old".to_string())),
                }],
        },
        meta: None,
    };
    adapter
        .append_update(&source_info, &SessionUpdate::Grow(Box::new(grow_notification)))
        .await
        .unwrap();
    let target_info = Info {
        id: acp::SessionId::new("fork-source-grow-abcd1234"),
        cwd: "/target".to_string(),
    };
    adapter
        .copy_session_data(&source_info, &target_info, Default::default())
        .await
        .unwrap();
    let loaded = adapter.load_session(&target_info).await.unwrap();
    match &loaded.updates[0] {
        SessionUpdate::Grow(notification) => {
            assert_eq!(
                    notification.session_id.0.as_ref(),
                    "fork-source-grow-abcd1234"
                );
        }
        _ => panic!("Expected Grow update"),
    }
}

#[tokio::test]
async fn copy_session_data_does_not_clone_live_subagent_routing_ids() {
    use crate::extensions::notification::{
        SessionNotification as GrowSessionNotification, SessionUpdate as GrowSessionUpdateType,
    };

    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("source-with-child"),
        cwd: "/source".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let grow_notification = GrowSessionNotification {
        session_id: source_info.id.clone(),
        update: GrowSessionUpdateType::SubagentProgress {
            subagent_id: "sub-1".into(),
            parent_session_id: source_info.id.0.to_string(),
            child_session_id: "live-child-session".into(),
            duration_ms: 1,
            turn_count: 1,
            tool_call_count: 0,
            tokens_used: 0,
            context_window_tokens: 1,
            context_usage_pct: 0,
            tools_used: Vec::new(),
            error_count: 0,
        },
        meta: None,
    };
    adapter
        .append_update(
            &source_info,
            &SessionUpdate::Grow(Box::new(grow_notification)),
        )
        .await
        .unwrap();

    let target_info = Info {
        id: acp::SessionId::new("fork-without-live-child"),
        cwd: "/target".to_string(),
    };
    let copied = adapter
        .copy_session_data(&source_info, &target_info, Default::default())
        .await
        .unwrap();
    let loaded = adapter.load_session(&target_info).await.unwrap();

    assert_eq!(copied.updates_copied, 0);
    assert!(loaded.updates.is_empty());
}
fn fork_user_chunk(session_id: &str, text: &str, prompt_index: usize) -> SessionUpdate {
    let chunk = acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
        )
        .meta(serde_json::json!({ "promptIndex": prompt_index }).as_object().cloned());
    SessionUpdate::Acp(
        Box::new(
            acp::SessionNotification::new(
                acp::SessionId::new(session_id),
                acp::SessionUpdate::UserMessageChunk(chunk),
            ),
        ),
    )
}
fn fork_agent_chunk(session_id: &str, text: &str) -> SessionUpdate {
    SessionUpdate::Acp(
        Box::new(
            acp::SessionNotification::new(
                acp::SessionId::new(session_id),
                acp::SessionUpdate::AgentMessageChunk(
                    acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
                    ),
                ),
            ),
        ),
    )
}
fn fork_rewind_marker(session_id: &str, target_prompt_index: usize) -> SessionUpdate {
    use crate::extensions::notification::{
        SessionNotification as GrowSessionNotification,
        SessionUpdate as GrowSessionUpdateType,
    };
    SessionUpdate::Grow(
        Box::new(GrowSessionNotification {
            session_id: acp::SessionId::new(session_id),
            update: GrowSessionUpdateType::RewindMarker {
                target_prompt_index,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
            meta: None,
        }),
    )
}
fn chat_user(text: &str, prompt_index: usize) -> ConversationItem {
    let mut item = ConversationItem::user(text);
    item.set_prompt_index(prompt_index);
    item
}
/// Fork truncation targets the live branch — dead-branch runs from a
/// prior rewind overlap its stamps (indices are branch-local) — and keeps
/// prompt N inclusive in both the updates and chat (model-context) files.
#[tokio::test]
async fn copy_session_data_fork_truncates_live_branch_inclusive() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let sid = "src-rewound";
    let source_info = Info {
        id: acp::SessionId::new(sid),
        cwd: "/src".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    for update in [
        fork_user_chunk(sid, "P0", 0),
        fork_agent_chunk(sid, "A0"),
        fork_user_chunk(sid, "P1-dead", 1),
        fork_agent_chunk(sid, "A1-dead"),
        fork_rewind_marker(sid, 1),
        fork_user_chunk(sid, "P1b", 1),
        fork_agent_chunk(sid, "A1b"),
        fork_user_chunk(sid, "P2", 2),
    ] {
        adapter.append_update(&source_info, &update).await.unwrap();
    }
    let conversation = vec![
        chat_user("P0", 0),
        ConversationItem::assistant("A0"),
        chat_user("P1b", 1),
        ConversationItem::assistant("A1b"),
        chat_user("P2", 2),
    ];
    let mut timeline = chat_state::Timeline::default();
    for item in conversation {
        let cause = match &item {
            ConversationItem::User(_) => chat_state::MessageCause::User,
            ConversationItem::Assistant(_) => chat_state::MessageCause::Assistant,
            _ => unreachable!("fixture contains only user and assistant items"),
        };
        timeline.append(item, cause).unwrap();
    }
    for event in timeline.events() {
        adapter.append_timeline_event(&source_info, event).await.unwrap();
    }
    let fork_at = |target: usize, fork_id: &str| {
        let target_info = Info {
            id: acp::SessionId::new(fork_id),
            cwd: "/src".to_string(),
        };
        let options = CopySessionOptions {
            target_prompt_index: Some(target),
            ..Default::default()
        };
        (target_info, options)
    };
    let (target_info, options) = fork_at(1, "fork-at-1");
    let result = adapter
        .copy_session_data(&source_info, &target_info, options)
        .await
        .unwrap();
    assert_eq!(result.updates_copied, 4);
    assert_eq!(result.surface_items_copied, 4);
    let loaded = adapter.load_session(&target_info).await.unwrap();
    let last = loaded.updates.last().unwrap();
    assert!(
            matches!(
                last,
                SessionUpdate::Acp(n) if matches!(
                    &n.update,
                    acp::SessionUpdate::AgentMessageChunk(c)
                        if matches!(&c.content, acp::ContentBlock::Text(t) if t.text == "A1b")
                )
            ),
            "fork must end at the live branch's A1b, got {last:?}"
        );
    let (target_info, options) = fork_at(0, "fork-at-0");
    let result = adapter
        .copy_session_data(&source_info, &target_info, options)
        .await
        .unwrap();
    assert_eq!(result.updates_copied, 2, "P0 + A0");
    assert_eq!(result.surface_items_copied, 2, "P0 + A0 in model context");
}
#[tokio::test]
async fn test_copy_session_data_source_not_found() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("nonexistent"),
        cwd: "/nonexistent".to_string(),
    };
    let target_info = Info {
        id: acp::SessionId::new("fork-nonexistent-abcd1234"),
        cwd: "/target".to_string(),
    };
    let result = adapter
        .copy_session_data(&source_info, &target_info, Default::default())
        .await;
    assert!(result.is_err());
}
#[tokio::test]
async fn test_copy_session_data_with_model_override() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("source-model-test"),
        cwd: "/source".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let target_info = Info {
        id: acp::SessionId::new("fork-model-test"),
        cwd: "/target".to_string(),
    };
    let options = CopySessionOptions {
        parent_session_id: Some("source-model-test".to_string()),
        new_model_id: Some("grow-3".to_string()),
        target_prompt_index: None,
        ..Default::default()
    };
    adapter.copy_session_data(&source_info, &target_info, options).await.unwrap();
    let loaded = adapter.load_session(&target_info).await.unwrap();
    assert_eq!(loaded.summary.current_model_id.0.as_ref(), "grow-3");
    assert_eq!(
            loaded.summary.parent_session_id,
            Some("source-model-test".to_string())
        );
}

#[tokio::test]
async fn prompt_records_are_derived_from_timeline_turns() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    append_completed_prompt_turn(&adapter, &info, 1, 0, "first prompt").await;
    append_completed_prompt_turn(&adapter, &info, 2, 1, "second prompt").await;

    let records = adapter.load_prompt_records(&info).await.unwrap();
    assert_eq!(
        records
            .iter()
            .map(|record| record.text.as_str())
            .collect::<Vec<_>>(),
        vec!["first prompt", "second prompt"]
    );
    assert!(
        records
            .iter()
            .all(|record| record.input_kind == chat_state::TurnInputKind::Prompt)
    );
}

#[tokio::test]
async fn prompt_records_fail_closed_without_timeline() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = Info {
        id: acp::SessionId::new("missing"),
        cwd: "/missing".into(),
    };

    let error = adapter.load_prompt_records(&info).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn copy_fork_provenance_persisted_in_summary() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("src-prov"),
        cwd: "/src".to_string(),
    };
    let target_info = Info {
        id: acp::SessionId::new("tgt-prov"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let options = CopySessionOptions {
        parent_session_id: Some("src-prov".to_string()),
        session_kind: Some("subagent_fork".to_string()),
        fork_context_source: Some("forked".to_string()),
        fork_parent_prompt_id: Some("prompt-42".to_string()),
        ..Default::default()
    };
    adapter.copy_session_data(&source_info, &target_info, options).await.unwrap();
    let data = adapter.load_session(&target_info).await.unwrap();
    assert_eq!(data.summary.session_kind.as_deref(), Some("subagent_fork"));
    assert_eq!(data.summary.fork_context_source.as_deref(), Some("forked"));
    assert_eq!(
            data.summary.fork_parent_prompt_id.as_deref(),
            Some("prompt-42")
        );
}
#[tokio::test]
async fn summary_provenance_survives_write_read_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let info = Info {
        id: acp::SessionId::new("prov-rt"),
        cwd: "/test".to_string(),
    };
    let mut summary = adapter.init_session(&info, default_model_id()).await.unwrap();
    summary.fork_context_source = Some("forked".to_string());
    summary.fork_parent_prompt_id = Some("prompt-99".to_string());
    summary.session_kind = Some("subagent_fork".to_string());
    let json = serde_json::to_vec_pretty(&summary).unwrap();
    std::fs::write(adapter.session_dir(&info).join("summary.json"), json).unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(
            loaded.summary.fork_context_source.as_deref(),
            Some("forked")
        );
    assert_eq!(
            loaded.summary.fork_parent_prompt_id.as_deref(),
            Some("prompt-99")
        );
    assert_eq!(
            loaded.summary.session_kind.as_deref(),
            Some("subagent_fork")
        );
}
#[tokio::test]
async fn summary_provenance_defaults_to_none() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let info = Info {
        id: acp::SessionId::new("prov-none"),
        cwd: "/test".to_string(),
    };
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let loaded = adapter.load_session(&info).await.unwrap();
    assert!(loaded.summary.fork_context_source.is_none());
    assert!(loaded.summary.fork_parent_prompt_id.is_none());
}
#[tokio::test]
async fn copy_session_kind_override() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("src-kind"),
        cwd: "/src".to_string(),
    };
    let target_info = Info {
        id: acp::SessionId::new("tgt-kind"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let options = CopySessionOptions {
        session_kind: Some("subagent_fork".to_string()),
        ..Default::default()
    };
    adapter.copy_session_data(&source_info, &target_info, options).await.unwrap();
    let summary = adapter.read_summary_sync(&target_info).unwrap();
    assert_eq!(summary.session_kind.as_deref(), Some("subagent_fork"));
}
#[tokio::test]
async fn copy_session_kind_defaults_to_fork() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("src-dflt"),
        cwd: "/src".to_string(),
    };
    let target_info = Info {
        id: acp::SessionId::new("tgt-dflt"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    adapter
        .copy_session_data(&source_info, &target_info, Default::default())
        .await
        .unwrap();
    let summary = adapter.read_summary_sync(&target_info).unwrap();
    assert_eq!(summary.session_kind.as_deref(), Some("fork"));
}
#[tokio::test]
async fn copy_session_preserves_head_fields() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("src-head"),
        cwd: "/src".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    adapter
        .update_git_head(
            &source_info,
            Some("abc123".into()),
            Some("feature-branch".into()),
        )
        .await
        .unwrap();
    let target_info = Info {
        id: acp::SessionId::new("tgt-head"),
        cwd: "/tgt".to_string(),
    };
    adapter
        .copy_session_data(&source_info, &target_info, CopySessionOptions::default())
        .await
        .unwrap();
    let loaded = adapter.load_summary(&target_info).await.unwrap();
    assert_eq!(loaded.head_commit.as_deref(), Some("abc123"));
    assert_eq!(loaded.head_branch.as_deref(), Some("feature-branch"));
}
#[tokio::test]
async fn copy_without_control_inheritance_has_no_control_event() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("src-pm"),
        cwd: "/src".to_string(),
    };
    let target_info = Info {
        id: acp::SessionId::new("tgt-pm"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    append_control_snapshot(
        &adapter,
        &source_info,
        &crate::session::control::SessionControlSnapshot::new(
            1,
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Clarify),
            None,
        ),
    )
    .await;
    let result = adapter
        .copy_session_data(
            &source_info,
            &target_info,
            CopySessionOptions {
                inherit_control: false,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(!result.control_event_seeded);
    assert!(adapter
        .load_session_without_updates(&target_info)
        .await
        .unwrap()
        .control_snapshot
        .is_none());
}

#[tokio::test]
async fn forked_control_snapshot_drops_goal_runtime_ownership() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source_info = Info {
        id: acp::SessionId::new("src-goal"),
        cwd: "/src".to_string(),
    };
    let target_info = Info {
        id: acp::SessionId::new("tgt-goal"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source_info, default_model_id()).await.unwrap();
    let mut goal = crate::session::goal_tracker::GoalTracker::new();
    goal.create_goal(
        "goal-1".into(),
        "ship safely".into(),
        None,
        0,
        "2026-01-01T00:00:00Z".into(),
        None,
    );
    append_control_snapshot(
        &adapter,
        &source_info,
        &crate::session::control::SessionControlSnapshot::new(
            7,
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            goal.snapshot().cloned(),
        ),
    )
    .await;

    let result = adapter
        .copy_session_data(&source_info, &target_info, CopySessionOptions::default())
        .await
        .unwrap();
    assert!(result.control_event_seeded);
    let forked = adapter
        .load_session_without_updates(&target_info)
        .await
        .unwrap()
        .control_snapshot
        .expect("fork keeps non-runtime control metadata");
    assert!(forked.goal.is_none());
    assert_eq!(
        forked.behavior.state,
        crate::session::behavior::BehaviorState::Normal
    );
}

#[test]
fn fork_filter_removes_synthetic_user_messages() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::system("system prompt"),
            ConversationItem::user("real question"),
            ConversationItem::User(UserItem {
                content: vec![ContentPart::Text {
                    text: "doom loop".into(),
                }],
                synthetic_reason: Some(SyntheticReason::SystemReminder),
                permission_evidence: None,
                ..Default::default()
            }),
            ConversationItem::assistant("response"),
        ];
    super::fork_filter_surface(&mut items);
    assert!(
            !items.iter().any(|i| match i {
                ConversationItem::User(u) => u.synthetic_reason.is_some(),
                _ => false,
            }),
            "synthetic messages should be stripped"
        );
}
#[test]
fn fork_filter_truncates_at_complete_turn() {
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q1"),
            ConversationItem::assistant("a1"),
            ConversationItem::user("q2"),
            // No assistant response — incomplete turn
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(items.len(), 3, "should truncate after last complete turn");
    assert!(matches!(items[2], ConversationItem::Assistant(_)));
}
#[test]
fn fork_filter_handles_consecutive_user_messages() {
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("user prefix with project info"),
            ConversationItem::user("actual user query"),
            ConversationItem::assistant("response to query"),
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            4,
            "consecutive User messages should be treated as a single turn: got {items:?}"
        );
    assert!(matches!(items[0], ConversationItem::System(_)));
    assert!(matches!(items[1], ConversationItem::User(_)));
    assert!(matches!(items[2], ConversationItem::User(_)));
    assert!(matches!(items[3], ConversationItem::Assistant(_)));
}
#[test]
fn fork_filter_consecutive_users_with_tool_calls() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("prefix"),
            ConversationItem::user("query"),
            ConversationItem::Assistant(AssistantItem {
                content: String::new().into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "output"),
            ConversationItem::user("follow-up"),
            // Incomplete turn — no assistant response
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            5,
            "should keep through complete tool turn, drop incomplete follow-up"
        );
}
#[test]
fn fork_filter_preserves_complete_tool_turn() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::user("q"),
            ConversationItem::Assistant(AssistantItem {
                content: String::new().into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "output"),
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(items.len(), 3, "complete tool turn should be preserved");
}
#[test]
fn fork_filter_strips_incomplete_tool_turn() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::user("q1"),
            ConversationItem::assistant("a1"),
            ConversationItem::user("q2"),
            ConversationItem::Assistant(AssistantItem {
                content: String::new().into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            // Missing tool result — incomplete
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            2,
            "should truncate before incomplete tool turn (trailing user(q2) also dropped)"
        );
    assert!(matches!(items[0], ConversationItem::User(_)));
    assert!(matches!(items[1], ConversationItem::Assistant(_)));
}
#[tokio::test]
async fn fork_filter_clears_updates() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source = Info {
        id: acp::SessionId::new("src-upd"),
        cwd: "/src".to_string(),
    };
    let target = Info {
        id: acp::SessionId::new("tgt-upd"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source, default_model_id()).await.unwrap();
    append_timeline_seed(
        &adapter,
        &source,
        vec![ConversationItem::user("q"), ConversationItem::assistant("a")],
    )
    .await;
    let result = adapter
        .copy_session_data(
            &source,
            &target,
            CopySessionOptions {
                fork_filter: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(result.updates_copied, 0, "fork_filter should clear updates");
}
async fn assert_copy_clears_pending_relocation(fork_filter: bool) {
    use crate::session::persistence::PendingCwdSwitchReminder;
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source = Info {
        id: acp::SessionId::new(format!("pending-source-{fork_filter}")),
        cwd: "/src".into(),
    };
    let target = Info {
        id: acp::SessionId::new(format!("pending-target-{fork_filter}")),
        cwd: "/target".into(),
    };
    let mut summary = adapter.init_session(&source, default_model_id()).await.unwrap();
    summary.cwd_generation = 3;
    summary.previous_cwd = Some("/older".into());
    summary.pending_cwd_switch_reminder = Some(PendingCwdSwitchReminder {
        cwd_generation: 3,
        previous_cwd: "/src".into(),
        destination_cwd: "/destination".into(),
        content: "switch".into(),
        destination_project_instructions: None,
    });
    adapter.write_summary_sync(&source, &summary).unwrap();
    append_timeline_seed(
        &adapter,
        &source,
        vec![ConversationItem::working_directory_switch("switch", 3)],
    )
    .await;
    adapter
        .copy_session_data(
            &source,
            &target,
            CopySessionOptions {
                fork_filter,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let copied = adapter.read_summary_sync(&target).unwrap();
    assert_eq!(copied.cwd_generation, 3);
    assert_eq!(copied.previous_cwd.as_deref(), Some("/older"));
    assert!(copied.pending_cwd_switch_reminder.is_none());
    let expected_generation = if fork_filter { 0 } else { 3 };
    assert_eq!(
            copied.cwd_switch_bookkeeping_generation,
            expected_generation
        );
}
#[tokio::test]
async fn unfiltered_copy_clears_pending_relocation() {
    assert_copy_clears_pending_relocation(false).await;
}
#[tokio::test]
async fn filtered_copy_clears_pending_relocation() {
    assert_copy_clears_pending_relocation(true).await;
}
#[tokio::test]
async fn init_session_stamps_configured_profile_on_new_session() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    sandbox::set_configured_profile("workspace");
    let expected = sandbox::configured_profile_name().map(String::from);
    let info = Info {
        id: acp::SessionId::new("new-sb"),
        cwd: "/new".to_string(),
    };
    let summary = adapter.init_session(&info, default_model_id()).await.unwrap();
    assert_eq!(summary.sandbox_profile, expected);
    let on_disk = adapter.read_summary_sync(&info).unwrap();
    assert_eq!(on_disk.sandbox_profile, expected);
}
#[tokio::test]
async fn fork_inherits_sandbox_profile() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let source = Info {
        id: acp::SessionId::new("src-sb"),
        cwd: "/src".to_string(),
    };
    let target = Info {
        id: acp::SessionId::new("tgt-sb"),
        cwd: "/tgt".to_string(),
    };
    adapter.init_session(&source, default_model_id()).await.unwrap();
    let mut src_summary = adapter.read_summary_sync(&source).unwrap();
    src_summary.sandbox_profile = Some("workspace".to_string());
    adapter.write_summary_sync(&source, &src_summary).unwrap();
    adapter
        .copy_session_data(&source, &target, CopySessionOptions::default())
        .await
        .unwrap();
    let tgt_summary = adapter.read_summary_sync(&target).unwrap();
    assert_eq!(tgt_summary.sandbox_profile.as_deref(), Some("workspace"));
}
#[test]
fn fork_filter_empty_input_produces_empty() {
    let mut items: Vec<ConversationItem> = vec![];
    super::fork_filter_surface(&mut items);
    assert!(items.is_empty());
}
#[test]
fn fork_filter_keeps_turn_with_reasoning_between_user_and_assistant() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "thinking",
            )),
            ConversationItem::assistant("a"),
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            4,
            "reasoning between user and assistant must not truncate the turn: got {items:?}"
        );
    assert!(matches!(items[3], ConversationItem::Assistant(_)));
}
#[test]
fn fork_filter_keeps_multi_tool_cycle_turn_with_reasoning() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "plan",
            )),
            ConversationItem::Assistant(AssistantItem {
                content: String::new().into(),
                tool_calls: vec![ToolCall {
                    id: "tc1".into(),
                    name: "bash".into(),
                    arguments: "{}".into(),
                }],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "output"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "reflect",
            )),
            ConversationItem::assistant("final text"),
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            7,
            "multi-tool-cycle turn with interleaved reasoning must be fully kept: got {items:?}"
        );
    match items.last() {
        Some(ConversationItem::Assistant(a)) => {
            assert_eq!(a.content.as_ref(), "final text")
        }
        other => panic!("expected final assistant text last, got {other:?}"),
    }
}
#[test]
fn fork_filter_keeps_multi_tool_turn_with_reasoning_between_results() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "plan",
            )),
            ConversationItem::Assistant(AssistantItem {
                content: String::new().into(),
                tool_calls: vec![
                    ToolCall {
                        id: "tc1".into(),
                        name: "bash".into(),
                        arguments: "{}".into(),
                    },
                    ToolCall {
                        id: "tc2".into(),
                        name: "grep".into(),
                        arguments: "{}".into(),
                    },
                ],
                model_id: None,
                model_fingerprint: None,
                reasoning_effort: None,
            }),
            ConversationItem::tool_result("tc1", "out1"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item("mid")),
            ConversationItem::tool_result("tc2", "out2"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "reflect",
            )),
            ConversationItem::assistant("final"),
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            9,
            "multi-tool turn with reasoning between results must be fully kept: got {items:?}"
        );
    match items.last() {
        Some(ConversationItem::Assistant(a)) => assert_eq!(a.content.as_ref(), "final"),
        other => panic!("expected final assistant text last, got {other:?}"),
    }
}
#[test]
fn fork_filter_drops_trailing_incomplete_goal_turn_after_reasoning() {
    use sampling_types::conversation::*;
    let mut items = vec![
            ConversationItem::system("sys"),
            ConversationItem::user("q"),
            ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
                "thinking",
            )),
            ConversationItem::assistant("a"),
            ConversationItem::user("/goal do the thing"),
        ];
    super::fork_filter_surface(&mut items);
    assert_eq!(
            items.len(),
            4,
            "trailing bare /goal user turn must be dropped: got {items:?}"
        );
    match items.last() {
        Some(ConversationItem::Assistant(a)) => assert_eq!(a.content.as_ref(), "a"),
        other => panic!("expected trailing assistant, got {other:?}"),
    }
}
/// Create a minimal on-disk session directory with a summary.json.
/// Returns the path to the session directory.
fn write_test_summary(
    root: &std::path::Path,
    cwd_encoded: &str,
    session_id: &str,
    updated_at: chrono::DateTime<chrono::Utc>,
    last_active_at: Option<chrono::DateTime<chrono::Utc>>,
    hidden: Option<bool>,
    session_kind: Option<&str>,
) -> PathBuf {
    let session_dir = root.join("sessions").join(cwd_encoded).join(session_id);
    std::fs::create_dir_all(&session_dir).unwrap();
    let summary = Summary {
        info: Info {
            id: acp::SessionId::new(session_id),
            cwd: urlencoding::decode(cwd_encoded).unwrap().into_owned(),
        },
        cwd_generation: 0,
        previous_cwd: None,
        pending_cwd_switch_reminder: None,
        cwd_switch_bookkeeping_generation: 0,
        title: None,
        title_source: None,
        title_event_seq: None,
        created_at: updated_at,
        updated_at,
        num_messages: 1,
        current_model_id: default_model_id(),
        parent_session_id: None,
        forked_at: None,
        session_format_version: crate::session::persistence::SESSION_FORMAT_VERSION,
        prompt_display_cwd: None,
        session_kind: session_kind.map(|s| s.to_string()),
        fork_context_source: None,
        fork_parent_prompt_id: None,
        hidden,
        source_workspace_dir: None,
        git_root_dir: None,
        git_remotes: Vec::new(),
        head_commit: None,
        head_branch: None,
        grow_home: None,
        last_active_at,
        worktree_label: None,
        agent_name: None,
        sandbox_profile: None,
        reasoning_effort: None,
    };
    let json = serde_json::to_vec_pretty(&summary).unwrap();
    std::fs::write(session_dir.join("summary.json"), json).unwrap();
    session_dir
}
#[test]
fn scan_session_dirs_returns_empty_for_explicit_mode() {
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(PathBuf::from("/fake"));
    assert!(adapter.scan_session_dirs(None).unwrap().is_empty());
}

#[tokio::test]
async fn dormant_title_append_commits_timeline_and_projection_together() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(tmp.path().join("session"));
    let info = Info {
        id: acp::SessionId::new("title-session"),
        cwd: "/workspace".into(),
    };
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();

    let first = adapter
        .append_session_title_durable(&info, "First title".into())
        .await
        .unwrap();
    assert_eq!(first.seq.get(), 0);
    let second = adapter
        .append_session_title_durable(&info, "Renamed title".into())
        .await
        .unwrap();
    assert_eq!(second.seq.get(), 1);

    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(loaded.summary.display_title(), "Renamed title");
    assert_eq!(loaded.summary.title_event_seq, Some(1));
    let timeline = chat_state::Timeline::from_events(loaded.timeline_events).unwrap();
    assert_eq!(timeline.session_title().unwrap().1.title, "Renamed title");
}

#[tokio::test]
async fn load_repairs_a_lagging_title_projection_from_timeline() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(tmp.path().join("session"));
    let info = Info {
        id: acp::SessionId::new("title-repair"),
        cwd: "/workspace".into(),
    };
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    adapter
        .append_session_title_durable(&info, "Canonical title".into())
        .await
        .unwrap();

    let mut summary = adapter.read_summary_sync(&info).unwrap();
    summary.title = None;
    summary.title_source = None;
    summary.title_event_seq = None;
    adapter.write_summary_sync(&info, &summary).unwrap();

    let loaded = adapter.load_session(&info).await.unwrap();
    assert_eq!(loaded.summary.display_title(), "Canonical title");
    assert_eq!(loaded.summary.title_event_seq, Some(0));
    assert_eq!(
        adapter.read_summary_sync(&info).unwrap().display_title(),
        "Canonical title"
    );
}

#[tokio::test]
async fn load_rejects_a_conflicting_title_projection_at_canonical_seq() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_explicit_session_dir(tmp.path().join("session"));
    let info = Info {
        id: acp::SessionId::new("title-conflict"),
        cwd: "/workspace".into(),
    };
    adapter
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    adapter
        .append_session_title_durable(&info, "Canonical title".into())
        .await
        .unwrap();

    let mut summary = adapter.read_summary_sync(&info).unwrap();
    summary.title = Some("Conflicting cache".into());
    adapter.write_summary_sync(&info, &summary).unwrap();

    let error = adapter.load_session(&info).await.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("conflicts"));
}
#[test]
fn scan_session_dirs_returns_empty_when_no_sessions_dir() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    assert!(adapter.scan_session_dirs(None).unwrap().is_empty());
}
#[test]
fn scan_session_dirs_finds_all_sessions() {
    let tmp = TempDir::new().unwrap();
    let now = chrono::Utc::now();
    let cwd = crate::util::grow_home::encode_cwd_dirname("/home/user/project");
    write_test_summary(tmp.path(), &cwd, "s1", now, None, None, None);
    write_test_summary(tmp.path(), &cwd, "s2", now, None, None, None);
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let dirs = adapter.scan_session_dirs(None).unwrap();
    assert_eq!(dirs.len(), 2);
}
#[test]
fn scan_session_dirs_filters_by_cwd() {
    let tmp = TempDir::new().unwrap();
    let now = chrono::Utc::now();
    let cwd_a = crate::util::grow_home::encode_cwd_dirname("/home/user/project-a");
    let cwd_b = crate::util::grow_home::encode_cwd_dirname("/home/user/project-b");
    write_test_summary(tmp.path(), &cwd_a, "s1", now, None, None, None);
    write_test_summary(tmp.path(), &cwd_b, "s2", now, None, None, None);
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let a_dirs = adapter.scan_session_dirs(Some("/home/user/project-a")).unwrap();
    assert_eq!(a_dirs.len(), 1);
    assert!(a_dirs[0].ends_with("s1"));
    let all_dirs = adapter.scan_session_dirs(None).unwrap();
    assert_eq!(all_dirs.len(), 2);
}
#[test]
fn scan_session_dirs_skips_non_directory_entries() {
    let tmp = TempDir::new().unwrap();
    let cwd = crate::util::grow_home::encode_cwd_dirname("/project");
    let cwd_dir = tmp.path().join("sessions").join(&cwd);
    std::fs::create_dir_all(&cwd_dir).unwrap();
    std::fs::write(cwd_dir.join("stray-file.txt"), b"oops").unwrap();
    std::fs::create_dir(cwd_dir.join("real-session")).unwrap();
    std::fs::write(cwd_dir.join("real-session/summary.json"), b"{}").unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let dirs = adapter.scan_session_dirs(None).unwrap();
    assert_eq!(dirs.len(), 1);
    assert!(dirs[0].ends_with("real-session"));
}
#[tokio::test]
async fn list_sessions_recent_returns_most_recent_by_mtime() {
    let tmp = TempDir::new().unwrap();
    let cwd = crate::util::grow_home::encode_cwd_dirname("/workspace");
    let t1 = chrono::Utc::now() - chrono::Duration::hours(3);
    let t2 = chrono::Utc::now() - chrono::Duration::hours(2);
    let t3 = chrono::Utc::now() - chrono::Duration::hours(1);
    let dir1 = write_test_summary(tmp.path(), &cwd, "old", t1, None, None, None);
    let dir2 = write_test_summary(tmp.path(), &cwd, "mid", t2, None, None, None);
    let dir3 = write_test_summary(tmp.path(), &cwd, "new", t3, None, None, None);
    set_mtime(&dir1.join("summary.json"), t1);
    set_mtime(&dir2.join("summary.json"), t2);
    set_mtime(&dir3.join("summary.json"), t3);
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let recent = adapter.list_sessions_recent(2).await.unwrap();
    assert_eq!(recent.len(), 2, "should return at most `limit` sessions");
    assert_eq!(recent[0].info.id, acp::SessionId::new("new"));
    assert_eq!(recent[1].info.id, acp::SessionId::new("mid"));
}
#[tokio::test]
async fn list_sessions_recent_excludes_hidden_sessions() {
    let tmp = TempDir::new().unwrap();
    let cwd = crate::util::grow_home::encode_cwd_dirname("/workspace");
    let now = chrono::Utc::now();
    write_test_summary(tmp.path(), &cwd, "visible", now, None, None, None);
    write_test_summary(tmp.path(), &cwd, "hidden-explicit", now, None, Some(true), None);
    write_test_summary(
        tmp.path(),
        &cwd,
        "hidden-subagent",
        now,
        None,
        None,
        Some("subagent"),
    );
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let recent = adapter.list_sessions_recent(100).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].info.id, acp::SessionId::new("visible"));
}
#[tokio::test]
async fn list_sessions_recent_empty_dir() {
    let tmp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let recent = adapter.list_sessions_recent(10).await.unwrap();
    assert!(recent.is_empty());
}
#[tokio::test]
async fn list_sessions_sorts_by_last_active_at_over_updated_at() {
    let tmp = TempDir::new().unwrap();
    let cwd_path = "/ws/resume-sort";
    let cwd = crate::util::grow_home::encode_cwd_dirname(cwd_path);
    let now = chrono::Utc::now();
    write_test_summary(
        tmp.path(),
        &cwd,
        "stale_activity",
        now,
        Some(now - chrono::Duration::hours(20)),
        None,
        None,
    );
    write_test_summary(
        tmp.path(),
        &cwd,
        "recent_activity",
        now - chrono::Duration::hours(10),
        Some(now - chrono::Duration::hours(1)),
        None,
        None,
    );
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let listed = adapter.list_sessions(Some(cwd_path)).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].info.id, acp::SessionId::new("recent_activity"));
    assert_eq!(listed[1].info.id, acp::SessionId::new("stale_activity"));
}
#[tokio::test]
async fn list_sessions_recent_sorts_by_updated_at() {
    let tmp = TempDir::new().unwrap();
    let cwd = crate::util::grow_home::encode_cwd_dirname("/ws");
    let now = chrono::Utc::now();
    let t_old = now - chrono::Duration::hours(10);
    let t_new = now;
    write_test_summary(tmp.path(), &cwd, "a-old", t_old, None, None, None);
    write_test_summary(tmp.path(), &cwd, "b-new", t_new, None, None, None);
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let recent = adapter.list_sessions_recent(10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].info.id, acp::SessionId::new("b-new"));
    assert_eq!(recent[1].info.id, acp::SessionId::new("a-old"));
}
#[tokio::test]
async fn list_sessions_recent_spans_multiple_workspaces() {
    let tmp = TempDir::new().unwrap();
    let cwd_a = crate::util::grow_home::encode_cwd_dirname("/project-a");
    let cwd_b = crate::util::grow_home::encode_cwd_dirname("/project-b");
    let now = chrono::Utc::now();
    write_test_summary(
        tmp.path(),
        &cwd_a,
        "a1",
        now - chrono::Duration::hours(1),
        None,
        None,
        None,
    );
    write_test_summary(tmp.path(), &cwd_b, "b1", now, None, None, None);
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let recent = adapter.list_sessions_recent(10).await.unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].info.id, acp::SessionId::new("b1"));
    assert_eq!(recent[1].info.id, acp::SessionId::new("a1"));
}
#[tokio::test]
async fn list_sessions_recent_skips_corrupt_summary() {
    let tmp = TempDir::new().unwrap();
    let cwd = crate::util::grow_home::encode_cwd_dirname("/ws");
    let now = chrono::Utc::now();
    write_test_summary(tmp.path(), &cwd, "good", now, None, None, None);
    let bad_dir = tmp.path().join("sessions").join(&cwd).join("bad");
    std::fs::create_dir_all(&bad_dir).unwrap();
    std::fs::write(bad_dir.join("summary.json"), b"not valid json!!!").unwrap();
    let adapter = JsonlStorageAdapter::with_root(tmp.path().to_path_buf());
    let recent = adapter.list_sessions_recent(10).await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].info.id, acp::SessionId::new("good"));
}
/// Helper: set the mtime of a file to a specific chrono DateTime.
fn set_mtime(path: &std::path::Path, time: chrono::DateTime<chrono::Utc>) {
    use std::time::{Duration, UNIX_EPOCH};
    let secs = time.timestamp() as u64;
    let system_time = UNIX_EPOCH + Duration::from_secs(secs);
    let mtime = filetime::FileTime::from_system_time(system_time);
    filetime::set_file_mtime(path, mtime).unwrap();
}
/// Invalid UTF-8 in `updates.jsonl` poisons only its own line.
#[tokio::test]
async fn read_updates_jsonl_skips_invalid_utf8_line() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let notification = SessionUpdate::Acp(
        Box::new(
            acp::SessionNotification::new(
                info.id.clone(),
                acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new("hi".to_string())),
                    ),
                ),
            ),
        ),
    );
    adapter.append_update(&info, &notification).await.unwrap();
    let updates_path = adapter.updates_file(&info);
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&updates_path)
            .unwrap();
        f.write_all(&[0xE2, 0x82, b'\n']).unwrap();
    }
    let updates = adapter.read_updates_jsonl(updates_path).unwrap();
    assert_eq!(
            updates.len(),
            1,
            "valid line kept, invalid-UTF8 line skipped"
        );
}
/// Same self-healing for `updates.jsonl` appends, and the lenient reader
/// skips the isolated torn line.
#[tokio::test]
async fn append_update_terminates_torn_trailing_line() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let notification = |text: &str| SessionUpdate::Acp(
        Box::new(
            acp::SessionNotification::new(
                info.id.clone(),
                acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new(text.to_string())),
                    ),
                ),
            ),
        ),
    );
    adapter.append_update(&info, &notification("first")).await.unwrap();
    let updates_path = adapter.updates_file(&info);
    {
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&updates_path)
            .unwrap();
        f.write_all(br#"{"timestamp":"2026-07-06T00:00:00Z","update":{"sessionId":"tor"#)
            .unwrap();
    }
    adapter.append_update(&info, &notification("second")).await.unwrap();
    let raw = std::fs::read_to_string(&updates_path).unwrap();
    assert_eq!(
            raw.lines().count(),
            3,
            "first + torn(terminated) + second: {raw:?}"
        );
    let updates = adapter.read_updates_jsonl(updates_path).unwrap();
    assert_eq!(updates.len(), 2, "torn line skipped, real updates kept");
}
#[tokio::test]
async fn timeline_control_roundtrips_goal_through_light_session_load() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let mut tracker = crate::session::goal_tracker::GoalTracker::new();
    tracker.create_goal(
        "goal-1".into(),
        "verify persistence".into(),
        Some(10_000),
        50,
        "now".into(),
        None,
    );
    let snapshot = crate::session::control::SessionControlSnapshot::new(
        7,
        crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
        tracker.snapshot().cloned(),
    );
    append_control_snapshot(&adapter, &info, &snapshot).await;

    let loaded = adapter.load_session_without_updates(&info).await.unwrap();
    let control = loaded.control_snapshot.expect("control state should load");
    assert_eq!(control.control_revision, 7);
    assert_eq!(control.behavior.state, crate::session::behavior::BehaviorState::Goal);
    let goal = control.goal.expect("Goal state should load");
    assert_eq!(goal.goal_id, "goal-1");
    assert_eq!(
        goal.architecture_version,
        crate::session::goal_tracker::GOAL_ARCHITECTURE_VERSION,
    );
}

#[tokio::test]
async fn malformed_timeline_control_bricks_session_load() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let events = adapter.read_timeline_events_sync(&info).unwrap();
    let mut timeline = chat_state::Timeline::from_events(events).unwrap();
    let event = timeline
        .record(chat_state::TimelineEventKind::Control(chat_state::ControlEvent {
            revision: 1,
            snapshot: serde_json::json!({ "broken": true }),
        }))
        .unwrap();
    adapter.append_timeline_event(&info, &event).await.unwrap();

    let error = adapter
        .load_session_without_updates(&info)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[tokio::test]
async fn committed_timeline_is_not_rejected_when_summary_projection_fails() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    std::fs::write(adapter.summary_file(&info), b"not valid json").unwrap();

    let events = adapter.read_timeline_events_sync(&info).unwrap();
    let mut timeline = chat_state::Timeline::from_events(events).unwrap();
    let event = timeline
        .append(
            ConversationItem::user("canonical fact"),
            chat_state::MessageCause::User,
        )
        .unwrap();

    adapter
        .append_timeline_event_durable(&info, &event)
        .await
        .expect("summary is a projection and cannot reject a committed Timeline fact");

    let stored = adapter.read_timeline_events_sync(&info).unwrap();
    assert_eq!(
        serde_json::to_value(stored.last().unwrap()).unwrap(),
        serde_json::to_value(&event).unwrap(),
    );
}

#[tokio::test]
async fn session_signals_restore_only_from_timeline() {
    let temp_dir = TempDir::new().unwrap();
    let info = create_test_info();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let events = adapter.read_timeline_events_sync(&info).unwrap();
    let mut timeline = chat_state::Timeline::from_events(events).unwrap();
    let signals = crate::session::signals::SessionSignals {
        turn_count: 3,
        tool_call_count: 8,
        ..Default::default()
    };
    let event = timeline.record(signals.timeline_kind().unwrap()).unwrap();
    adapter.append_timeline_event(&info, &event).await.unwrap();

    let loaded = adapter.load_session_without_updates(&info).await.unwrap();
    assert_eq!(loaded.signals, Some(signals));
    assert!(!adapter.session_dir(&info).join("signals.json").exists());
}

#[tokio::test]
async fn announcement_state_restores_and_forks_only_through_timeline() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source = create_test_info();
    let target = Info {
        id: acp::SessionId::new("announcement-fork-target"),
        cwd: "/test/fork".into(),
    };
    adapter.init_session(&source, default_model_id()).await.unwrap();
    let events = adapter.read_timeline_events_sync(&source).unwrap();
    let mut timeline = chat_state::Timeline::from_events(events).unwrap();
    let state = crate::session::announcement_state::AnnouncementState {
        mcp_server_fingerprints: std::collections::HashMap::from([(
            "source-control".into(),
            crate::session::announcement_state::McpServerFingerprint {
                tool_count: 2,
                description_hash: 11,
                tool_names_hash: 22,
            },
        )]),
        announced_skill_names: std::collections::HashSet::from(["review".into()]),
    };
    let event = timeline.record(state.timeline_kind().unwrap()).unwrap();
    adapter.append_timeline_event(&source, &event).await.unwrap();

    let resumed = adapter.load_session_without_updates(&source).await.unwrap();
    assert_eq!(resumed.announcement_state, Some(state.clone()));
    assert!(
        !adapter
            .session_dir(&source)
            .join("announcement_state.json")
            .exists()
    );

    adapter
        .copy_session_data(&source, &target, CopySessionOptions::default())
        .await
        .unwrap();
    let forked = adapter.load_session_without_updates(&target).await.unwrap();
    assert_eq!(forked.announcement_state, Some(state));
    assert!(
        !adapter
            .session_dir(&target)
            .join("announcement_state.json")
            .exists()
    );
}

#[tokio::test]
async fn session_copy_does_not_clone_goal_runtime_state() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let source = create_test_info();
    let target = Info {
        id: acp::SessionId::new("goal-copy-target"),
        cwd: "/test/fork".into(),
    };
    adapter.init_session(&source, default_model_id()).await.unwrap();
    let mut tracker = crate::session::goal_tracker::GoalTracker::new();
    tracker.create_goal(
        "source-goal".into(),
        "stay in the source session".into(),
        None,
        0,
        "now".into(),
        None,
    );
    append_control_snapshot(
        &adapter,
        &source,
        &crate::session::control::SessionControlSnapshot::new(
            3,
            crate::session::behavior::BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
            tracker.snapshot().cloned(),
        ),
    )
    .await;

    adapter
        .copy_session_data(&source, &target, CopySessionOptions::default())
        .await
        .unwrap();
    let loaded = adapter.load_session_without_updates(&target).await.unwrap();
    let control = loaded.control_snapshot.expect("sanitized control snapshot");
    assert!(control.goal.is_none());
    assert_eq!(
        control.behavior.state,
        crate::session::behavior::BehaviorState::Normal
    );
}

#[tokio::test]
async fn durable_sideband_append_is_sequence_aware_and_idempotent() {
    let temp_dir = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());
    let info = create_test_info();
    adapter.init_session(&info, default_model_id()).await.unwrap();
    let id = uuid::Uuid::now_v7().to_string();
    let mut timeline = chat_state::SidebandTimeline::new(id.clone()).unwrap();
    let event = timeline
        .prepare(chat_state::SidebandEventKind::Request(
            chat_state::SidebandRequest {
                purpose: chat_state::SidebandPurpose::PermissionJudgment,
                prompt: "summarize".into(),
                source_refs: Vec::new(),
                route: chat_state::SidebandRoute {
                    model: "test-model".into(),
                    backend: "responses".into(),
                },
                initiator_ref: "t:test-session-123/0".into(),
                executor: "main".into(),
                output_schema: None,
            },
        ))
        .unwrap();
    adapter
        .append_sideband_event_durable(&info, &event)
        .await
        .unwrap();
    adapter
        .append_sideband_event_durable(&info, &event)
        .await
        .unwrap();
    let path = adapter.sideband_timeline_file(&info, &id).unwrap();
    let lines = std::fs::read_to_string(path).unwrap();
    assert_eq!(lines.lines().count(), 1);
    let stored = lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect::<Vec<chat_state::SidebandEvent>>();
    chat_state::SidebandTimeline::from_events(stored).unwrap();
}
