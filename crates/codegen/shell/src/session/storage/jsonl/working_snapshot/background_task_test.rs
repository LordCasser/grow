use crate::extensions::notification::{SessionNotification, SessionUpdate as GrowUpdate};
use std::io::{Seek, SeekFrom, Write};
use std::time::{Duration, SystemTime};
use tools::types::TaskSnapshot;

fn task_start(info: &Info, id: &str) -> SessionUpdate {
    SessionUpdate::Grow(Box::new(SessionNotification {
        session_id: info.id.clone(),
        update: GrowUpdate::TaskBackgrounded {
            task_id: id.into(),
            tool_call_id: format!("call-{id}"),
            command: "fixture".into(),
            cwd: info.cwd.clone(),
            output_file: format!("{id}.log"),
            description: Some(format!("background {id}")),
            monitor_description: None,
        },
        meta: None,
    }))
}

fn task_finish(info: &Info, id: &str) -> SessionUpdate {
    SessionUpdate::Grow(Box::new(SessionNotification {
        session_id: info.id.clone(),
        update: GrowUpdate::TaskCompleted {
            task_snapshot: TaskSnapshot {
                task_id: id.into(),
                command: "fixture".into(),
                display_command: None,
                cwd: info.cwd.clone(),
                start_time: SystemTime::UNIX_EPOCH + Duration::from_secs(10),
                end_time: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(12)),
                output: "exact task output".into(),
                output_file: format!("{id}.log").into(),
                truncated: true,
                exit_code: Some(1),
                signal: None,
                completed: true,
                kind: Default::default(),
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: None,
                goal_id: None,
                goal_definition_revision: None,
                description: Some(format!("background {id}")),
                is_backgrounded: true,
            },
        },
        meta: None,
    }))
}

fn task_facts(records: &[JournalRecord]) -> Vec<(&str, bool)> {
    records
        .iter()
        .map(|record| {
            let JournalFact::Presentation { event, .. } = &record.fact else {
                panic!("task history must contain original presentations")
            };
            let HistoryContent::Grow(fact) = &event.content else {
                panic!("task history must contain Grow facts")
            };
            match fact.as_ref() {
                history::GrowHistoryFact::TaskBackgrounded { task_id, .. } => {
                    (task_id.as_str(), false)
                }
                history::GrowHistoryFact::TaskCompleted { task_snapshot } => {
                    (task_snapshot.task_id.as_str(), true)
                }
                _ => panic!("task history must contain task facts"),
            }
        })
        .collect()
}

#[tokio::test]
async fn background_task_history_keeps_completed_lifecycle_and_open_start_at_fixed_cut() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    for update in [
        task_start(&info, "closed"),
        task_finish(&info, "closed"),
        task_start(&info, "open"),
    ] {
        storage.append_update(&info, &update).await.unwrap();
    }
    let mut timeline = chat_state::Timeline::default();
    publish(&storage, &info, &mut timeline, "task-prefix").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap().clone();
    let original = index.background_task_history().unwrap();
    assert_eq!(
        task_facts(&original),
        [("closed", false), ("closed", true), ("open", false)]
    );
    assert!(original.windows(2).all(|pair| pair[0].seq < pair[1].seq));
    let JournalFact::Presentation { event, .. } = &original[1].fact else {
        unreachable!()
    };
    let HistoryContent::Grow(fact) = &event.content else {
        unreachable!()
    };
    let history::GrowHistoryFact::TaskCompleted { task_snapshot } = fact.as_ref() else {
        unreachable!()
    };
    assert_eq!(task_snapshot.output, "exact task output");
    assert!(task_snapshot.truncated);
    assert_eq!(task_snapshot.exit_code, Some(1));
    assert_eq!(task_snapshot.duration_secs(), 2.0);
    assert_eq!(
        loaded.history.sources.len(),
        1,
        "hydrate only retains the open start"
    );
    drop(loaded);

    // A later publication extends the same redb tables. The older handle must
    // still select its own physical and branch frontier.
    storage
        .append_update(&info, &task_finish(&info, "open"))
        .await
        .unwrap();
    storage
        .append_update(&info, &task_start(&info, "later"))
        .await
        .unwrap();
    publish(&storage, &info, &mut timeline, "task-successor").await;
    let successor = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let successor = successor.history.indexed.as_ref().unwrap();
    assert_eq!(index.manifest.generation, successor.manifest.generation);
    assert_eq!(
        index
            .background_task_history()
            .unwrap()
            .iter()
            .map(|record| (record.seq, record.digest))
            .collect::<Vec<_>>(),
        original
            .iter()
            .map(|record| (record.seq, record.digest))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        task_facts(&successor.background_task_history().unwrap()),
        [
            ("closed", false),
            ("closed", true),
            ("open", false),
            ("open", true),
            ("later", false),
        ]
    );
}

#[tokio::test]
async fn background_task_history_excludes_unpublished_tail() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .append_update(&info, &task_start(&info, "prefix"))
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    publish(&storage, &info, &mut timeline, "prefix").await;
    let directory = storage.open_session(&info).unwrap().directory_handle();
    let published = read_manifest(&directory).unwrap().unwrap();
    storage
        .append_update(&info, &task_finish(&info, "prefix"))
        .await
        .unwrap();
    storage
        .append_update(&info, &task_start(&info, "tail"))
        .await
        .unwrap();
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    assert_eq!(index.manifest.bytes, published.bytes);
    assert_eq!(
        task_facts(&index.background_task_history().unwrap()),
        [("prefix", false)]
    );
    assert_eq!(
        loaded
            .history
            .positions
            .iter()
            .filter(|position| **position >= index.records())
            .count(),
        2
    );
}

#[tokio::test]
async fn background_task_history_hides_rewound_tasks_and_preserves_older_branch_cut() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline =
        chat_state::Timeline::from_seed(vec![ConversationItem::system("fixed seed")]).unwrap();
    storage
        .append_timeline_event(&info, &timeline.events()[0])
        .await
        .unwrap();
    for (id, prompt) in [(10, 0), (11, 1)] {
        let start = timeline
            .record(turn_start(
                TurnId(id),
                "task prompt",
                timeline.surface_len(),
                prompt,
            ))
            .unwrap();
        let mut input = ConversationItem::user("task prompt");
        input.set_prompt_index(prompt);
        let message = timeline.append(input, MessageCause::User).unwrap();
        let finish = timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: TurnId(id),
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
        storage
            .append_update(
                &info,
                &user_chunk(
                    &info,
                    "task prompt",
                    serde_json::json!({"messageId": format!("echo-{id}"), "promptIndex": prompt}),
                    None,
                ),
            )
            .await
            .unwrap();
        let task = if prompt == 0 { "selected" } else { "discarded" };
        storage
            .append_update(&info, &task_start(&info, task))
            .await
            .unwrap();
        storage
            .append_update(&info, &task_finish(&info, task))
            .await
            .unwrap();
    }
    publish(&storage, &info, &mut timeline, "before-rewind").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let original = loaded.history.indexed.as_ref().unwrap().clone();
    drop(loaded);
    let rewind = timeline
        .replace_all(timeline.rewind_surface(1).unwrap(), MessageCause::Rewind)
        .unwrap();
    storage
        .append_timeline_event_durable(&info, &rewind)
        .await
        .unwrap();
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    assert_eq!(
        task_facts(
            &loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .background_task_history()
                .unwrap()
        ),
        [("selected", false), ("selected", true)]
    );
    assert_eq!(
        task_facts(&original.background_task_history().unwrap()),
        [
            ("selected", false),
            ("selected", true),
            ("discarded", false),
            ("discarded", true)
        ]
    );
}

#[tokio::test]
async fn background_task_history_does_not_read_unrelated_message_body() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline =
        super::super::super::tests::response_projection_production_timeline_with_items(
            "unrelated-response",
            1,
            vec![ConversationItem::assistant("unrelated response body")],
            0,
        );
    let response_position = timeline.events().last().unwrap().seq.get();
    for event in timeline.events() {
        storage.append_timeline_event(&info, event).await.unwrap();
    }
    storage
        .append_update(&info, &task_start(&info, "task"))
        .await
        .unwrap();
    publish(&storage, &info, &mut timeline, "task-only-read").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let unrelated = read_indexed_domain_reference(
        &index.directory,
        &index.manifest,
        EventSeq::new(response_position),
    )
    .unwrap()
    .unwrap();
    let mut journal = index
        .directory
        .open_read_write_existing(OsStr::new(TIMELINE_FILE), "journal")
        .unwrap();
    journal.seek(SeekFrom::Start(unrelated.offset)).unwrap();
    journal.write_all(b"X").unwrap();
    assert_eq!(
        task_facts(&index.background_task_history().unwrap()),
        [("task", false)]
    );
}

#[tokio::test]
async fn background_task_history_rejects_replaced_or_corrupt_source() {
    for replace in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        storage
            .append_update(&info, &task_start(&info, "task"))
            .await
            .unwrap();
        let mut timeline = chat_state::Timeline::default();
        publish(&storage, &info, &mut timeline, "source-check").await;
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let index = loaded.history.indexed.as_ref().unwrap();
        let path = index.directory.display_path().join(TIMELINE_FILE);
        if replace {
            let original = std::fs::read(&path).unwrap();
            std::fs::rename(&path, path.with_extension("original")).unwrap();
            std::fs::write(&path, original).unwrap();
        } else {
            let mut journal = index
                .directory
                .open_read_write_existing(OsStr::new(TIMELINE_FILE), "journal")
                .unwrap();
            journal.write_all(b"X").unwrap();
        }
        assert_eq!(
            index.background_task_history().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}

#[tokio::test]
async fn background_task_history_rebuilds_an_outdated_derived_schema_without_rewriting_journal() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    storage
        .append_update(&info, &task_start(&info, "closed"))
        .await
        .unwrap();
    storage
        .append_update(&info, &task_finish(&info, "closed"))
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    publish(&storage, &info, &mut timeline, "schema-rebuild").await;
    let directory = storage.open_session(&info).unwrap().directory_handle();
    let mut outdated = read_manifest(&directory).unwrap().unwrap();
    let generation = outdated.generation;
    outdated.version = 51;
    assert!(write_manifest(&directory, &outdated).unwrap());
    assert!(read_manifest(&directory).unwrap().is_none());
    let path = directory.display_path().join(TIMELINE_FILE);
    let journal = std::fs::read(&path).unwrap();
    drop(storage);
    let (storage, _) = fixture(root.path());
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    assert_eq!(index.manifest.version, VERSION);
    assert_ne!(index.manifest.generation, generation);
    assert_eq!(
        task_facts(&index.background_task_history().unwrap()),
        [("closed", false), ("closed", true)]
    );
    assert_eq!(std::fs::read(path).unwrap(), journal);
}

#[tokio::test]
async fn background_task_history_rejects_wrong_class_sequence_and_cut_reference() {
    for corruption in ["class", "sequence", "bytes", "position"] {
        let root = tempfile::tempdir().unwrap();
        let (storage, info) = fixture(root.path());
        storage
            .init_session(&info, default_model_id())
            .await
            .unwrap();
        storage
            .append_update(&info, &task_start(&info, "task"))
            .await
            .unwrap();
        let recap = SessionUpdate::Grow(Box::new(SessionNotification {
            session_id: info.id.clone(),
            update: GrowUpdate::SessionRecap {
                summary: "unrelated".into(),
                auto: false,
                away_period_id: None,
            },
            meta: None,
        }));
        storage.append_update(&info, &recap).await.unwrap();
        let mut timeline = chat_state::Timeline::default();
        publish(&storage, &info, &mut timeline, "reference-check").await;
        let loaded = storage
            .load_session_for_write_with_indexed_history(&info)
            .await
            .unwrap();
        let index = loaded.history.indexed.as_ref().unwrap();
        let directory = index.directory.clone();
        let manifest = (*index.manifest).clone();
        let mut reference = index
            .tools
            .background_task_references_before(index.records())
            .unwrap()[0];
        let mut key = reference.position;
        match corruption {
            "class" => {
                reference = index.tools.display_at(1).unwrap();
                key = reference.position;
            }
            "sequence" => {
                reference.position = 1;
                key = 1;
            }
            "bytes" => reference.record.offset = manifest.bytes,
            "position" => reference.position += 1,
            _ => unreachable!(),
        }
        drop(loaded);
        drop(storage);
        // Mutate only the disposable fixture's derived table, keeping the source
        // record valid for the class/seq checks and preserving file identity.
        {
            let db = redb::Database::open(
                directory
                    .display_path()
                    .join(super::super::super::tool_history::name(manifest.generation)),
            )
            .unwrap();
            let tx = db.begin_write().unwrap();
            {
                let mut rows = tx
                    .open_table(redb::TableDefinition::<u64, &[u8]>::new(
                        "background_task_display",
                    ))
                    .unwrap();
                rows.insert(key, serde_json::to_vec(&reference).unwrap().as_slice())
                    .unwrap();
            }
            tx.commit().unwrap();
        }
        let index = open_history(directory, manifest, None).unwrap();
        assert_eq!(
            index.background_task_history().unwrap_err().kind(),
            io::ErrorKind::InvalidData,
            "{corruption}"
        );
    }
}
