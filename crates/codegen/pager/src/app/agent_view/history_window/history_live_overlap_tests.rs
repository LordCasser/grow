//! A late history page adopting a live user echo already in the tail.
//!
//! The live tail owns the echo display until a page covering the same
//! `messageId` arrives. The page must keep one entity at the canonical source
//! position, reuse the echo's `EntryId`, and carry its selection and native
//! commit state — never a second copy and never a reprint.

use crate::acp::meta::NotificationMeta;
use crate::app::actions::{Action, TaskResult};
use crate::app::agent_view::AgentView;
use crate::app::agent_view::history_window::HistoryPageOutcome;
use crate::app::root::AppView;
use crate::app::root::tests::test_app_with_agent;
use crate::app::session::AgentId;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::entry::EntryId;
use crate::scrollback::state::ScrollbackState;
use acp_transport::protocol as acp;
use shell::session::storage::{
    HistoryEntity, HistoryPageQuery, HistoryPageResponse, HistoryRow, HistorySourceKey,
    SessionUpdate,
};

const GENERATION: u64 = 4;

fn agent(app: &mut AppView) -> &mut AgentView {
    app.agents.get_mut(&AgentId(0)).expect("test agent")
}

fn bind(view: &mut AgentView, session_id: &acp::SessionId) {
    view.session.session_id = Some(session_id.clone());
    view.history_cut = Some("cut".into());
    view.history_generation = GENERATION;
}

/// A live `UserMessageChunk` exactly as the ACP handler delivers it.
fn live_user_chunk(message_id: &str, text: &str) -> acp::SessionUpdate {
    let mut chunk_meta = acp::Meta::new();
    chunk_meta.insert("messageId".into(), serde_json::json!(message_id));
    acp::SessionUpdate::UserMessageChunk(
        acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
            text.to_string(),
        )))
        .meta(Some(chunk_meta)),
    )
}

/// A coalesced live echo: one chunk whose combined display texts become one
/// `UserPrompt` per segment, all sharing the `messageId`.
fn live_combined_chunk(message_id: &str, wire: &str, segments: &[&str]) -> acp::SessionUpdate {
    let mut chunk_meta = acp::Meta::new();
    chunk_meta.insert("messageId".into(), serde_json::json!(message_id));
    let mut content_meta = acp::Meta::new();
    content_meta.insert("combinedDisplayTexts".into(), serde_json::json!(segments));
    acp::SessionUpdate::UserMessageChunk(
        acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(wire.to_string()).meta(Some(content_meta)),
        ))
        .meta(Some(chunk_meta)),
    )
}

fn user_history_row(
    session: &acp::SessionId,
    record: u64,
    message_id: Option<&str>,
    text: &str,
    segments: Option<&[&str]>,
) -> HistoryRow {
    let mut content = acp::TextContent::new(text.to_string());
    if let Some(segments) = segments {
        let mut content_meta = acp::Meta::new();
        content_meta.insert("combinedDisplayTexts".into(), serde_json::json!(segments));
        content = content.meta(Some(content_meta));
    }
    let mut chunk = acp::ContentChunk::new(acp::ContentBlock::Text(content));
    if let Some(message_id) = message_id {
        let mut chunk_meta = acp::Meta::new();
        chunk_meta.insert("messageId".into(), serde_json::json!(message_id));
        chunk = chunk.meta(Some(chunk_meta));
    }
    HistoryRow {
        source: HistorySourceKey { record, part: 0 },
        entity: message_id.map(|message_id| HistoryEntity::UserMessage {
            message_id: message_id.to_owned(),
        }),
        fragment: None,
        update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            session.clone(),
            acp::SessionUpdate::UserMessageChunk(chunk),
        ))),
    }
}

fn reply_history_row(session: &acp::SessionId, record: u64, text: &str) -> HistoryRow {
    HistoryRow {
        source: HistorySourceKey { record, part: 0 },
        entity: Some(HistoryEntity::Message {
            event_seq: record,
            item_ordinal: 0,
        }),
        fragment: None,
        update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            session.clone(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text.to_string()),
            ))),
        ))),
    }
}

fn latest_page(
    session: &acp::SessionId,
    rows: Vec<HistoryRow>,
    older_cursor: Option<String>,
) -> HistoryPageResponse {
    HistoryPageResponse {
        session_id: session.clone(),
        cut: "cut".to_string(),
        rows,
        range: None,
        older_cursor,
        newer_cursor: None,
        target_source: None,
        descendant_ownership: None,
    }
}

fn scrollback_user_prompt_ids(scrollback: &ScrollbackState, message_id: &str) -> Vec<EntryId> {
    (0..scrollback.len())
        .filter_map(|index| scrollback.entry(index))
        .filter(|entry| {
            matches!(
                &entry.block,
                RenderBlock::UserPrompt(block)
                    if block.message_id.as_deref() == Some(message_id)
            )
        })
        .map(|entry| entry.id)
        .collect()
}

fn prompt_texts(view: &AgentView) -> Vec<String> {
    (0..view.scrollback.len())
        .filter_map(|index| view.scrollback.entry(index))
        .filter_map(|entry| match &entry.block {
            RenderBlock::UserPrompt(block) => Some(block.text.clone()),
            _ => None,
        })
        .collect()
}

fn prompt_position(view: &AgentView, text: &str) -> usize {
    (0..view.scrollback.len())
        .find(|&index| {
            view.scrollback
                .entry(index)
                .and_then(|entry| entry.block.searchable_text())
                .is_some_and(|candidate| candidate == text)
        })
        .expect("prompt row")
}

fn feed_live(view: &mut AgentView, update: acp::SessionUpdate) -> bool {
    let AgentView {
        session,
        scrollback,
        ..
    } = view;
    session.handle_update(update, &NotificationMeta::default(), scrollback)
}

/// Request and complete Latest through the production TaskComplete route.
/// The caller arms once, so failures and retries share the initial-load state.
fn complete_requested_latest(
    app: &mut AppView,
    session_id: &acp::SessionId,
    result: Result<HistoryPageResponse, String>,
) {
    let (request_id, generation, binding_epoch) = {
        let view = agent(app);
        assert!(view.request_latest_history_page(&mut Vec::new()));
        (
            view.history_page_request.expect("latest request"),
            view.history_generation,
            view.session_binding_epoch,
        )
    };
    crate::app::root::dispatch::dispatch(
        Action::TaskComplete(TaskResult::HistoryPageLoaded {
            agent_id: AgentId(0),
            session_id: session_id.clone(),
            binding_epoch,
            request_id,
            generation,
            query: HistoryPageQuery::Latest,
            result,
        }),
        app,
    );
}

#[test]
fn initial_latest_leaves_new_history_for_one_native_commit() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("initial-native-session");
    let view = agent(&mut app);
    bind(view, &session_id);
    view.arm_history_load(Some("cut".into()));
    assert!(view.history_latest_pending);

    complete_requested_latest(
        &mut app,
        &session_id,
        Ok(latest_page(
            &session_id,
            vec![
                user_history_row(&session_id, 5, Some("m1"), "restored prompt", None),
                reply_history_row(&session_id, 6, "restored reply"),
            ],
            None,
        )),
    );
    let view = agent(&mut app);
    assert!(!view.history_latest_pending);
    assert_eq!(view.history_prefix_len(), 2);
    assert_eq!(view.scrollback.commit_scan_cursor(), 0);
    let ids = (0..view.history_prefix_len())
        .map(|index| view.scrollback.entry(index).expect("history entry").id)
        .collect::<Vec<_>>();
    assert!(ids.iter().all(|id| !view.scrollback.is_committed(*id)));

    // Exercise the scrollback facts used by the normal native commit pass:
    // scan from the hint, skip committed IDs, and record successful writes.
    let mut committed = Vec::new();
    for _ in 0..2 {
        for index in view.scrollback.commit_scan_cursor()..view.scrollback.len() {
            let id = view.scrollback.entry(index).expect("entry").id;
            if !view.scrollback.is_committed(id) {
                committed.push(id);
                view.scrollback.mark_committed(index);
            }
        }
        view.scrollback
            .set_commit_scan_cursor(view.scrollback.len());
    }
    assert_eq!(committed, ids, "each initial history entity commits once");
}

#[test]
fn initial_latest_preserves_printed_echo_and_later_pages_stay_committed() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("initial-adopted-session");
    let view = agent(&mut app);
    bind(view, &session_id);
    view.arm_history_load(Some("cut".into()));
    assert!(feed_live(view, live_user_chunk("m1", "printed echo")));
    let echo_id = scrollback_user_prompt_ids(&view.scrollback, "m1")[0];
    let echo_index = view.scrollback.index_of_id(echo_id).expect("echo");
    view.scrollback.mark_committed(echo_index);
    let tail_id = view
        .scrollback
        .push_block(RenderBlock::notice("printed live tail"));
    view.scrollback.mark_committed(view.scrollback.len() - 1);
    let pending_tail = view
        .scrollback
        .push_block(RenderBlock::notice("pending live tail"));
    view.scrollback
        .set_commit_scan_cursor(view.scrollback.len() - 1);
    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "printed echo", None),
        reply_history_row(&session_id, 6, "new history reply"),
    ];
    complete_requested_latest(
        &mut app,
        &session_id,
        Ok(latest_page(&session_id, rows.clone(), Some("older".into()))),
    );
    let view = agent(&mut app);
    assert_eq!(
        scrollback_user_prompt_ids(&view.scrollback, "m1"),
        vec![echo_id]
    );
    assert!(view.scrollback.is_committed(echo_id));
    assert!(view.scrollback.is_committed(tail_id));
    assert!(!view.scrollback.is_committed(pending_tail));
    assert_eq!(view.scrollback.commit_scan_cursor(), 0);
    let reply_id = view.scrollback.entry(1).expect("history reply").id;
    assert!(!view.scrollback.is_committed(reply_id));
    // Successful native writes for the new history and live tail.
    for index in 0..view.scrollback.len() {
        view.scrollback.mark_committed(index);
    }
    view.scrollback
        .set_commit_scan_cursor(view.scrollback.len());
    let generation = view.history_generation;
    let pages = [
        (
            HistoryPageQuery::Before {
                cursor: "older".into(),
            },
            vec![user_history_row(
                &session_id,
                1,
                Some("m0"),
                "older prompt",
                None,
            )],
        ),
        (HistoryPageQuery::Latest, rows.clone()),
        (
            HistoryPageQuery::Around {
                target: "5:0".into(),
            },
            rows,
        ),
        (
            HistoryPageQuery::After {
                cursor: "newer".into(),
            },
            vec![user_history_row(
                &session_id,
                7,
                Some("m2"),
                "newer prompt",
                None,
            )],
        ),
    ];
    for (query, rows) in pages {
        assert!(matches!(
            view.apply_history_page(generation, &query, latest_page(&session_id, rows, None)),
            HistoryPageOutcome::Adopted
        ));
        assert_eq!(
            scrollback_user_prompt_ids(&view.scrollback, "m1"),
            vec![echo_id]
        );
        assert!(view.scrollback.is_committed(echo_id));
        assert!(view.scrollback.is_committed(tail_id));
        assert!(view.scrollback.is_committed(pending_tail));
        assert!((0..view.history_prefix_len()).all(|index| {
            view.scrollback
                .is_committed(view.scrollback.entry(index).expect("history").id)
        }));
        assert_eq!(
            view.scrollback.commit_scan_cursor(),
            view.history_prefix_len()
        );
    }
}

#[test]
fn pages_before_first_native_draw_preserve_the_pending_baseline() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("initial-before-draw-session");
    let view = agent(&mut app);
    bind(view, &session_id);
    view.arm_history_load(Some("cut".into()));
    assert!(feed_live(view, live_user_chunk("m1", "unprinted echo")));
    let echo_id = scrollback_user_prompt_ids(&view.scrollback, "m1")[0];
    let tail_id = view
        .scrollback
        .push_block(RenderBlock::notice("unprinted live tail"));
    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "unprinted echo", None),
        reply_history_row(&session_id, 6, "unprinted restored reply"),
    ];
    complete_requested_latest(
        &mut app,
        &session_id,
        Ok(latest_page(&session_id, rows.clone(), Some("older".into()))),
    );
    let view = agent(&mut app);
    let initial_ids = (0..view.history_prefix_len())
        .map(|index| view.scrollback.entry(index).expect("initial history").id)
        .collect::<Vec<_>>();
    assert_eq!(initial_ids.len(), 2);
    assert_eq!(initial_ids[0], echo_id);
    assert!(
        initial_ids
            .iter()
            .all(|id| !view.scrollback.is_committed(*id))
    );
    assert!(!view.scrollback.is_committed(tail_id));
    // No draw or native commit occurs before the next page completes.
    assert!(view.request_history_before(&mut Vec::new()));
    let request_id = view.history_page_request.expect("before request");
    let generation = view.history_generation;
    let binding_epoch = view.session_binding_epoch;
    crate::app::root::dispatch::dispatch(
        Action::TaskComplete(TaskResult::HistoryPageLoaded {
            agent_id: AgentId(0),
            session_id: session_id.clone(),
            binding_epoch,
            request_id,
            generation,
            query: HistoryPageQuery::Before {
                cursor: "older".into(),
            },
            result: Ok(latest_page(
                &session_id,
                vec![user_history_row(
                    &session_id,
                    1,
                    Some("m0"),
                    "older resident prompt",
                    None,
                )],
                None,
            )),
        }),
        &mut app,
    );
    let view = agent(&mut app);
    let older_id = scrollback_user_prompt_ids(&view.scrollback, "m0")[0];
    assert!(
        view.scrollback.is_committed(older_id),
        "new older rows cannot native-prepend"
    );
    assert!(
        initial_ids
            .iter()
            .all(|id| !view.scrollback.is_committed(*id))
    );
    assert!(!view.scrollback.is_committed(tail_id));
    assert_eq!(view.scrollback.commit_scan_cursor(), 1);

    complete_requested_latest(
        &mut app,
        &session_id,
        Ok(latest_page(&session_id, rows, None)),
    );
    let view = agent(&mut app);
    assert!(view.scrollback.is_committed(older_id));
    assert_eq!(
        scrollback_user_prompt_ids(&view.scrollback, "m1"),
        vec![echo_id]
    );
    assert!(
        initial_ids
            .iter()
            .all(|id| !view.scrollback.is_committed(*id))
    );
    assert!(!view.scrollback.is_committed(tail_id));
    assert_eq!(view.scrollback.commit_scan_cursor(), 1);
    let mut committed = Vec::new();
    for _ in 0..2 {
        for index in view.scrollback.commit_scan_cursor()..view.scrollback.len() {
            let id = view.scrollback.entry(index).expect("native candidate").id;
            if !view.scrollback.is_committed(id) {
                committed.push(id);
                view.scrollback.mark_committed(index);
            }
        }
        view.scrollback
            .set_commit_scan_cursor(view.scrollback.len());
    }
    let mut expected = initial_ids;
    expected.push(tail_id);
    assert_eq!(
        committed, expected,
        "pending baseline and live tail commit exactly once"
    );
}

#[test]
fn failed_initial_latest_keeps_native_baseline_pending_until_success() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("initial-retry-session");
    let view = agent(&mut app);
    bind(view, &session_id);
    view.arm_history_load(Some("cut".into()));
    complete_requested_latest(&mut app, &session_id, Err("read failed".into()));
    assert!(agent(&mut app).history_latest_pending);
    assert!(agent(&mut app).history_page_request.is_none());
    assert!(agent(&mut app).scrollback.is_empty());

    // Rejected page content must also leave the first successful Latest armed.
    complete_requested_latest(
        &mut app,
        &session_id,
        Ok(latest_page(
            &session_id,
            vec![
                reply_history_row(&session_id, 6, "first value"),
                reply_history_row(&session_id, 6, "conflicting value"),
            ],
            None,
        )),
    );
    assert!(agent(&mut app).history_latest_pending);
    assert!(agent(&mut app).history_page_request.is_none());
    assert!(agent(&mut app).scrollback.is_empty());

    complete_requested_latest(
        &mut app,
        &session_id,
        Ok(latest_page(
            &session_id,
            vec![reply_history_row(&session_id, 6, "successful retry")],
            None,
        )),
    );
    let view = agent(&mut app);
    assert!(!view.history_latest_pending);
    assert_eq!(view.history_prefix_len(), 1);
    assert_eq!(view.scrollback.commit_scan_cursor(), 0);
    let id = view.scrollback.entry(0).expect("retried history").id;
    assert!(!view.scrollback.is_committed(id));
}

/// Live echo first, history page later: one entity, canonical position, the
/// echo's id and native state kept, and later reads never rebuild it.
#[test]
fn late_history_page_adopts_the_live_echo_once() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("overlap-session");
    bind(agent(&mut app), &session_id);

    // Real live path: ACP handler -> session tracker -> scrollback.
    assert!(feed_live(
        agent(&mut app),
        live_user_chunk("m1", "echoed prompt"),
    ));
    let echo_id = scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m1")[0];
    let bookmark = {
        let view = agent(&mut app);
        view.scrollback
            .push_block(RenderBlock::notice("live tail marker"));
        view.scrollback.prepare_layout(40, 3);
        let echo_index = view.scrollback.index_of_id(echo_id).expect("echo index");
        view.scrollback.mark_committed(echo_index);
        assert!(view.scrollback.is_committed(echo_id));
        // Pin the viewport (bookmark anchor) and the selection onto the echo.
        view.scrollback.scroll_to_entry_top(echo_index);
        view.scrollback.set_selected(Some(echo_index));
        let bookmark = view.scrollback.capture_scroll_bookmark();
        assert!(bookmark.is_some(), "bookmark pinned on the echoed prompt");
        bookmark
    };

    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "echoed prompt", None),
        reply_history_row(&session_id, 6, "the persisted reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows, Some("older".into())),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));

    {
        let view = agent(&mut app);
        assert_eq!(
            scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
            &[echo_id],
            "one entity keeping the echo id"
        );
        let texts = prompt_texts(view);
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "echoed prompt")
                .count(),
            1
        );
        let echo_index = view.scrollback.index_of_id(echo_id).expect("adopted row");
        let reply_index = (0..view.scrollback.len())
            .find(|&index| {
                view.scrollback.entry(index).is_some_and(|entry| {
                    entry.block.searchable_text().as_deref() == Some("the persisted reply")
                })
            })
            .expect("reply row");
        assert!(echo_index < reply_index, "prompt precedes its reply");
        assert!(
            reply_index < view.scrollback.len() - 1,
            "reply precedes the live tail"
        );
        assert!(echo_index < view.history_prefix_len());
        // Selection and native commit survive the adoption.
        let selected = view.scrollback.selected().expect("selection kept");
        assert_eq!(
            view.scrollback.entry(selected).expect("selected").id,
            echo_id
        );
        assert!(view.scrollback.is_committed(echo_id));
        assert_eq!(
            view.scrollback.capture_scroll_bookmark(),
            bookmark,
            "the pinned viewport row keeps its content anchor"
        );
        assert_eq!(
            view.scrollback.commit_scan_cursor(),
            view.history_prefix_len(),
            "cursor stops at the true live-tail boundary"
        );
        // The timeline source -> entry getter resolves the original id.
        assert_eq!(
            view.history_entry_id_for_timeline(
                HistorySourceKey { record: 5, part: 0 },
                Some("m1"),
                None
            ),
            Some(echo_id)
        );
    }

    // A Before page extends the window: the adopted entity stays exactly once
    // with the same id, and older rows land before it.
    let before_rows = vec![
        user_history_row(&session_id, 1, Some("m0"), "older prompt", None),
        reply_history_row(&session_id, 2, "older reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Before {
            cursor: "older".into(),
        },
        latest_page(&session_id, before_rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));
    {
        let view = agent(&mut app);
        assert_eq!(
            scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
            &[echo_id]
        );
        let texts = prompt_texts(view);
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "echoed prompt")
                .count(),
            1
        );
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "older prompt")
                .count(),
            1
        );
        assert!(prompt_position(view, "older prompt") < prompt_position(view, "echoed prompt"));
        assert!(prompt_position(view, "echoed prompt") < view.history_prefix_len());
        assert_eq!(
            view.scrollback.commit_scan_cursor(),
            view.history_prefix_len()
        );
    }

    // A duplicate page re-reads the same rows without duplicating anything.
    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "echoed prompt", None),
        reply_history_row(&session_id, 6, "the persisted reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows, Some("older".into())),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));
    {
        let view = agent(&mut app);
        assert_eq!(
            scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
            &[echo_id]
        );
        assert!(view.scrollback.is_committed(echo_id));
    }

    // A live re-echo of the same message id must not rebuild a second row.
    assert!(!feed_live(
        agent(&mut app),
        live_user_chunk("m1", "echoed prompt"),
    ));
    assert_eq!(
        scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m1").as_slice(),
        &[echo_id]
    );
}

/// Page first, live echo afterwards: the tracker reconciles by `messageId`
/// across the whole scrollback, so no second row appears in either order.
#[test]
fn page_before_live_echo_reconciles_by_message_id() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("page-first-session");
    bind(agent(&mut app), &session_id);

    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "typed prompt", None),
        reply_history_row(&session_id, 6, "the reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));
    let installed_id = scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m1")[0];

    assert!(!feed_live(
        agent(&mut app),
        live_user_chunk("m1", "typed prompt"),
    ));
    let view = agent(&mut app);
    assert_eq!(
        scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
        &[installed_id],
        "the live echo joins the installed row instead of adding one"
    );
    let texts = prompt_texts(view);
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == "typed prompt")
            .count(),
        1
    );
    assert_eq!(
        view.history_entry_id_for_timeline(
            HistorySourceKey { record: 5, part: 0 },
            Some("m1"),
            None
        ),
        Some(installed_id)
    );
}

/// Adoption matches exact `messageId` identity only: a different id with the
/// same text, and rows without any id, stay independent entities.
#[test]
fn adoption_matches_message_id_not_text() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("identity-session");
    bind(agent(&mut app), &session_id);

    assert!(feed_live(
        agent(&mut app),
        live_user_chunk("m-a", "same words"),
    ));
    let echo_id = scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m-a")[0];

    let rows = vec![
        user_history_row(&session_id, 5, Some("m-a"), "same words", None),
        user_history_row(&session_id, 6, Some("m-b"), "same words", None),
        user_history_row(&session_id, 7, None, "same words", None),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));

    let view = agent(&mut app);
    assert_eq!(
        scrollback_user_prompt_ids(&view.scrollback, "m-a").as_slice(),
        &[echo_id]
    );
    let m_b = scrollback_user_prompt_ids(&view.scrollback, "m-b");
    assert_eq!(m_b.len(), 1, "a different id stays its own row");
    assert_ne!(m_b[0], echo_id);
    assert_eq!(
        view.history_entry_id_for_timeline(
            HistorySourceKey { record: 6, part: 0 },
            Some("m-b"),
            None
        ),
        Some(m_b[0])
    );
    // Rows without a message id are located by source, never merged.
    let no_id =
        view.history_entry_id_for_timeline(HistorySourceKey { record: 7, part: 0 }, None, None);
    assert!(no_id.is_some_and(|id| id != echo_id && id != m_b[0]));
    let texts = prompt_texts(view);
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == "same words")
            .count(),
        3,
        "three identities, one row each"
    );
}

/// Coalesced display segments adopt one-for-one in their display order.
#[test]
fn coalesced_segments_each_adopt_once_in_display_order() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("coalesced-session");
    bind(agent(&mut app), &session_id);

    assert!(feed_live(
        agent(&mut app),
        live_combined_chunk("m-c", "wire body", &["first half", "second half"]),
    ));
    let echoed = scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m-c");
    assert_eq!(echoed.len(), 2, "the live echo pushed both segments");
    let (first_id, second_id) = (echoed[0], echoed[1]);

    let rows = vec![user_history_row(
        &session_id,
        5,
        Some("m-c"),
        "wire body",
        Some(&["first half", "second half"]),
    )];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));

    let view = agent(&mut app);
    assert_eq!(
        scrollback_user_prompt_ids(&view.scrollback, "m-c").as_slice(),
        &[first_id, second_id],
        "both segments adopt their echo ids in display order"
    );
    let texts = prompt_texts(view);
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == "first half")
            .count(),
        1
    );
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == "second half")
            .count(),
        1
    );
    assert_eq!(
        view.history_entry_id_for_timeline(
            HistorySourceKey { record: 5, part: 0 },
            Some("m-c"),
            Some(0)
        ),
        Some(first_id)
    );
    assert_eq!(
        view.history_entry_id_for_timeline(
            HistorySourceKey { record: 5, part: 0 },
            Some("m-c"),
            Some(1)
        ),
        Some(second_id)
    );
}

/// Replacing the interval (Around) reprojects from scratch; an already
/// adopted entity keeps its id and does not rebuild a second copy.
#[test]
fn around_replacement_keeps_the_adopted_entity() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("around-session");
    bind(agent(&mut app), &session_id);

    assert!(feed_live(
        agent(&mut app),
        live_user_chunk("m1", "echoed prompt"),
    ));
    let echo_id = scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m1")[0];
    let view = agent(&mut app);
    let echo_index = view.scrollback.index_of_id(echo_id).expect("echo");
    view.scrollback.mark_committed(echo_index);

    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "echoed prompt", None),
        reply_history_row(&session_id, 6, "the persisted reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows.clone(), Some("older".into())),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));
    assert_eq!(
        scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m1").as_slice(),
        &[echo_id]
    );

    let around = HistoryPageQuery::Around {
        target: "5:0".into(),
    };
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &around,
        latest_page(&session_id, rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));
    let view = agent(&mut app);
    assert_eq!(
        scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
        &[echo_id],
        "interval replacement keeps one entity with the adopted id"
    );
    assert!(view.scrollback.is_committed(echo_id));
}

/// One message may be journalled as several `UserMessageChunk` rows sharing a
/// `messageId` (one per content block). They are one display entity: the
/// first visible chunk owns the bubbles, later chunks add none, and the
/// timeline catalog's first-source lookup stays in sync — matching the live
/// tracker and an eager full projection.
#[test]
fn same_id_chunks_collapse_into_one_entity() {
    let mut app = test_app_with_agent();
    let session_id = acp::SessionId::new("same-id-session");
    bind(agent(&mut app), &session_id);

    // The live echo renders the message's first chunk; the tracker collapses
    // the message's later chunks by id.
    assert!(feed_live(
        agent(&mut app),
        live_user_chunk("m1", "first chunk text"),
    ));
    let echo_id = scrollback_user_prompt_ids(&agent(&mut app).scrollback, "m1")[0];
    let view = agent(&mut app);
    let echo_index = view.scrollback.index_of_id(echo_id).expect("echo");
    view.scrollback.mark_committed(echo_index);

    let rows = vec![
        user_history_row(&session_id, 5, Some("m1"), "first chunk text", None),
        user_history_row(&session_id, 6, Some("m1"), "second chunk text", None),
        reply_history_row(&session_id, 7, "the persisted reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Latest,
        latest_page(&session_id, rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));

    {
        let view = agent(&mut app);
        assert_eq!(
            scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
            &[echo_id],
            "one entity for the whole message, keeping the echo id"
        );
        let texts = prompt_texts(view);
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "first chunk text")
                .count(),
            1
        );
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "second chunk text")
                .count(),
            0,
            "a later chunk of the same message adds no bubble"
        );
        // The catalog references the first source of the message only.
        assert_eq!(
            view.history_entry_id_for_timeline(
                HistorySourceKey { record: 5, part: 0 },
                Some("m1"),
                None
            ),
            Some(echo_id)
        );
        assert_eq!(
            view.history_entry_id_for_timeline(
                HistorySourceKey { record: 6, part: 0 },
                Some("m1"),
                None
            ),
            None,
            "a later chunk's source owns no separate entity"
        );
    }

    // A later page revealing an earlier source of the same message keeps the
    // entity and its EntryId and moves it to the first-source position.
    let before_rows = vec![
        user_history_row(&session_id, 1, Some("m1"), "first chunk text", None),
        reply_history_row(&session_id, 2, "older reply"),
    ];
    let outcome = agent(&mut app).apply_history_page(
        GENERATION,
        &HistoryPageQuery::Before {
            cursor: "older".into(),
        },
        latest_page(&session_id, before_rows, None),
    );
    assert!(matches!(outcome, HistoryPageOutcome::Adopted));
    {
        let view = agent(&mut app);
        assert_eq!(
            scrollback_user_prompt_ids(&view.scrollback, "m1").as_slice(),
            &[echo_id],
            "the entity keeps its installed EntryId across the source move"
        );
        let texts = prompt_texts(view);
        assert_eq!(
            texts
                .iter()
                .filter(|text| text.as_str() == "first chunk text")
                .count(),
            1,
            "records 1 and 5 are the same message, not two occurrences"
        );
        assert_eq!(
            view.history_entry_id_for_timeline(
                HistorySourceKey { record: 1, part: 0 },
                Some("m1"),
                None
            ),
            Some(echo_id),
            "the first source now resolves the entity"
        );
        assert_eq!(
            view.history_entry_id_for_timeline(
                HistorySourceKey { record: 5, part: 0 },
                Some("m1"),
                None
            ),
            None
        );
        // Consecutive assistant chunks in this fixture share one rendered
        // block; locate the persisted reply within it rather than requiring
        // an artificial chunk boundary in the display.
        let reply_position = (0..view.scrollback.len())
            .find(|&index| {
                view.scrollback
                    .entry(index)
                    .and_then(|entry| entry.block.searchable_text())
                    .is_some_and(|text| text.contains("the persisted reply"))
            })
            .expect("persisted reply remains visible");
        assert!(prompt_position(view, "first chunk text") < reply_position);
        assert!(view.scrollback.is_committed(echo_id));
    }
}
