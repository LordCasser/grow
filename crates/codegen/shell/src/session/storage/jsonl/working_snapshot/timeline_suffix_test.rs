#[tokio::test(flavor = "current_thread")]
async fn suffix_overlay_keeps_a_same_generation_tail_on_the_old_byte_cut() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    for (text, id) in [("one", "p1"), ("two", "p2"), ("three", "p3")] {
        storage
            .append_update(
                &info,
                &user_chunk(&info, text, serde_json::json!({"messageId": id}), None),
            )
            .await
            .unwrap();
    }
    publish(&storage, &info, &mut timeline, "base").await;
    storage
        .append_update(
            &info,
            &user_chunk(&info, "four", serde_json::json!({"messageId": "p4"}), None),
        )
        .await
        .unwrap();
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let base_bytes = index.execution_frontier().bytes;
    assert_eq!(index.timeline_newest(8).unwrap().total_entries, 3);
    let prepared = loaded
        .history
        .prepare_indexed(&info.id, &loaded.timeline)
        .unwrap();
    assert!(
        prepared
            .updates
            .iter()
            .any(|update| chunk_text(update) == Some("four"))
    );
    let updates = prepared.updates;
    drop(loaded);

    publish(&storage, &info, &mut timeline, "tail").await;
    let mut loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let published = index.execution_frontier().bytes;
    assert_ne!(published, base_bytes);
    assert_eq!(
        index.timeline_generation(base_bytes).unwrap(),
        index.timeline_generation(published).unwrap()
    );
    let frozen = index
        .timeline_at(base_bytes, history::TimelineQuery::Newest, 8)
        .unwrap();
    assert_eq!(frozen.total_entries, 3);
    assert_eq!(previews(&frozen), ["one", "two", "three"]);
    assert_eq!(
        previews(&index.timeline_newest(8).unwrap()),
        ["one", "two", "three", "four"]
    );

    // The load that captured `updates` still names the old frontier, while this
    // database already stored the tail in the same generation.
    let mut stale = loaded.history.indexed.clone().unwrap();
    let mut manifest = (*stale.manifest).clone();
    manifest.tool_frontier.bytes = base_bytes;
    stale.manifest = std::sync::Arc::new(manifest);
    loaded.history.indexed = Some(stale);
    let pages = loaded
        .history
        .indexed_pages(history::PreparedHistory {
            updates,
            mark_replay: false,
        })
        .unwrap()
        .unwrap();
    let page = pages.timeline_newest(8).unwrap();
    assert_eq!(previews(&page), ["one", "two", "three", "four"]);
    assert_eq!(page.total_entries, 4);
    assert_eq!(
        page.entries
            .iter()
            .filter(|entry| entry.message_id.as_deref() == Some("p4"))
            .count(),
        1
    );
}

fn open_pages(
    loaded: &crate::session::storage::PersistedDataLight,
    info: &Info,
) -> crate::session::history::IndexedPages {
    let prepared = loaded
        .history
        .prepare_indexed(&info.id, &loaded.timeline)
        .unwrap();
    loaded.history.indexed_pages(prepared).unwrap().unwrap()
}

fn chunk_text(update: &SessionUpdate) -> Option<&str> {
    let SessionUpdate::Acp(notification) = update else {
        return None;
    };
    let acp::SessionUpdate::UserMessageChunk(chunk) = &notification.update else {
        return None;
    };
    match &chunk.content {
        acp::ContentBlock::Text(text) => Some(text.text.as_str()),
        _ => None,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn suffix_overlay_unpublished_prompts_join_the_current_cut() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    for (text, id) in [("one", "p1"), ("two", "p2"), ("three", "p3")] {
        storage
            .append_update(
                &info,
                &user_chunk(&info, text, serde_json::json!({"messageId": id}), None),
            )
            .await
            .unwrap();
    }
    publish(&storage, &info, &mut timeline, "base").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let base_bytes = index.execution_frontier().bytes;
    let published = index.timeline_newest(8).unwrap();
    assert_eq!(previews(&published), ["one", "two", "three"]);
    let base_sources: Vec<_> = published.entries.iter().map(|entry| entry.source).collect();
    drop(loaded);

    for (text, id) in [("four", "p4"), ("five", "p5")] {
        storage
            .append_update(
                &info,
                &user_chunk(&info, text, serde_json::json!({"messageId": id}), None),
            )
            .await
            .unwrap();
    }
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    assert_eq!(index.timeline_newest(8).unwrap().total_entries, 3);
    assert_eq!(
        index
            .timeline_at(base_bytes, history::TimelineQuery::Newest, 8)
            .unwrap()
            .total_entries,
        3
    );
    let pages = open_pages(&loaded, &info);
    let page = pages.timeline_newest(8).unwrap();
    assert_eq!(previews(&page), ["one", "two", "three", "four", "five"]);
    assert_eq!(page.total_entries, 5);
    let around = pages.timeline_around_ordinal(1, 1).unwrap();
    let hit = around
        .entries
        .iter()
        .find(|entry| entry.ordinal == 1)
        .unwrap();
    assert_eq!(hit.source, base_sources[1]);
    assert_eq!(hit.preview, "two");
    assert!(pages.timeline_before(0, 4).is_err());
    assert!(
        pages
            .timeline_around_ordinal(page.total_entries, 4)
            .is_err()
    );
    drop(pages);
    drop(loaded);

    publish(&storage, &info, &mut timeline, "later").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    let published = index.timeline_newest(8).unwrap();
    let merged = open_pages(&loaded, &info).timeline_newest(8).unwrap();
    assert_eq!(
        previews(&published),
        ["one", "two", "three", "four", "five"]
    );
    assert_eq!(merged.total_entries, published.total_entries);
    assert_eq!(
        merged
            .entries
            .iter()
            .map(|entry| (entry.source, entry.preview.as_str()))
            .collect::<Vec<_>>(),
        published
            .entries
            .iter()
            .map(|entry| (entry.source, entry.preview.as_str()))
            .collect::<Vec<_>>(),
    );
}

#[tokio::test(flavor = "current_thread")]
async fn suffix_overlay_consume_inserts_inside_the_published_record_range() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    storage
        .append_update(
            &info,
            &user_chunk(
                &info,
                "first",
                serde_json::json!({"messageId": "first"}),
                None,
            ),
        )
        .await
        .unwrap();
    admit_prompt(
        &storage,
        &info,
        &mut timeline,
        "input",
        TurnId(1),
        "middle",
        0,
    )
    .await;
    append_provisional_echo(&storage, &info, "middle", "echo", 0).await;
    storage
        .append_update(
            &info,
            &user_chunk(
                &info,
                "last",
                serde_json::json!({"messageId": "last"}),
                None,
            ),
        )
        .await
        .unwrap();
    publish(&storage, &info, &mut timeline, "hidden").await;
    let hidden = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let hidden_index = hidden.history.indexed.as_ref().unwrap();
    assert_eq!(
        previews(&hidden_index.timeline_newest(8).unwrap()),
        ["first", "last"]
    );
    let hidden_cut = hidden_index.execution_frontier().bytes;
    let records = hidden_index.records();
    drop(hidden);

    consume_prompt(
        &storage,
        &info,
        &mut timeline,
        "input",
        TurnId(1),
        "middle",
        "echo",
        0,
    )
    .await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    assert_eq!(index.execution_frontier().bytes, hidden_cut);
    assert_eq!(
        previews(&index.timeline_newest(8).unwrap()),
        ["first", "last"]
    );
    let prepared = loaded
        .history
        .prepare_indexed(&info.id, &loaded.timeline)
        .unwrap();
    assert!(
        prepared
            .updates
            .iter()
            .any(|update| chunk_text(update) == Some("middle"))
    );
    let pages = loaded.history.indexed_pages(prepared).unwrap().unwrap();
    let page = pages.timeline_newest(8).unwrap();
    assert_eq!(previews(&page), ["first", "middle", "last"]);
    assert_eq!(page.total_entries, 3);
    assert!(page.entries[1].source.record < records);
    assert!(page.entries[0].source < page.entries[1].source);
    assert!(page.entries[1].source < page.entries[2].source);
    assert_eq!(page.entries[1].message_id.as_deref(), Some("echo"));
    let window = pages
        .read_before(&info.id, crate::session::history::PagePosition::newest())
        .unwrap();
    let visible = window
        .rows
        .iter()
        .find(|row| chunk_text(&row.update) == Some("middle"))
        .unwrap();
    assert_eq!(visible.source, page.entries[1].source);
    assert_eq!(
        index
            .timeline_at(hidden_cut, history::TimelineQuery::Newest, 8)
            .unwrap()
            .total_entries,
        2
    );
}

#[tokio::test(flavor = "current_thread")]
async fn suffix_overlay_keeps_message_identity_and_coalesced_segments() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    storage
        .append_update(
            &info,
            &user_chunk(&info, "same", serde_json::json!({"messageId": "a"}), None),
        )
        .await
        .unwrap();
    publish(&storage, &info, &mut timeline, "base").await;
    storage
        .append_update(
            &info,
            &user_chunk(&info, "more", serde_json::json!({"messageId": "a"}), None),
        )
        .await
        .unwrap();
    storage
        .append_update(
            &info,
            &user_chunk(&info, "same", serde_json::json!({"messageId": "b"}), None),
        )
        .await
        .unwrap();
    storage
        .append_update(
            &info,
            &user_chunk(
                &info,
                "joined",
                serde_json::json!({"messageId": "combo"}),
                Some(serde_json::json!({"combinedDisplayTexts": ["alpha", "beta"]})),
            ),
        )
        .await
        .unwrap();
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    assert_eq!(
        previews(
            &loaded
                .history
                .indexed
                .as_ref()
                .unwrap()
                .timeline_newest(8)
                .unwrap()
        ),
        ["same"]
    );
    let page = open_pages(&loaded, &info).timeline_newest(8).unwrap();
    assert_eq!(previews(&page), ["same", "same", "alpha", "beta"]);
    assert_eq!(page.total_entries, 4);
    assert_ne!(page.entries[0].source, page.entries[1].source);
    assert_eq!(page.entries[1].message_id.as_deref(), Some("b"));
    assert_eq!(page.entries[2].source, page.entries[3].source);
    assert_eq!(page.entries[2].display_segment, Some(0));
    assert_eq!(page.entries[3].display_segment, Some(1));
    assert_eq!(page.entries[2].message_id, page.entries[3].message_id);
    drop(loaded);

    publish(&storage, &info, &mut timeline, "later").await;
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let published = loaded
        .history
        .indexed
        .as_ref()
        .unwrap()
        .timeline_newest(8)
        .unwrap();
    let merged = open_pages(&loaded, &info).timeline_newest(8).unwrap();
    assert_eq!(previews(&published), ["same", "same", "alpha", "beta"]);
    assert_eq!(merged.total_entries, 4);
    assert_eq!(
        merged
            .entries
            .iter()
            .map(|entry| entry.source)
            .collect::<Vec<_>>(),
        published
            .entries
            .iter()
            .map(|entry| entry.source)
            .collect::<Vec<_>>(),
    );
}

#[tokio::test(flavor = "current_thread")]
async fn suffix_overlay_page_reads_only_the_bounded_rank_window() {
    let root = tempfile::tempdir().unwrap();
    let (storage, info) = fixture(root.path());
    storage
        .init_session(&info, default_model_id())
        .await
        .unwrap();
    let mut timeline = chat_state::Timeline::default();
    for index in 0..300 {
        storage
            .append_update(
                &info,
                &user_chunk(
                    &info,
                    "base",
                    serde_json::json!({"messageId": format!("m{index}")}),
                    None,
                ),
            )
            .await
            .unwrap();
    }
    publish(&storage, &info, &mut timeline, "base").await;
    storage
        .append_update(
            &info,
            &user_chunk(
                &info,
                "tail-a",
                serde_json::json!({"messageId": "tail-a"}),
                None,
            ),
        )
        .await
        .unwrap();
    storage
        .append_update(
            &info,
            &user_chunk(
                &info,
                "tail-b",
                serde_json::json!({"messageId": "tail-b"}),
                None,
            ),
        )
        .await
        .unwrap();
    let loaded = storage
        .load_session_for_write_with_indexed_history(&info)
        .await
        .unwrap();
    let index = loaded.history.indexed.as_ref().unwrap();
    assert_eq!(index.timeline_newest(8).unwrap().total_entries, 300);
    let pages = open_pages(&loaded, &info);
    let catalog = pages.timeline_newest(8).unwrap();
    assert_eq!(catalog.total_entries, 302);
    assert_eq!(catalog.entries.last().unwrap().preview, "tail-b");

    IndexedHistory::reset_timeline_rank_reads();
    let around = pages.timeline_around_ordinal(0, 10_000).unwrap();
    assert_eq!(around.entries.len(), 128);
    assert_eq!(around.entries[0].ordinal, 0);
    assert_eq!(IndexedHistory::timeline_rank_reads(), 128);

    IndexedHistory::reset_timeline_rank_reads();
    let newest = pages.timeline_newest(10_000).unwrap();
    assert_eq!(newest.entries.len(), 128);
    assert!(IndexedHistory::timeline_rank_reads() <= 128);
    assert!(IndexedHistory::timeline_rank_reads() < 300);

    IndexedHistory::reset_timeline_rank_reads();
    let before = pages.timeline_before(10, 4).unwrap();
    assert_eq!(
        before
            .entries
            .iter()
            .map(|entry| entry.ordinal)
            .collect::<Vec<_>>(),
        vec![6, 7, 8, 9]
    );
    assert_eq!(IndexedHistory::timeline_rank_reads(), 4);

    IndexedHistory::reset_timeline_rank_reads();
    assert!(pages.timeline_before(0, 4).is_err());
    assert!(
        pages
            .timeline_around_ordinal(catalog.total_entries, 4)
            .is_err()
    );
    assert_eq!(IndexedHistory::timeline_rank_reads(), 0);
}
