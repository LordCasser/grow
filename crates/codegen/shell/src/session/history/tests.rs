use super::{
    HistoryContent, HistoryFact, HistorySnapshot, HistorySource, JournalFact, decode_record,
    encode_presentation,
};
use crate::session::storage::SessionUpdate;
use acp_transport::protocol as acp;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct GoldenCase {
    family: String,
    channel: String,
    notification: Value,
}

fn golden_cases() -> Vec<GoldenCase> {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/history-wire-golden.json"
    ))
    .expect("history wire golden fixture parses")
}

fn update_from_case(case: &GoldenCase) -> SessionUpdate {
    match case.channel.as_str() {
        "grow" => SessionUpdate::Grow(Box::new(
            serde_json::from_value(case.notification.clone()).expect("Grow DTO parses"),
        )),
        "acp" => SessionUpdate::Acp(Box::new(
            serde_json::from_value(case.notification.clone()).expect("ACP DTO parses"),
        )),
        channel => panic!("unknown golden channel {channel}"),
    }
}

fn notification_value(update: SessionUpdate) -> Value {
    match update {
        SessionUpdate::Acp(notification) => {
            serde_json::to_value(notification.as_ref()).expect("serialize ACP notification")
        }
        SessionUpdate::Grow(notification) => {
            serde_json::to_value(notification.as_ref()).expect("serialize Grow notification")
        }
        SessionUpdate::ResponseReplayProjection(_) => {
            panic!("response projection is not a presentation update")
        }
    }
}

#[test]
fn durable_history_wire_facts_round_trip_all_golden_fields() {
    let session_id = acp::SessionId::new("golden");
    let mut grow_count = 0;
    let mut acp_count = 0;
    let mut transient_count = 0;
    let mut grow_families = std::collections::BTreeSet::new();
    let mut acp_families = std::collections::BTreeSet::new();

    for (index, case) in golden_cases().into_iter().enumerate() {
        let update = update_from_case(&case);
        if case.family == "sampling_attempt" {
            transient_count += 1;
            assert!(
                HistoryFact::from_update(&update).is_err(),
                "SamplingAttempt must stay transient"
            );
            continue;
        }
        match case.channel.as_str() {
            "grow" => {
                grow_count += 1;
                let family = if case.family == "turn_completed_incomplete" {
                    "turn_completed"
                } else {
                    case.family.as_str()
                };
                grow_families.insert(family.to_owned());
            }
            "acp" => {
                acp_count += 1;
                acp_families.insert(case.family.clone());
            }
            channel => panic!("unknown golden channel {channel}"),
        }
        let fact = HistoryFact::from_update(&update)
            .expect("golden update maps to history fact")
            .expect("golden update is durable history");
        let raw = serde_json::value::to_raw_value(&fact).expect("serialize history fact");
        let encoded = encode_presentation(index as u64, 0, &raw).expect("encode history fact");
        let record = decode_record(
            encoded
                .strip_suffix(b"\n")
                .expect("encoded record is newline terminated"),
        )
        .expect("decode history record");
        let JournalFact::Presentation { at_ms, event } = record.fact else {
            panic!("expected presentation record")
        };
        assert_eq!(at_ms, 0);
        let actual = event
            .into_update(&session_id)
            .expect("history fact converts to a typed update")
            .expect("golden fact has a presentation update");
        assert_eq!(
            notification_value(actual),
            case.notification,
            "{}",
            case.family
        );
    }

    assert_eq!(grow_count, 38, "all durable Grow fixture rows are covered");
    assert_eq!(acp_count, 7, "all ACP fixture rows are covered");
    assert_eq!(transient_count, 1, "only SamplingAttempt is transient here");
    assert_eq!(
        grow_families.len(),
        31,
        "all durable Grow DTO families are covered"
    );
    assert_eq!(acp_families.len(), 7, "all ACP DTO families are covered");
}

fn strip_history_projection_meta(value: &mut Value) {
    let Some(meta) = value.get_mut("_meta").and_then(Value::as_object_mut) else {
        return;
    };
    meta.remove("grow/historyCursor");
    meta.remove("grow/historyTimestampMs");
    if meta.is_empty() {
        value.as_object_mut().unwrap().remove("_meta");
    }
}

#[test]
fn heterogeneous_history_preserves_order_fields_and_ownership() {
    let session_id = acp::SessionId::new("golden");
    let mut grow = Vec::new();
    let mut acp = Vec::new();
    for case in golden_cases() {
        if matches!(case.family.as_str(), "sampling_attempt" | "rewind_marker") {
            continue;
        }
        match case.channel.as_str() {
            "grow" => grow.push(case),
            "acp" => acp.push(case),
            channel => panic!("unknown golden channel {channel}"),
        }
    }

    let mut mixed = Vec::with_capacity(grow.len() + acp.len());
    let (mut grow, mut acp) = (grow.into_iter(), acp.into_iter());
    loop {
        let mut added = false;
        if let Some(case) = grow.next() {
            mixed.push(case);
            added = true;
        }
        if let Some(case) = acp.next() {
            mixed.push(case);
            added = true;
        }
        if !added {
            break;
        }
    }

    let mut state = super::AppendState::default();
    let mut history = super::HistorySnapshot::default();
    let mut journal_hash = blake3::Hasher::new();
    let mut expected = Vec::with_capacity(mixed.len());
    let mut expected_timestamps = Vec::with_capacity(mixed.len());
    for (index, case) in mixed.into_iter().enumerate() {
        let mut notification = case.notification;
        let event_id = format!("mixed-{index}");
        notification["_meta"] = serde_json::json!({"eventId": event_id});
        let update = match case.channel.as_str() {
            "grow" => SessionUpdate::Grow(Box::new(
                serde_json::from_value(notification.clone()).expect("Grow DTO parses"),
            )),
            "acp" => SessionUpdate::Acp(Box::new(
                serde_json::from_value(notification.clone()).expect("ACP DTO parses"),
            )),
            channel => panic!("unknown golden channel {channel}"),
        };
        let fact = HistoryFact::from_update(&update)
            .expect("durable fixture becomes a history fact")
            .expect("fixture is durable");
        let raw = serde_json::value::to_raw_value(&fact).expect("serialize history fact");
        let encoded = encode_presentation(index as u64, index as i64 * 17, &raw)
            .expect("encode physical presentation");
        journal_hash.update(&encoded);
        expected_timestamps.push(index as i64 * 17);
        history.bytes += encoded.len() as u64;
        let record = decode_record(
            encoded
                .strip_suffix(b"\n")
                .expect("encoded record is newline terminated"),
        )
        .expect("decode physical presentation");
        state.accept(&record).expect("mixed record is admitted");
        history.push(&record);
        expected.push(notification_value(update));
    }

    assert_eq!(history.sources.len(), 44);
    assert_eq!(state.records, 44);
    history.prefix_hash = journal_hash.finalize().to_hex().to_string();
    let timeline = chat_state::Timeline::default();
    let prepared = history
        .prepare(&session_id, &timeline, None)
        .expect("mixed history projects");
    let consumed = history
        .clone()
        .into_prepare(&session_id, &timeline, None)
        .expect("owned history projects without retaining duplicate bodies");
    assert_eq!(consumed.mark_replay, prepared.mark_replay);
    assert_eq!(
        consumed
            .updates
            .into_iter()
            .map(notification_value)
            .collect::<Vec<_>>(),
        prepared
            .updates
            .iter()
            .cloned()
            .map(notification_value)
            .collect::<Vec<_>>(),
        "moving payloads must preserve every field and physical position"
    );
    assert_eq!(prepared.updates.len(), expected.len());
    let mut last_cursor = None;
    for (index, (actual, expected)) in prepared.updates.into_iter().zip(expected).enumerate() {
        let mut actual = notification_value(actual);
        let meta = actual["_meta"].as_object().expect("projected outer _meta");
        let cursor_raw = meta["grow/historyCursor"]
            .as_str()
            .expect("projected history cursor");
        let cursor: Value = serde_json::from_str(cursor_raw).expect("cursor JSON");
        assert_eq!(cursor["version"], super::PROJECTOR_VERSION);
        assert_eq!(cursor["session"], "golden");
        assert_eq!(cursor["cut"], 44);
        assert_eq!(cursor["hash"], history.prefix_hash);
        assert_eq!(cursor["record"], index);
        assert_eq!(cursor["part"], 0);
        assert_eq!(cursor["complete"], true);
        assert_eq!(meta["grow/historyTimestampMs"], expected_timestamps[index]);
        last_cursor = Some(cursor_raw.to_owned());
        strip_history_projection_meta(&mut actual);
        assert_eq!(
            actual, expected,
            "full notification at mixed physical position {index}"
        );
    }

    let last_cursor = last_cursor.expect("mixed history produced a final cursor");
    let resumed = history
        .prepare(&session_id, &timeline, Some(&last_cursor))
        .expect("complete cursor can be resumed");
    assert!(!resumed.mark_replay);
    assert!(
        resumed.updates.is_empty(),
        "final complete cursor resumes at EOF"
    );
}

#[test]
fn canonical_multimessage_response_cursor_requires_complete_matching_proof() {
    let session_id = acp::SessionId::new("canonical-response");
    let items = vec![
        sampling_types::ConversationItem::Reasoning(sampling_types::synthesized_reasoning_item(
            "reasoning part",
        )),
        sampling_types::ConversationItem::assistant("assistant part one"),
        sampling_types::ConversationItem::assistant("assistant part two"),
    ];
    let timeline =
        crate::session::storage::jsonl::tests::response_projection_production_timeline_with_items(
            "cursor-request",
            3,
            items,
            0,
        );
    let event = timeline.events().len() - 1;
    let mut bytes = crate::session::storage::JsonlStorageAdapter::encode_journal(
        &timeline.events()[..event],
        &[],
    )
    .unwrap();
    let before = HistoryFact {
        meta: Some(
            serde_json::json!({"eventId":"canonical-response-99"})
                .as_object()
                .unwrap()
                .clone(),
        ),
        content: HistoryContent::Assistant(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new("independent before candidate"),
        ))),
    };
    let raw = serde_json::value::to_raw_value(&before).unwrap();
    bytes.extend(encode_presentation(event as u64, timeline.events()[event].at_ms, &raw).unwrap());
    let record = event + 1;
    let anchor = HistoryFact {
        meta: Some(
            serde_json::json!({"eventId":"canonical-response-100"})
                .as_object()
                .unwrap()
                .clone(),
        ),
        content: HistoryContent::Candidate(super::ResponseKey {
            request_id: "cursor-request".into(),
            attempt: 3,
        }),
    };
    let raw = serde_json::value::to_raw_value(&anchor).unwrap();
    bytes.extend(encode_presentation(record as u64, timeline.events()[event].at_ms, &raw).unwrap());
    let raw = serde_json::value::to_raw_value(&timeline.events()[event]).unwrap();
    bytes.extend(
        super::encode_domain(
            record as u64 + 1,
            &raw,
            Some(super::ResponseAnchor::Candidate {
                record: record as u64,
            }),
        )
        .unwrap(),
    );
    let lines = std::str::from_utf8(&bytes)
        .expect("journal is UTF-8")
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let journal = crate::session::storage::JsonlStorageAdapter::decode_journal_lines(&lines)
        .expect("decode canonical response Timeline");
    let prepared = journal
        .history
        .prepare(&session_id, &timeline, None)
        .expect("canonical response projects");
    assert_eq!(prepared.updates.len(), 4);
    let projected_messages = prepared
        .updates
        .iter()
        .skip(1)
        .cloned()
        .map(notification_value)
        .map(|notification| {
            (
                notification["update"]["sessionUpdate"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                notification["update"]["content"]["text"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        projected_messages,
        [
            (
                "agent_thought_chunk".to_owned(),
                "reasoning part".to_owned()
            ),
            (
                "agent_message_chunk".to_owned(),
                "assistant part one".to_owned()
            ),
            (
                "agent_message_chunk".to_owned(),
                "assistant part two".to_owned()
            ),
        ],
        "canonical response parts retain their role and order"
    );
    let mut cursors = Vec::new();
    for (part, update) in prepared.updates.iter().skip(1).enumerate() {
        let value = notification_value(update.clone());
        let meta = value["_meta"].as_object().expect("projected outer _meta");
        assert_eq!(meta["grow/historyMessageEvent"], event);
        assert!(
            !meta.contains_key("eventId"),
            "a preview highwater must not be reused by multiple canonical rows"
        );
        assert_eq!(meta["grow/historyMessageItem"], part);
        assert_eq!(meta["samplingRequestId"], "cursor-request");
        assert_eq!(meta["samplingAttempt"], 3);
        assert_eq!(
            meta["grow/historyTimestampMs"],
            timeline.events()[event].at_ms
        );
        let raw = meta["grow/historyCursor"].as_str().unwrap();
        let cursor: Value = serde_json::from_str(raw).expect("cursor JSON");
        assert_eq!(cursor["session"], "canonical-response");
        assert_eq!(cursor["cut"], timeline.events().len() + 2);
        assert_eq!(cursor["hash"], journal.history.prefix_hash);
        assert_eq!(cursor["record"], record);
        assert_eq!(cursor["part"], part);
        assert_eq!(cursor["complete"], part == 2);
        cursors.push(raw.to_owned());
    }

    let before_cursor =
        notification_value(prepared.updates[0].clone())["_meta"]["grow/historyCursor"]
            .as_str()
            .unwrap()
            .to_owned();
    let delta = journal
        .history
        .prepare(&session_id, &timeline, Some(&before_cursor))
        .unwrap();
    assert!(
        !delta.mark_replay,
        "a complete prior row keeps the existing transcript"
    );
    assert_eq!(
        delta.updates.len(),
        3,
        "all parts of the following response are delivered"
    );
    for update in delta.updates {
        let notification = notification_value(update);
        assert!(
            notification["_meta"].get("eventId").is_none(),
            "cursor-tail parts cannot share a numeric preview event ID"
        );
        assert!(notification["_meta"]["grow/historyCursor"].is_string());
    }

    let half_response = journal
        .history
        .prepare(&session_id, &timeline, Some(&cursors[1]))
        .expect("incomplete response cursor is rejected for skipping");
    assert!(half_response.mark_replay);
    assert_eq!(half_response.updates.len(), 4);

    let mut forged_complete: Value = serde_json::from_str(&cursors[1]).unwrap();
    forged_complete["complete"] = true.into();
    let forged_complete = journal
        .history
        .prepare(&session_id, &timeline, Some(&forged_complete.to_string()))
        .expect("client completion flag cannot make a partial source complete");
    assert!(forged_complete.mark_replay);
    assert_eq!(forged_complete.updates.len(), 4);

    let mut foreign: Value = serde_json::from_str(&cursors[2]).unwrap();
    foreign["session"] = "another-session".into();
    let foreign = serde_json::to_string(&foreign).unwrap();
    let foreign_session = journal
        .history
        .prepare(&session_id, &timeline, Some(&foreign))
        .expect("foreign session cursor triggers replacement");
    assert!(foreign_session.mark_replay);
    assert_eq!(foreign_session.updates.len(), 4);

    let mut newer_cut: Value = serde_json::from_str(&cursors[2]).unwrap();
    newer_cut["cut"] = (timeline.events().len() as u64 + 3).into();
    let newer_cut = serde_json::to_string(&newer_cut).unwrap();
    let changed_authority = journal
        .history
        .prepare(&session_id, &timeline, Some(&newer_cut))
        .expect("newer cut cursor triggers replacement");
    assert!(changed_authority.mark_replay);
    assert_eq!(changed_authority.updates.len(), 4);

    let complete = journal
        .history
        .prepare(&session_id, &timeline, Some(&cursors[2]))
        .expect("matching complete response cursor resumes");
    assert!(!complete.mark_replay);
    assert!(complete.updates.is_empty());
}

#[test]
fn acp_nested_content_and_tool_references_round_trip_through_history() {
    let session_id = acp::SessionId::new("nested-acp");
    let wire_notifications = vec![
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": {
                    "type": "image",
                    "data": "aW1hZ2UtYnl0ZXM=",
                    "mimeType": "image/png",
                    "uri": "file:///tmp/input.png",
                    "annotations": {"audience": ["user"], "priority": 2},
                    "_meta": {"imageKey": "input-image"}
                },
                "messageId": "user-msg-7",
                "_meta": {"chunkKey": "user-image"}
            },
            "_meta": {"eventId": "nested-0", "origin": "user-image"}
        }),
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": {
                    "type": "audio",
                    "data": "YXVkaW8=",
                    "mimeType": "audio/wav",
                    "annotations": {"audience": ["user"]},
                    "_meta": {"audioKey": "input-audio"}
                },
                "messageId": "user-msg-7",
                "_meta": {"chunkKey": "user-audio"}
            },
            "_meta": {"eventId": "nested-1", "origin": "user-audio"}
        }),
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": {
                    "type": "resource_link",
                    "name": "source.rs",
                    "uri": "file:///workspace/source.rs",
                    "title": "Source file",
                    "description": "Selected source",
                    "mimeType": "text/x-rust",
                    "size": 123,
                    "_meta": {"resourceKey": "linked-source"}
                },
                "messageId": "user-msg-7",
                "_meta": {"chunkKey": "user-resource-link"}
            },
            "_meta": {"eventId": "nested-2", "origin": "user-resource-link"}
        }),
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "resource",
                    "resource": {
                        "uri": "file:///workspace/report.txt",
                        "mimeType": "text/plain",
                        "text": "embedded text resource",
                        "_meta": {"resourceVersion": 4}
                    },
                    "_meta": {"embedding": "text"}
                },
                "messageId": "assistant-msg-8",
                "_meta": {"chunkKey": "assistant-resource-text"}
            },
            "_meta": {"eventId": "nested-3", "origin": "assistant-resource-text"}
        }),
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "resource",
                    "resource": {
                        "uri": "file:///workspace/archive.bin",
                        "mimeType": "application/octet-stream",
                        "blob": "AAECAw==",
                        "_meta": {"resourceVersion": 5}
                    },
                    "_meta": {"embedding": "blob"}
                },
                "messageId": "assistant-msg-8",
                "_meta": {"chunkKey": "assistant-resource-blob"}
            },
            "_meta": {"eventId": "nested-4", "origin": "assistant-resource-blob"}
        }),
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "agent_thought_chunk",
                "content": {
                    "type": "text",
                    "text": "reasoning with preserved metadata",
                    "annotations": {"audience": ["assistant"]},
                    "_meta": {"thoughtKey": "private-thought"}
                },
                "messageId": "thought-msg-9",
                "_meta": {"chunkKey": "thought-text"}
            },
            "_meta": {"eventId": "nested-5", "origin": "thought-text"}
        }),
        serde_json::json!({
            "sessionId": "nested-acp",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "tool-42",
                "title": "Inspect source",
                "content": [
                    {
                        "type": "content",
                        "content": {
                            "type": "resource_link",
                            "name": "source.rs",
                            "uri": "file:///workspace/source.rs",
                            "_meta": {"resourceKey": "tool-link"}
                        },
                        "_meta": {"itemKey": "tool-content"}
                    },
                    {
                        "type": "diff",
                        "path": "src/lib.rs",
                        "oldText": "fn old() {}",
                        "newText": "fn new() {}",
                        "_meta": {"diffKey": "diff-42"}
                    },
                    {
                        "type": "terminal",
                        "terminalId": "terminal-13",
                        "_meta": {"terminalKey": "terminal-reference"}
                    }
                ],
                "rawInput": {"path": "src/lib.rs"},
                "_meta": {"callKey": "tool-call"}
            },
            "_meta": {"eventId": "nested-6", "origin": "tool-call"}
        }),
    ];

    let mut state = super::AppendState::default();
    let mut history = super::HistorySnapshot::default();
    let mut hasher = blake3::Hasher::new();
    let mut expected = Vec::new();
    for (seq, wire) in wire_notifications.into_iter().enumerate() {
        let notification: acp::SessionNotification =
            serde_json::from_value(wire).expect("wire notification parses as ACP DTO");
        let update = SessionUpdate::Acp(Box::new(notification));
        let fact = HistoryFact::from_update(&update)
            .expect("supported ACP update maps to a history fact")
            .expect("supported ACP update is durable");
        let raw = serde_json::value::to_raw_value(&fact).expect("serialize typed history fact");
        let at_ms = seq as i64 * 23;
        let encoded = encode_presentation(seq as u64, at_ms, &raw)
            .expect("encode typed physical history fact");
        hasher.update(&encoded);
        history.bytes += encoded.len() as u64;
        let record = decode_record(
            encoded
                .strip_suffix(b"\n")
                .expect("encoded record ends with newline"),
        )
        .expect("decode typed physical history fact");
        state.accept(&record).expect("physical position is valid");
        history.push(&record);
        expected.push(notification_value(update));
    }
    assert_eq!(state.records, 7);
    assert_eq!(history.sources.len(), expected.len());
    assert_eq!(expected[0]["update"]["content"]["type"], "image");
    assert_eq!(
        expected[0]["update"]["content"]["_meta"]["imageKey"],
        "input-image"
    );
    assert_eq!(expected[0]["update"]["messageId"], "user-msg-7");
    assert_eq!(expected[1]["update"]["content"]["type"], "audio");
    assert_eq!(expected[2]["update"]["content"]["type"], "resource_link");
    assert_eq!(
        expected[3]["update"]["content"]["resource"]["text"],
        "embedded text resource"
    );
    assert_eq!(
        expected[4]["update"]["content"]["resource"]["blob"],
        "AAECAw=="
    );
    assert_eq!(
        expected[5]["update"]["content"]["_meta"]["thoughtKey"],
        "private-thought"
    );
    assert_eq!(expected[6]["update"]["toolCallId"], "tool-42");
    assert_eq!(
        expected[6]["update"]["content"].as_array().unwrap().len(),
        3
    );
    assert_eq!(expected[6]["update"]["content"][1]["type"], "diff");
    assert_eq!(
        expected[6]["update"]["content"][1]["_meta"]["diffKey"],
        "diff-42"
    );
    assert_eq!(
        expected[6]["update"]["content"][2]["terminalId"],
        "terminal-13"
    );
    assert_eq!(
        expected[6]["update"]["content"][2]["_meta"]["terminalKey"],
        "terminal-reference"
    );
    history.prefix_hash = hasher.finalize().to_hex().to_string();

    let timeline = chat_state::Timeline::default();
    let prepared = history
        .prepare(&session_id, &timeline, None)
        .expect("nested ACP facts project");
    assert_eq!(prepared.updates.len(), expected.len());
    for (index, (update, expected)) in prepared.updates.into_iter().zip(expected).enumerate() {
        let mut actual = notification_value(update);
        strip_history_projection_meta(&mut actual);
        assert_eq!(
            actual, expected,
            "nested ACP fact at physical record {index}"
        );
    }

    let invalid_update = serde_json::json!({
        "sessionId": "nested-acp",
        "update": {"sessionUpdate": "future_unknown_update"}
    });
    assert!(serde_json::from_value::<acp::SessionNotification>(invalid_update).is_err());
    let invalid_content = serde_json::json!({
        "sessionId": "nested-acp",
        "update": {
            "sessionUpdate": "user_message_chunk",
            "content": {"type": "future_unknown_content", "payload": "opaque"}
        }
    });
    assert!(serde_json::from_value::<acp::SessionNotification>(invalid_content).is_err());
}

#[test]
fn diff_review_is_a_typed_durable_history_fact() {
    let session_id = acp::SessionId::new("golden");
    let update = SessionUpdate::Grow(Box::new(
        crate::extensions::notification::SessionNotification {
            session_id: session_id.clone(),
            update: crate::extensions::notification::SessionUpdate::DiffReview {
                content: vec![crate::extensions::notification::DiffContent {
                    diff: acp::Diff::new("src/lib.rs", "-old\n+new"),
                }],
            },
            meta: Some(serde_json::json!({"eventId": "diff-review-1"})),
        },
    ));
    let fact = HistoryFact::from_update(&update)
        .expect("DiffReview converts to history")
        .expect("DiffReview is durable presentation history");
    let raw = serde_json::value::to_raw_value(&fact).expect("serialize DiffReview fact");
    let encoded = encode_presentation(0, 0, &raw).expect("encode DiffReview presentation");
    let record = decode_record(
        encoded
            .strip_suffix(b"\n")
            .expect("encoded record is newline terminated"),
    )
    .expect("decode DiffReview presentation");
    let JournalFact::Presentation { event, .. } = record.fact else {
        panic!("expected DiffReview presentation fact")
    };
    let round_trip = event
        .into_update(&session_id)
        .expect("DiffReview fact converts back")
        .expect("DiffReview remains a presentation update");
    assert_eq!(notification_value(round_trip), notification_value(update));
}

fn checksum_fixture_record() -> Vec<u8> {
    let fact = HistoryFact {
        meta: None,
        content: HistoryContent::User(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new("payload-value"),
        ))),
    };
    let raw = serde_json::value::to_raw_value(&fact).expect("serialize fixture fact");
    encode_presentation(0, 0, &raw).expect("encode fixture record")
}

#[test]
fn valid_json_fact_mutation_breaks_record_checksum() {
    let mut bytes = checksum_fixture_record();
    let offset = bytes
        .windows(b"payload-value".len())
        .position(|window| window == b"payload-value")
        .expect("fact payload is present");
    bytes[offset + b"payload-valu".len()] = b'f';
    assert!(decode_record(bytes.strip_suffix(b"\n").unwrap()).is_err());
}

#[test]
fn physical_position_mutation_breaks_record_checksum() {
    let bytes = checksum_fixture_record();
    let mut record: Value = serde_json::from_slice(&bytes).expect("record is JSON");
    record["seq"] = Value::from(1u64);
    let changed = serde_json::to_vec(&record).expect("serialize changed record");
    assert!(decode_record(&changed).is_err());
}

#[test]
fn unknown_record_version_is_rejected() {
    let bytes = checksum_fixture_record();
    let mut record: Value = serde_json::from_slice(&bytes).expect("record is JSON");
    record["version"] = Value::from(99u64);
    let changed = serde_json::to_vec(&record).expect("serialize changed version");
    assert!(decode_record(&changed).is_err());
}

#[test]
fn writer_counter_floor_covers_derived_receipts_and_hidden_physical_positions() {
    let mut history = HistorySnapshot {
        sources: (0..8)
            .map(|event| HistorySource::Domain {
                event,
                response_anchor: None,
            })
            .collect(),
        ..HistorySnapshot::default()
    };
    assert_eq!(
        history.event_counter_floor(),
        8,
        "even noReplay with no persisted presentation IDs reserves all synthesized receipt positions"
    );
    history.sources.push(HistorySource::Presentation {
        at_ms: 0,
        fact: Box::new(HistoryFact {
            meta: Some(
                serde_json::json!({"eventId":"session-100"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            content: HistoryContent::Assistant(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new("hidden row"),
            ))),
        }),
    });
    assert_eq!(
        history.event_counter_floor(),
        101,
        "a stored identity above the physical cut remains reserved"
    );
}
