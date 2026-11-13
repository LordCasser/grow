//! Upgrade legacy replay fixtures with canonical admitted-response history.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use acp_transport::protocol as acp;
use chat_state::{
    MessageCause, MessageEvent, RequestEvent, RequestUsage, ResponseAdmission,
    ResponseAdmissionIdentity, StepEvent, Timeline, TimelineEventKind, TurnEvent, TurnIdentity,
    TurnInputKind, TurnTerminal, TurnTerminalSource,
};
use sampling_types::ConversationItem;

use crate::session::history::{HistoryFact, ResponseAnchor};
use crate::session::info::Info;
use crate::session::storage::SessionUpdate;

use super::replay::SessionSpec;

const FIXTURE_BUDGET: u64 = 512 * 1024 * 1024;

/// Replay an existing synthetic journal through the ordinary storage writer.
/// Sync once per MiB and on the final record to exercise periodic index
/// publication without opening the session for recovery.
pub async fn commit_fixture_through_normal_writer(
    root: &Path,
    info: &Info,
    source_directory: &Path,
    model_id: &str,
) {
    use std::io::BufRead;

    use crate::session::history::JournalFact;
    use crate::session::storage::{JsonlStorageAdapter, StorageAdapter};

    let storage = JsonlStorageAdapter::with_root(root.to_path_buf());
    storage
        .init_session(info, crate::agent::models::ModelId::new(model_id))
        .await
        .expect("create fixture session through the normal writer");
    let source = std::fs::File::open(source_directory.join(crate::session::storage::TIMELINE_FILE))
        .expect("open synthetic journal");
    let mut remaining = source.metadata().expect("stat synthetic journal").len();
    let mut reader = std::io::BufReader::new(source);
    let mut line = Vec::new();
    let mut unpublished = 0;
    while remaining != 0 {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).expect("read fixture fact");
        assert_eq!(line.last(), Some(&b'\n'));
        remaining -= read as u64;
        unpublished += read;
        let durable = unpublished >= 1024 * 1024 || remaining == 0;
        let record = crate::session::history::decode_record(&line[..read - 1])
            .expect("validate synthetic fact");
        match record.fact {
            JournalFact::Domain { event, .. } => {
                if durable {
                    storage.append_timeline_event_durable(info, &event).await
                } else {
                    storage.append_timeline_event(info, &event).await
                }
                .expect("commit domain fact");
            }
            JournalFact::Presentation { event, .. } => {
                let update = if let crate::session::history::HistoryContent::Candidate(key) =
                    &event.content
                {
                    candidate_anchor(info, &key.request_id, event.event_id().unwrap())
                } else {
                    event.into_update(&info.id).unwrap().unwrap()
                };
                if durable {
                    storage.append_update_durable_commit_aware(info, &update).await.unwrap();
                } else {
                    storage.append_update(info, &update).await.unwrap();
                }
            }
        }
        if durable {
            unpublished = 0;
        }
    }
}

/// Convert the legacy input fixture into an ordered physical Timeline journal
/// with durable response admissions. The legacy updates file is an intermediate
/// input only and is removed after the journal has been written.
pub fn write_admitted_history(
    info: &Info,
    directory: &Path,
    spec: &SessionSpec,
    model_id: &str,
    missing_projection_every: usize,
    model_route: Option<(&str, &sampling_types::ModelImageInputKey)>,
) {
    write_admitted_history_with_closed_observations(
        info,
        directory,
        spec,
        model_id,
        missing_projection_every,
        0,
        model_route,
    );
}

/// Write admitted history with inert, committed presentation observations
/// before the fixed working turn. These records grow the closed history without
/// changing the model-visible Timeline Surface.
pub fn write_admitted_history_with_closed_observations(
    info: &Info,
    directory: &Path,
    spec: &SessionSpec,
    model_id: &str,
    _missing_projection_every: usize,
    closed_observations: usize,
    model_route: Option<(&str, &sampling_types::ModelImageInputKey)>,
) -> String {
    write_admitted_history_with_closed_observations_kind(
        info,
        directory,
        spec,
        model_id,
        _missing_projection_every,
        closed_observations,
        ClosedHistoryKind::PresentationNotice,
        model_route,
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClosedHistoryKind {
    #[default]
    PresentationNotice,
    DomainObservation,
}

/// Variant of the fixed-surface fixture that can grow closed history as inert
/// Shell diagnostic Timeline events instead of presentation records.
pub fn write_admitted_history_with_closed_observations_kind(
    info: &Info,
    directory: &Path,
    spec: &SessionSpec,
    model_id: &str,
    _missing_projection_every: usize,
    closed_observations: usize,
    closed_history_kind: ClosedHistoryKind,
    model_route: Option<(&str, &sampling_types::ModelImageInputKey)>,
) -> String {
    let updates_path = directory.join(crate::session::storage::UPDATES_FILE);
    let legacy_updates = std::fs::read_to_string(&updates_path).expect("read legacy updates");
    let legacy_updates = legacy_updates
        .lines()
        .map(|line| {
            crate::session::storage::SessionUpdateEnvelope::from_str(line)
                .expect("parse legacy update")
        })
        .collect::<Vec<_>>();

    let mut groups = Vec::with_capacity(spec.turns);
    let mut input = legacy_updates.into_iter();
    for turn_index in 0..spec.turns {
        let (user_update, user_text) = take_user(input.next(), turn_index);
        let mut catalogs = Vec::with_capacity(spec.acu_per_turn);
        for _ in 0..spec.acu_per_turn {
            let update = input.next().expect("legacy fixture missing catalog update");
            assert!(
                is_catalog(&update),
                "expected catalog update in turn {turn_index}"
            );
            catalogs.push(update);
        }
        let mut assistant = String::new();
        for _ in 0..spec.agent_chunks_per_turn {
            assistant.push_str(&take_agent_text(input.next(), turn_index));
        }
        assistant.push_str(&format!("\nGROW_SEQ_{:06}", turn_index + 1));
        groups.push((user_update, user_text, catalogs, assistant));
    }
    assert!(
        input.next().is_none(),
        "legacy fixture has trailing updates"
    );

    let mut timeline = Timeline::default();
    timeline
        .append_many(
            vec![ConversationItem::system("admitted response resume fixture")],
            MessageCause::Seed,
        )
        .expect("record seed");
    if let Some((provider_model, transport)) = model_route {
        let model = crate::agent::models::ModelId::new(model_id);
        timeline
            .record(crate::session::persistence::model_change_event(
                &model,
                &model,
                None,
                None,
                provider_model,
                provider_model,
                transport,
                transport,
                "fixture_route_baseline",
                None,
            ))
            .expect("record current model route baseline");
    }

    if closed_history_kind == ClosedHistoryKind::DomainObservation {
        for observation in 0..closed_observations {
            timeline
                .record(TimelineEventKind::Observation(
                    chat_state::ObservationEvent {
                        scope: "shell".into(),
                        name: "artifact".into(),
                        turn: None,
                        step: None,
                        data: Some(serde_json::json!({
                            "payload": "x".repeat(16 * 1024),
                            "index": observation,
                        })),
                    },
                ))
                .expect("record closed-history diagnostic observation");
        }
    }

    let mut requests = Vec::with_capacity(spec.turns);
    for (turn_index, (_, user, _, assistant)) in groups.iter().enumerate() {
        let turn_number = u64::try_from(turn_index + 1).expect("turn index fits u64");
        let turn = chat_state::TurnId(turn_number);
        let step = chat_state::StepId { turn, index: 0 };
        let request_id = format!("grow-admitted-{}-{turn_number:06}", info.id.0);

        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                prompt_id: format!("grow-admitted-prompt-{turn_number:06}"),
                input_ids: Vec::new(),
                identity: TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: model_id.to_owned(),
                input_message_count: timeline.surface_len(),
                prompt_index: turn_index,
                prompt_text: user.clone(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .expect("start turn");
        timeline
            .append(ConversationItem::user(user.clone()), MessageCause::User)
            .expect("record user input");
        timeline
            .record(TimelineEventKind::Step(StepEvent::Started { id: step }))
            .expect("start step");
        timeline
            .record(TimelineEventKind::Request(RequestEvent::Started {
                id: request_id.clone(),
                turn,
                step,
                model_id: model_id.to_owned(),
                input_message_count: timeline.surface_len(),
                tool_count: 0,
            }))
            .expect("start request");
        timeline
            .record(TimelineEventKind::Request(RequestEvent::Completed {
                id: request_id.clone(),
                duration_ms: 1,
                time_to_first_token_ms: None,
                usage: RequestUsage::default(),
                response_message_count: 1,
                attempt: 1,
                provider_terminal: None,
            }))
            .expect("complete request");
        timeline
            .record(TimelineEventKind::Messages(MessageEvent {
                cause: MessageCause::Assistant,
                items: vec![ConversationItem::assistant(assistant.clone())],
                surface: chat_state::SurfaceOp::Append,
                response_admission: Some(ResponseAdmission {
                    identity: ResponseAdmissionIdentity {
                        request_id: request_id.clone(),
                        attempt: 1,
                    },
                    quarantined_tool_exchanges: 0,
                }),
            }))
            .expect("record admitted response");
        timeline
            .record(TimelineEventKind::Step(StepEvent::Ended {
                id: step,
                outcome: "completed".into(),
                duration_ms: 1,
            }))
            .expect("end step");
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 1,
                tool_count: 0,
                terminal: TurnTerminal {
                    source: TurnTerminalSource::Host,
                    stop_reason: "end_turn".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .expect("end turn");
        requests.push(request_id);
    }

    let restored = Timeline::from_events(timeline.events().to_vec()).expect("restore Timeline");
    assert_eq!(restored.surface_len(), spec.turns * 2 + 1);
    let fixed_surface_blake3 = blake3::hash(&serde_json::to_vec(restored.surface()).unwrap())
        .to_hex()
        .to_string();
    let admissions = restored.admitted_responses();
    assert_eq!(admissions.len(), spec.turns);
    assert!(
        admissions
            .iter()
            .all(|response| response.quarantined_tool_exchanges == 0)
    );
    let mut user_facts = Vec::with_capacity(spec.turns);
    let mut candidate_facts = HashMap::with_capacity(spec.turns);
    for (turn_index, (user, _, catalogs, _)) in groups.into_iter().enumerate() {
        let user_fact = HistoryFact::from_update(&user)
            .expect("convert user presentation")
            .expect("user chunk is durable history");
        user_facts.push(user_fact);
        let request_id = &requests[turn_index];
        let event_id = crate::util::event_id::generate_event_id(&info.id.0);
        let candidate = candidate_anchor(info, request_id, &event_id);
        let fact = HistoryFact::from_update(&candidate)
            .expect("convert candidate presentation")
            .expect("candidate anchor is durable history");
        candidate_facts.insert(request_id.clone(), fact);
        // Available-command catalogs are deliberately regenerated for clients;
        // keep validating the legacy fixture rows without persisting them.
        assert_eq!(catalogs.len(), spec.acu_per_turn);
    }

    let timeline_path = directory.join(crate::session::storage::TIMELINE_FILE);
    let journal_file = std::fs::File::create(&timeline_path).expect("create Timeline journal");
    let mut journal = std::io::BufWriter::new(journal_file);
    let mut physical_position = 0u64;
    let mut user_fact_index = 0usize;
    let mut candidates_placed = 0usize;
    for event in restored.events() {
        let admission = event
            .messages()
            .and_then(|message| message.response_admission.as_ref());
        let mut response_anchor = None;
        if let Some(admission) = admission {
            let fact = candidate_facts
                .remove(&admission.identity.request_id)
                .expect("candidate fact matches admitted response");
            let position = append_presentation_fact(&mut journal, &mut physical_position, &fact);
            candidates_placed += 1;
            response_anchor = Some(ResponseAnchor::Candidate { record: position });
        }

        let raw = serde_json::value::to_raw_value(event).expect("serialize domain event");
        journal
            .write_all(
                &crate::session::history::encode_domain(physical_position, &raw, response_anchor)
                    .expect("encode domain journal record"),
            )
            .expect("write domain journal record");
        physical_position = physical_position
            .checked_add(1)
            .expect("physical position does not overflow");

        if closed_history_kind == ClosedHistoryKind::PresentationNotice
            && event
                .messages()
                .is_some_and(|message| message.cause == MessageCause::Seed)
        {
            for observation in 0..closed_observations {
                let fact = HistoryFact {
                    meta: Some(acp::Meta::from_iter([
                        (
                            "eventId".into(),
                            format!("{}-closed-{observation}", info.id.0).into(),
                        ),
                        ("agentTimestampMs".into(), event.at_ms.into()),
                    ])),
                    content: crate::session::history::HistoryContent::Grow(Box::new(
                        crate::session::history::GrowHistoryFact::UiNotice(
                            crate::extensions::notification::UiNotice {
                                correlation_id: format!("pty-history-{observation:08}"),
                                category:
                                    crate::extensions::notification::UiNoticeCategory::Command,
                                subject: Some("benchmark fixture".into()),
                                description: None,
                                message: "Committed presentation history fixture".into(),
                                tone: crate::extensions::notification::UiNoticeTone::Info,
                                details: Some(format!(
                                    "Closed-history observation {observation:08}: {}",
                                    "x".repeat(16 * 1024)
                                )),
                            },
                        ),
                    )),
                };
                append_presentation_fact(&mut journal, &mut physical_position, &fact);
            }
        }

        if event
            .messages()
            .is_some_and(|message| message.cause == MessageCause::User)
        {
            let fact = user_facts
                .get(user_fact_index)
                .expect("user fact matches user Timeline event");
            append_presentation_fact(&mut journal, &mut physical_position, fact);
            user_fact_index += 1;
        }
    }
    assert_eq!(
        user_fact_index,
        user_facts.len(),
        "all user facts were placed"
    );
    assert!(
        candidate_facts.is_empty(),
        "all candidate facts were placed"
    );
    assert_eq!(candidates_placed, admissions.len());
    journal.flush().expect("flush unified Timeline journal");
    drop(journal);
    std::fs::remove_file(&updates_path).expect("remove legacy updates input");

    let updates_bytes = 0u64;
    let timeline_bytes = std::fs::metadata(&timeline_path)
        .expect("stat Timeline")
        .len();
    let rewind_path = directory.join("rewind_points.jsonl");
    let rewind_bytes = std::fs::metadata(&rewind_path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let total_bytes = updates_bytes
        .saturating_add(timeline_bytes)
        .saturating_add(rewind_bytes);
    assert!(
        total_bytes <= FIXTURE_BUDGET,
        "fixture exceeds 512 MiB: {total_bytes} bytes"
    );
    eprintln!(
        "[testkit] admitted history: turns={} admissions={} missing_projections={} updates={}B timeline={}B rewind={}B total={}B",
        spec.turns,
        admissions.len(),
        spec.turns,
        updates_bytes,
        timeline_bytes,
        rewind_bytes,
        total_bytes,
    );
    fixed_surface_blake3
}

fn append_presentation_fact(
    journal: &mut impl Write,
    physical_position: &mut u64,
    fact: &HistoryFact,
) -> u64 {
    let position = *physical_position;
    let raw = serde_json::value::to_raw_value(fact).expect("serialize presentation fact");
    journal
        .write_all(
            &crate::session::history::encode_presentation(
                position,
                chrono::Utc::now().timestamp_millis(),
                &raw,
            )
            .expect("encode presentation journal record"),
        )
        .expect("write presentation journal record");
    *physical_position = (*physical_position)
        .checked_add(1)
        .expect("physical position does not overflow");
    position
}

fn take_user(update: Option<SessionUpdate>, turn_index: usize) -> (SessionUpdate, String) {
    let SessionUpdate::Acp(notification) = update.expect("legacy fixture missing user update")
    else {
        panic!("expected ACP user update in turn {turn_index}");
    };
    let text = match &notification.update {
        acp::SessionUpdate::UserMessageChunk(chunk) => text_content(chunk),
        other => panic!("expected user chunk in turn {turn_index}, found {other:?}"),
    };
    (SessionUpdate::Acp(notification), text)
}

fn take_agent_text(update: Option<SessionUpdate>, turn_index: usize) -> String {
    let SessionUpdate::Acp(notification) = update.expect("legacy fixture missing agent chunk")
    else {
        panic!("expected ACP agent update in turn {turn_index}");
    };
    match &notification.update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => text_content(chunk),
        other => panic!("expected agent chunk in turn {turn_index}, found {other:?}"),
    }
}

fn text_content(chunk: &acp::ContentChunk) -> String {
    match &chunk.content {
        acp::ContentBlock::Text(text) => text.text.clone(),
        other => panic!("expected text fixture content, found {other:?}"),
    }
}

fn is_catalog(update: &SessionUpdate) -> bool {
    matches!(
        update,
        SessionUpdate::Acp(notification)
            if matches!(&notification.update, acp::SessionUpdate::AvailableCommandsUpdate(_))
    )
}

fn candidate_anchor(info: &Info, request_id: &str, event_id: &str) -> SessionUpdate {
    let mut notification = acp::SessionNotification::new(
        info.id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(String::new()),
        ))),
    );
    notification.meta = Some(
        serde_json::json!({
            "eventId": event_id,
            "samplingRequestId": request_id,
            "samplingAttempt": 1,
        })
        .as_object()
        .cloned()
        .expect("candidate metadata object"),
    );
    SessionUpdate::Acp(Box::new(notification))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::storage::{JsonlStorageAdapter, stream_replay_updates_at};
    use crate::session::testkit::synth::replay::prepare_legacy_session;
    use std::path::Path;

    fn visible_messages(updates: Vec<acp::SessionUpdate>) -> Vec<String> {
        updates
            .into_iter()
            .filter_map(|update| match update {
                acp::SessionUpdate::UserMessageChunk(chunk)
                | acp::SessionUpdate::AgentMessageChunk(chunk) => Some(text_content(&chunk)),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn admitted_history_derives_missing_projection_without_cache_repair() {
        let root = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let spec = SessionSpec {
            turns: 3,
            acu_per_turn: 2,
            catalog_commands: 2,
            catalog_desc_len: 12,
            agent_chunks_per_turn: 2,
            agent_chunk_len: 12,
            rewind_points: 0,
            files_per_rewind: 0,
            file_content_len: 0,
        };
        let (info, directory) = prepare_legacy_session(root.path(), cwd.path(), &spec).await;
        let contents = std::fs::read_to_string(directory.join("updates.jsonl")).unwrap();
        let prepared = crate::session::storage::prepare_replay_lines(&contents, None);
        let legacy = prepared.lines.into_iter().filter_map(|line| {
            match serde_json::from_str::<SessionUpdate>(line).unwrap() {
                SessionUpdate::Acp(notification) => Some(notification.update),
                _ => None,
            }
        });
        let mut expected = Vec::with_capacity(spec.turns * 2);
        let mut pending_answer = String::new();
        for update in legacy {
            match update {
                acp::SessionUpdate::UserMessageChunk(chunk) => {
                    if !pending_answer.is_empty() {
                        expected.push(format!(
                            "{pending_answer}\nGROW_SEQ_{:06}",
                            expected.len() / 2 + 1
                        ));
                        pending_answer.clear();
                    }
                    expected.push(text_content(&chunk));
                }
                acp::SessionUpdate::AgentMessageChunk(chunk) => {
                    pending_answer.push_str(&text_content(&chunk))
                }
                _ => {}
            }
        }
        if !pending_answer.is_empty() {
            expected.push(format!("{pending_answer}\nGROW_SEQ_{:06}", spec.turns));
        }

        write_admitted_history(&info, &directory, &spec, "test-model", 2, None);
        let updates_path = directory.join(crate::session::storage::UPDATES_FILE);
        assert!(!updates_path.exists(), "new fixture has no updates cache");
        let before_load = replay_updates(info.id.0.as_ref(), root.path());
        let before_load = visible_messages(before_load);
        assert_eq!(before_load, expected);

        let writer = JsonlStorageAdapter::with_root(root.path().to_path_buf());
        let restored = writer
            .load_session_for_write_without_updates(&info)
            .await
            .expect("writer load uses the derived history projection");
        assert_eq!(restored.timeline.admitted_responses().len(), spec.turns);
        assert!(
            restored
                .timeline
                .admitted_responses()
                .iter()
                .all(|response| response.quarantined_tool_exchanges == 0)
        );
        drop(restored);
        assert!(
            !updates_path.exists(),
            "writer load leaves derived history in journal"
        );

        let after_load = replay_updates(info.id.0.as_ref(), root.path());
        let after_load = visible_messages(after_load);
        assert_eq!(after_load, expected);
    }

    fn replay_updates(session_id: &str, grow_home: &Path) -> Vec<acp::SessionUpdate> {
        let mut updates = Vec::new();
        let emission =
            stream_replay_updates_at(session_id, grow_home, |update| updates.push(update)).unwrap();
        assert_eq!(emission, crate::session::storage::ReplayEmission::Emitted);
        updates
    }
}
