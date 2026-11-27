//! Both comparison binaries read the very same published fixture.
use super::*;
use crate::session::{
    history::SeedPresentation, info::Info, persistence::Summary, storage::SessionUpdate,
};
use crate::test_support::lsp_runtime::ctx_with_toggle;
use sampling_types::ConversationItem;
use std::collections::HashMap;

fn historical_observation(scope: &str) -> TimelineEventKind {
    TimelineEventKind::Observation(chat_state::ObservationEvent {
        scope: scope.into(),
        name: "benchmark".into(),
        turn: None,
        step: None,
        data: Some(serde_json::Value::String("x".repeat(256 * 1024))),
    })
}

fn inherited_content(
    id: &acp::SessionId,
    blocks: usize,
    block_bytes: usize,
) -> (Vec<ConversationItem>, SeedPresentation) {
    let mut surface = vec![ConversationItem::system("source system".to_owned())];
    let mut updates = Vec::new();
    for i in 0..blocks {
        let text = format!("row-{i:03} {}", "a".repeat(block_bytes));
        let chunk =
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text.clone())));
        let source_meta = serde_json::json!({"grow/historyMessageEvent": surface.len(), "grow/historyMessageItem": 0});
        updates.push(SessionUpdate::Acp(Box::new(
            acp::SessionNotification::new(id.clone(), acp::SessionUpdate::AgentMessageChunk(chunk))
                .meta(source_meta.as_object().cloned()),
        )));
        surface.push(ConversationItem::assistant(text));
    }
    let ids = Timeline::from_seed(surface.clone())
        .unwrap()
        .surface_ids()
        .to_vec();
    (surface, SeedPresentation::for_resume(ids, updates))
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "prepare an isolated, published fixture for the indexed resume measurement"]
async fn create_published_resume_bench_fixture() {
    let home = PathBuf::from(std::env::var_os("GROW_RESUME_INDEXED_FIXTURE").unwrap());
    std::fs::create_dir(&home).unwrap();
    let scenario = std::env::var("GROW_RESUME_INDEXED_SCENARIO").unwrap();
    assert!(matches!(
        scenario.as_str(),
        "parent" | "small-parent" | "source-diagnostic" | "source-content"
    ));
    let storage = JsonlStorageAdapter::with_root(home.clone());
    let mut parent_facts = Vec::new();
    let mut infos = Vec::new();
    for index in 0..4 {
        let id = format!("source-{index}");
        let info = Info {
            id: acp::SessionId::new(id.clone()),
            cwd: CWD.into(),
        };
        let mut summary =
            Summary::new(&info, crate::session::persistence::default_model_id()).unwrap();
        summary.session_kind = Some("subagent".into());
        summary.parent_session_id = Some(PARENT.into());
        let mut spawn = recovery_spawn(&id, &id);
        spawn.context_source = chat_state::SubagentContextSource::New;
        let seed = chat_state::SubagentSeedEvent {
            parent_timeline_id: PARENT.into(),
            parent_spawn_seq: parent_facts.len() as u64,
            subagent_id: id.clone(),
            security_parent_session_id: PARENT.into(),
            context_source: spawn.context_source,
            source_ref: None,
            normalized: false,
        };
        parent_facts.push(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            Box::new(spawn),
        )));
        let (blocks, bytes) = match (scenario.as_str(), index) {
            ("source-content", 0) => (128, 256 * 1024),
            ("source-diagnostic", 0) => (1, 64 * 1024),
            _ => (1, 64),
        };
        let (surface, history) = inherited_content(&info.id, blocks, bytes);
        let mut facts = vec![TimelineEventKind::SubagentSeed(seed)];
        if scenario == "source-diagnostic" && index == 0 {
            facts.extend((0..128).map(|_| historical_observation("shell")));
        }
        let result = SubagentResult {
            success: true,
            subagent_id: id.clone(),
            child_session_id: id.clone(),
            ..Default::default()
        };
        facts.push(TimelineEventKind::SubagentResult(subagent_result_fact(
            &result, None,
        )));
        let (_, timeline, _) = storage
            .init_session_with_summary(&info, summary, surface, Default::default(), history, facts)
            .await
            .unwrap();
        let seq = timeline.last_seq().unwrap().get();
        parent_facts.push(TimelineEventKind::Subagent(SubagentEvent::Ended(
            chat_state::SubagentTerminalEvent {
                subagent_id: id.clone(),
                child_session_id: id.clone(),
                outcome: chat_state::SubagentOutcome::Completed,
                duration_ms: 0,
                tool_calls: 0,
                turns: 0,
                tokens_used: 0,
                error: None,
                result_ref: Some(chat_state::TimelineRangeRef {
                    timeline_id: id,
                    first_seq: seq,
                    last_seq: seq,
                }),
                snapshot_ref: None,
            },
        )));
        infos.push(info);
    }
    if scenario == "parent" {
        parent_facts.extend((0..128).map(|_| historical_observation("benchmark")));
    }
    let info = Info {
        id: acp::SessionId::new(PARENT.to_owned()),
        cwd: CWD.into(),
    };
    let summary = Summary::new(&info, crate::session::persistence::default_model_id()).unwrap();
    storage
        .init_session_with_summary(
            &info,
            summary,
            vec![],
            Default::default(),
            Default::default(),
            parent_facts,
        )
        .await
        .unwrap();
    infos.push(info);
    drop(storage);
    for info in infos {
        let dir = directory(&home, info.id.0.as_ref());
        let bytes = dir.join("timeline.jsonl").metadata().unwrap().len();
        assert!(dir.join("working-history.json").is_file());
        let storage = JsonlStorageAdapter::with_root(home.clone());
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        assert_eq!(loaded.history.bytes, bytes);
        assert_eq!(
            loaded.history.indexed.as_ref().unwrap().records(),
            loaded.history.record_count()
        );
        println!("indexed id={} journal_bytes={bytes}", info.id.0);
    }
}

async fn hold_parent_writer_with_unpublished_tail(
    home: &Path,
) -> (
    JsonlStorageAdapter,
    crate::session::storage::jsonl::WriterJournal,
    chat_state::ChatStateHandle,
) {
    use crate::session::storage::StorageAdapter as _;
    let info = Info {
        id: acp::SessionId::new(PARENT.to_owned()),
        cwd: CWD.into(),
    };
    let dir = directory(home, PARENT);
    let manifest = dir.join("working-history.json");
    let journal = dir.join("timeline.jsonl");
    let published = std::fs::read(&manifest).unwrap();
    let prefix_bytes = journal.metadata().unwrap().len();
    let storage = JsonlStorageAdapter::with_root(home.to_owned());
    let mut loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    assert!(loaded.history.indexed.is_some());
    let scope = std::env::var("GROW_RESUME_BENCH_TAIL_SCOPE").unwrap_or_else(|_| "shell".into());
    let event = loaded
        .timeline
        .record(TimelineEventKind::Observation(
            chat_state::ObservationEvent {
                scope: scope.clone(),
                name: "online benchmark tail".into(),
                turn: None,
                step: None,
                data: None,
            },
        ))
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &event)
        .await
        .unwrap();
    let end = journal.metadata().unwrap().len();
    assert!(end > prefix_bytes);
    assert_eq!(std::fs::read(&manifest).unwrap(), published);
    println!(
        "live_writer=true scope={scope} published_bytes={prefix_bytes} unpublished_bytes={}",
        end - prefix_bytes
    );
    let journal = storage.writer_journal(&info).unwrap();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let actor = chat_state::ChatStateActor::spawn_from_validated_timeline(
        loaded.timeline,
        test_sampling_config("bench"),
        None,
        Box::new(chat_state::NullTimelinePersistence),
        event_tx,
        Default::default(),
    )
    .await
    .unwrap();
    (storage, journal, actor)
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "measure real resolver and inherited material on a prebuilt indexed fixture"]
async fn measure_published_resume_material() {
    let width: usize = std::env::var(WIDTH).unwrap().parse().unwrap();
    let home = PathBuf::from(std::env::var_os("GROW_RESUME_INDEXED_FIXTURE").unwrap());
    // The marker lets the external inode counter exclude setup of the live writer.
    let parent_writer = if let Some(marker) = std::env::var_os("GROW_RESUME_BENCH_LIVE_TAIL_MARKER")
    {
        let writer = hold_parent_writer_with_unpublished_tail(&home).await;
        std::fs::File::create_new(marker).unwrap();
        Some(writer)
    } else {
        None
    };
    let ctx = ctx_with_toggle(HashMap::new());
    let requests = (0..width).map(|i| {
        let ctx = &ctx;
        let home = home.clone();
        let owner = parent_writer
            .as_ref()
            .map(|(_, journal, actor)| (journal.clone(), actor.clone()));
        async move {
            let id = format!("source-{i}");
            let source =
                durable_resume_source_for(home, id.clone(), PARENT.into(), PARENT.into(), owner)
                    .await
                    .unwrap();
            assert_eq!(source.data.child_session_id, id);
            let mut request = bootstrap_test_request(false);
            request.resume_from = Some(id);
            match bootstrap_initial_context(&request, Some(source.initial), ctx, 128_000).await {
                BootstrapInitialContext::Ready(context) => context,
                BootstrapInitialContext::Abort(message) => panic!("{message}"),
            }
        }
    });
    let before = crate::session::signals::sample_rss_bytes();
    #[cfg(target_os = "macos")]
    let disk = super::physical_read_bytes();
    let start = std::time::Instant::now();
    let materials = futures_util::future::join_all(requests).await;
    println!(
        "width={width} elapsed_ms={} peak_rss_bytes={} baseline_hwm_bytes={before}",
        start.elapsed().as_millis(),
        crate::session::signals::sample_rss_bytes()
    );
    #[cfg(target_os = "macos")]
    println!(
        "physical_read_bytes={}",
        super::physical_read_bytes() - disk
    );
    for material in materials {
        assert!(matches!(material.source, InitialContextSource::Resumed));
        let mut surface = blake3::Hasher::new();
        serde_json::to_writer(&mut surface, &material.conversation).unwrap();
        let mut presentation = blake3::Hasher::new();
        serde_json::to_writer(&mut presentation, &material.presentation.updates).unwrap();
        let mut text_bytes = 0;
        for update in &material.presentation.updates {
            let SessionUpdate::Acp(update) = update else {
                panic!("unexpected Grow row")
            };
            let acp::SessionUpdate::AgentMessageChunk(chunk) = &update.update else {
                panic!("unexpected ACP row")
            };
            let acp::ContentBlock::Text(text) = &chunk.content else {
                panic!("unexpected content")
            };
            text_bytes += text.text.len();
        }
        assert_eq!(
            material.conversation.len(),
            material.presentation.updates.len() + 1
        );
        assert!(text_bytes > 0);
        println!(
            "items={} rows={} visible_text_bytes={text_bytes} surface_digest={} display_digest={}",
            material.conversation.len(),
            material.presentation.updates.len(),
            surface.finalize(),
            presentation.finalize()
        );
    }
}

/// Check the actual seed facts separately from transient projector transport metadata.
#[tokio::test(flavor = "current_thread")]
#[ignore = "compare retained seed material for an existing benchmark fixture"]
async fn compare_published_resume_seed_facts() {
    let home = PathBuf::from(std::env::var_os("GROW_RESUME_INDEXED_FIXTURE").unwrap());
    let storage = JsonlStorageAdapter::with_root(home.clone());
    for i in 0..4 {
        let id = format!("source-{i}");
        let opened = storage.open_session_by_id(&id).unwrap().unwrap();
        let dense = opened.validated_timeline(&id).unwrap();
        let rows = dense
            .history
            .prepare(&opened.summary().info.id, &dense.timeline, None)
            .unwrap()
            .updates;
        let expected = SeedPresentation::for_resume(dense.timeline.surface_ids().to_vec(), rows);
        let recovered =
            durable_resume_source_for(home.clone(), id.clone(), PARENT.into(), PARENT.into(), None)
                .await
                .unwrap()
                .initial
                .unwrap();
        // A history cursor belongs to the source projection; into_facts removes
        // it before inserting the inherited row into a different session.
        let transport_without_cursor = |updates: &[SessionUpdate]| {
            let mut rows = serde_json::to_value(updates).unwrap();
            for row in rows.as_array_mut().unwrap() {
                row.pointer_mut("/params/_meta")
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove("grow/historyCursor");
            }
            rows
        };
        assert!(
            transport_without_cursor(&expected.updates)
                == transport_without_cursor(&recovered.presentation.updates),
            "{id} display rows differ outside the source cursor"
        );
        assert!(
            serde_json::to_value(dense.timeline.surface()).unwrap()
                == serde_json::to_value(&recovered.conversation).unwrap(),
            "{id} model Surface differs"
        );
        let seed = Timeline::from_seed(recovered.conversation).unwrap();
        let expected = serde_json::to_value(expected.into_facts(&seed).unwrap()).unwrap();
        let actual =
            serde_json::to_value(recovered.presentation.into_facts(&seed).unwrap()).unwrap();
        assert!(expected == actual, "{id} persisted seed facts differ");
    }
}
