use chat_state::timeline::ExecutionHistory;
use chat_state::{ImageProjectionEvent, ImageShadow, ImageShadowSource};
use sampling_types::conversation::{UNSUPPORTED_IMAGE_REPLACEMENT, conversation_image_groups};
use sampling_types::{ContentPart, ConversationItem, ModelImageInputKey};

const BRANCH_TEST: &str = "session::storage::jsonl::working_snapshot::tests::branch_publication::process_exit_around_branch_publication_preserves_exact_cut";

async fn append_turn(
    storage: &JsonlStorageAdapter,
    info: &Info,
    timeline: &mut chat_state::Timeline,
    id: TurnId,
    index: usize,
    text: &str,
) {
    let start = timeline
        .record(turn_start(id, text, timeline.surface_len(), index))
        .unwrap();
    let mut input = ConversationItem::user(text);
    input.set_prompt_index(index);
    let message = timeline.append(input, MessageCause::User).unwrap();
    let finish = timeline
        .record(TimelineEventKind::Turn(TurnEvent::Ended {
            id,
            outcome: "completed".into(),
            duration_ms: 1,
            tool_count: 0,
            terminal: chat_state::TurnTerminal {
                source: chat_state::TurnTerminalSource::Host,
                stop_reason: "end_turn".into(),
                completion_kind: "completed".into(),
            },
            cancellation_category: None,
            details: None,
        }))
        .unwrap();
    for event in [&start, &message, &finish] {
        storage.append_timeline_event(&info, event).await.unwrap();
    }
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text))).meta(
        Some(acp::Meta::from_iter([
            ("messageId".into(), format!("echo-{}", id.0).into()),
            ("promptIndex".into(), index.into()),
        ])),
    );
    let echo =
        acp::SessionNotification::new(info.id.clone(), acp::SessionUpdate::UserMessageChunk(chunk));
    storage.append_acp_event_exact(info, &echo).await.unwrap();
}

async fn base(root: &Path, image: bool) -> (chat_state::Timeline, u64) {
    let (storage, info) = fixture(root);
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let seed = if image {
        ConversationItem::user_with_parts(vec![ContentPart::Image {
            url: "data:image/png;base64,original".into(),
            description: None,
        }])
    } else {
        ConversationItem::system("fixed seed")
    };
    let mut timeline = chat_state::Timeline::from_seed(vec![seed]).unwrap();
    storage
        .append_timeline_event(&info, &timeline.events()[0])
        .await
        .unwrap();
    if !image {
        append_turn(
            &storage,
            &info,
            &mut timeline,
            TurnId(10),
            0,
            "selected user prompt",
        )
        .await;
        append_turn(
            &storage,
            &info,
            &mut timeline,
            TurnId(11),
            1,
            "discarded user prompt",
        )
        .await;
    }
    let force = timeline
        .record(observation("publish branch source"))
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &force)
        .await
        .unwrap();
    let directory = storage.open_session(&info).unwrap().directory_handle();
    let before = read_manifest(&directory).unwrap().unwrap().bytes;
    (timeline, before)
}

fn transform(timeline: &mut chat_state::Timeline, image: bool) -> TimelineEvent {
    if image {
        let group = conversation_image_groups(timeline.surface()).remove(0);
        timeline
            .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
                trigger_runtime: ModelImageInputKey::new("mock", "messages", "endpoint"),
                source_revision: timeline.surface_revision(),
                shadows: vec![ImageShadow {
                    source: timeline.surface_ids()[0],
                    fingerprint: group.fingerprint,
                    image_count: 1,
                    replacement: UNSUPPORTED_IMAGE_REPLACEMENT.into(),
                    provenance: ImageShadowSource::UnsupportedModel,
                }],
                tool_calls: vec![],
            }))
            .unwrap()
    } else {
        timeline
            .replace_all(timeline.rewind_surface(1).unwrap(), MessageCause::Rewind)
            .unwrap()
    }
}

#[tokio::test(flavor = "current_thread")]
async fn process_exit_around_branch_publication_preserves_exact_cut() {
    if let Some(root) = std::env::var_os(CRASH_HOME_ENV) {
        let root = Path::new(&root);
        let image = std::env::var("GROW_TEST_BRANCH_KIND").as_deref() == Ok("image");
        let (storage, info) = fixture(root);
        let opened = storage.open_session(&info).unwrap();
        let mut dense = opened
            .validated_timeline(info.id.0.as_ref())
            .unwrap()
            .timeline;
        let event = transform(&mut dense, image);
        storage
            .append_timeline_event_durable(&info, &event)
            .await
            .unwrap();
        panic!("branch publication crash probe was not reached");
    }

    for image in [false, true] {
        for (stage, before_commit) in [("advance-before", true), ("advance-after", false)] {
            let root = tempfile::tempdir().unwrap();
            let (mut dense, before) = base(root.path(), image).await;
            let expected = transform(&mut dense, image);
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", BRANCH_TEST, "--nocapture"])
                .env(CRASH_HOME_ENV, root.path())
                .env(CRASH_STAGE_ENV, stage)
                .env(
                    "GROW_TEST_BRANCH_KIND",
                    if image { "image" } else { "rewind" },
                )
                .status()
                .unwrap();
            assert_eq!(status.code(), Some(73));

            let (storage, info) = fixture(root.path());
            let directory = storage.open_session(&info).unwrap().directory_handle();
            let manifest = read_manifest(&directory).unwrap().unwrap();
            assert_eq!(manifest.bytes == before, before_commit);
            assert_eq!(
                manifest.domain_events,
                expected.seq.get() + u64::from(!before_commit)
            );

            // A partially written successor must not change the selected cut.
            use std::io::Write;
            let path = directory.display_path().join(TIMELINE_FILE);
            std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .unwrap()
                .write_all(b"{\"torn\"")
                .unwrap();

            let recovered = storage
                .resume_session_with_indexed_history(&info)
                .await
                .unwrap();
            assert!(recovered.timeline.has_surface_checkpoint());
            assert_eq!(recovered.timeline.surface_ids(), dense.surface_ids());
            assert_eq!(
                recovered.timeline.surface_revision(),
                dense.surface_revision()
            );
            assert_eq!(recovered.timeline.next_seq(), dense.next_seq());
            assert_eq!(
                serde_json::to_value(recovered.timeline.surface()).unwrap(),
                serde_json::to_value(dense.surface()).unwrap(),
            );
            let published = recovered.history.indexed.as_ref().unwrap();
            assert_eq!(published.manifest.retained_records, 0);
            assert_eq!(published.manifest.domain_events, expected.seq.get() + 1);
            let selected = recovered
                .timeline
                .exact_event(expected.seq)
                .unwrap()
                .unwrap();
            let journal_before_retry = std::fs::metadata(&path).unwrap().len();
            storage
                .append_timeline_event_durable(&info, &selected)
                .await
                .unwrap();
            assert!(std::fs::metadata(&path).unwrap().len() < journal_before_retry);
            assert_eq!(
                std::fs::metadata(&path).unwrap().len(),
                published.manifest.bytes
            );
            if image {
                let original = recovered
                    .timeline
                    .exact_event(EventSeq::new(0))
                    .unwrap()
                    .unwrap();
                assert!(!conversation_image_groups(&original.messages().unwrap().items).is_empty());
            } else {
                let cut = published.tools.cut(published.clone()).unwrap();
                let prompts = cut.prompts_before(10).unwrap();
                assert_eq!(
                    prompts
                        .iter()
                        .map(|prompt| prompt.text.as_str())
                        .collect::<Vec<_>>(),
                    ["selected user prompt"]
                );
                assert!(!cut.has_prompt(1).unwrap());
                let prepared = recovered
                    .history
                    .prepare_indexed(&info.id, &recovered.timeline)
                    .unwrap();
                let pages = recovered.history.indexed_pages(prepared).unwrap().unwrap();
                let (_, newest) = pages
                    .page_before(&info.id, history::PagePosition::newest())
                    .unwrap();
                let shown = serde_json::to_string(&newest).unwrap();
                assert!(shown.contains("selected user prompt"));
                assert!(!shown.contains("discarded user prompt"));
            }
            drop(recovered);
            drop(storage);
            let (storage, _) = fixture(root.path());
            let reopened = storage
                .resume_session_with_indexed_history(&info)
                .await
                .unwrap();
            assert!(reopened.timeline.has_surface_checkpoint());
            assert!(reopened.timeline.events().is_empty());
        }
    }
}

#[tokio::test]
async fn rewound_prompt_reuse_preserves_the_discarded_identity_and_pages_the_new_branch() {
    use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};

    let root = tempfile::tempdir().unwrap();
    let (mut dense, _) = base(root.path(), false).await;
    let (storage, info) = fixture(root.path());
    let rewound = transform(&mut dense, false);
    storage
        .append_timeline_event_durable(&info, &rewound)
        .await
        .unwrap();
    storage
        .append_update_durable_commit_aware(
            &info,
            &SessionUpdate::Grow(Box::new(SessionNotification {
                session_id: info.id.clone(),
                update: GrowUpdate::RewindMarker {
                    target_prompt_index: 1,
                    created_at: "fixture".into(),
                },
                meta: None,
            })),
        )
        .await
        .unwrap();
    drop(storage);

    let (storage, _) = fixture(root.path());
    let mut recovered = storage
        .resume_session_with_indexed_history(&info)
        .await
        .unwrap();
    assert_eq!(recovered.timeline.history_branch(), Some(rewound.seq));
    assert_eq!(recovered.timeline.next_prompt_index(), 1);
    assert!(matches!(
        recovered
            .timeline
            .prepare(turn_start(TurnId(11), "old owner", 2, 1)),
        Err(chat_state::TimelineError::TurnAlreadySeen(TurnId(11)))
    ));
    append_turn(
        &storage,
        &info,
        &mut recovered.timeline,
        TurnId(12),
        1,
        "replacement user prompt",
    )
    .await;
    let publish = recovered
        .timeline
        .record(observation("publish replacement turn"))
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &publish)
        .await
        .unwrap();
    drop(recovered);
    drop(storage);

    let (storage, _) = fixture(root.path());
    let recovered = storage
        .resume_session_with_indexed_history(&info)
        .await
        .unwrap();
    assert_eq!(recovered.timeline.next_prompt_index(), 2);
    let index = recovered.history.indexed.as_ref().unwrap();
    let cut = index.tools.cut(index.clone()).unwrap();
    assert_eq!(
        cut.prompts_before(10)
            .unwrap()
            .into_iter()
            .map(|prompt| prompt.text)
            .collect::<Vec<_>>(),
        ["replacement user prompt", "selected user prompt"]
    );
    let prepared = recovered
        .history
        .prepare_indexed(&info.id, &recovered.timeline)
        .unwrap();
    let pages = recovered.history.indexed_pages(prepared).unwrap().unwrap();
    let (older, newest) = pages
        .page_before(&info.id, history::PagePosition::newest())
        .unwrap();
    assert!(
        serde_json::to_string(&newest)
            .unwrap()
            .contains("replacement user prompt")
    );
    let (_, old) = pages
        .page_before(&info.id, older.expect("selected older window"))
        .unwrap();
    let shown = serde_json::to_string(&old).unwrap();
    assert!(shown.contains("selected user prompt"));
    assert!(!shown.contains("discarded user prompt"));
    assert!(
        recovered
            .timeline
            .exact_event(EventSeq::new(5))
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn image_checkpoint_keeps_linked_provider_description_auditable() {
    use chat_state::{
        SidebandAssemblyManifest, SidebandAttempt, SidebandBudgetPolicy, SidebandEnd,
        SidebandEventKind, SidebandOutcome, SidebandPurpose, SidebandRequest, SidebandResult,
        SidebandRoute, SidebandSpawnEvent, SidebandTimeline, SidebandUsage, TimelineRangeRef,
    };

    let root = tempfile::tempdir().unwrap();
    let (mut timeline, _) = base(root.path(), true).await;
    let (storage, info) = fixture(root.path());
    let source = timeline.surface_ids()[0];
    let source_ref = TimelineRangeRef {
        timeline_id: info.id.to_string(),
        first_seq: source.event.get(),
        last_seq: source.event.get(),
    };
    let id = uuid::Uuid::now_v7().to_string();
    let purpose = SidebandPurpose::ImageDescription;
    let spawn = timeline
        .record(TimelineEventKind::Sideband(SidebandSpawnEvent {
            sideband_id: id.clone(),
            purpose,
            source_refs: vec![source_ref.clone()],
        }))
        .unwrap();
    storage.append_timeline_event(&info, &spawn).await.unwrap();
    let mut description = SidebandTimeline::new(id.clone()).unwrap();
    let raw = "neutral visual description";
    for kind in [
        SidebandEventKind::Request(SidebandRequest {
            purpose,
            prompt: "describe".into(),
            source_refs: vec![source_ref.clone()],
            budget_policy: SidebandBudgetPolicy {
                max_attempts: 1,
                max_input_tokens_per_attempt: 100,
                max_output_tokens_per_attempt: None,
            },
            route: SidebandRoute {
                model: "mock/mock".into(),
                backend: sampling_types::ApiBackend::Responses,
            },
            initiator_ref: format!("t:{}/sideband:{id}", info.id),
            executor: "main".into(),
            output_schema: None,
        }),
        SidebandEventKind::Attempt(SidebandAttempt {
            attempt_no: 1,
            input_refs: vec![source_ref.clone()],
            assembly_manifest: SidebandAssemblyManifest {
                strategy: "image-group".into(),
                strategy_version: 1,
                source_revision: Some(timeline.surface_revision()),
                context_surface_ids: vec![source],
                selected_surface_ids: vec![source],
                materialized_input_tokens: 10,
                max_output_tokens: None,
            },
            feedback: None,
        }),
        SidebandEventKind::Result(SidebandResult {
            raw_output: raw.into(),
            structured_output: None,
            usage: SidebandUsage::default(),
            finish: "stop".into(),
            source_event_seqs: [0, 1],
            evidence_refs: vec![source_ref],
        }),
        SidebandEventKind::End(SidebandEnd {
            outcome: SidebandOutcome::Completed,
            error: None,
        }),
    ] {
        let event = description.prepare(kind).unwrap();
        description.accept(event.clone()).unwrap();
        storage
            .append_sideband_event_durable(&info, &event)
            .await
            .unwrap();
    }
    let group = conversation_image_groups(timeline.surface()).remove(0);
    let expected_description = crate::session::image_describe::render_image_description_block(raw);
    let projection = timeline
        .record(TimelineEventKind::ImageProjection(ImageProjectionEvent {
            trigger_runtime: ModelImageInputKey::new("mock", "messages", "endpoint"),
            source_revision: timeline.surface_revision(),
            shadows: vec![ImageShadow {
                source,
                fingerprint: group.fingerprint,
                image_count: 1,
                replacement: expected_description.clone(),
                provenance: ImageShadowSource::Description {
                    result_ref: TimelineRangeRef {
                        timeline_id: id.clone(),
                        first_seq: 2,
                        last_seq: 2,
                    },
                },
            }],
            tool_calls: vec![],
        }))
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &projection)
        .await
        .unwrap();
    drop(storage);

    let (storage, _) = fixture(root.path());
    let recovered = storage
        .resume_session_with_indexed_history(&info)
        .await
        .unwrap();
    assert!(recovered.timeline.has_surface_checkpoint());
    assert!(recovered.timeline.events().is_empty());
    assert_eq!(recovered.timeline.surface_ids(), timeline.surface_ids());
    assert_eq!(
        sampling_types::conversation::item_image_description(&recovered.timeline.surface()[0]),
        Some(expected_description.as_str())
    );
    assert_eq!(
        conversation_image_groups(recovered.timeline.surface()).len(),
        1
    );
    let opened = storage.open_session(&info).unwrap();
    let audited = opened.validated_timeline(info.id.0.as_ref()).unwrap();
    assert_eq!(audited.sidebands[&id].len(), 4);
    assert_eq!(audited.timeline.surface_ids(), timeline.surface_ids());
}
