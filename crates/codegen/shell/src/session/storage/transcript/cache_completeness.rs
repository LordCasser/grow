use std::collections::BTreeSet;
use std::io;

use chat_state::{
    DisplayProjectionExpectation, InputEvent, NotificationEvent, Timeline, TimelineEventKind,
    TurnEvent, TurnId, TurnTerminalSource,
};
use sampling_types::ConversationItem;

/// Verify that each durable projection expectation has exactly one intact
/// cache row in the captured display frontier. The hash is over the canonical
/// serialized `update` value, never over timestamps or envelope formatting.
pub(super) fn validate_expected_display_projections(
    session_id: &str,
    timeline: &Timeline,
    filtered_lines: &[&str],
) -> io::Result<()> {
    let mut expected = std::collections::BTreeMap::<String, DisplayProjectionExpectation>::new();
    for event in timeline.events() {
        let TimelineEventKind::DisplayProjectionExpected(projection) = &event.kind else {
            continue;
        };
        if let Some(previous) = expected.insert(projection.event_id.clone(), projection.clone())
            && previous != *projection
        {
            return Err(invalid_projection(
                session_id,
                &projection.event_id,
                "Timeline contains conflicting expectations",
            ));
        }
    }
    if expected.is_empty() {
        return Ok(());
    }

    let mut seen = std::collections::BTreeMap::<String, (usize, String)>::new();
    for line in filtered_lines {
        let envelope: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let Some(params) = envelope.get("params") else {
            continue;
        };
        let meta = params
            .get("meta")
            .or_else(|| params.get("_meta"))
            .and_then(serde_json::Value::as_object);
        let Some(event_id) = meta
            .and_then(|meta| meta.get("eventId"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        if !expected.contains_key(event_id) {
            continue;
        }
        let Some(update) = params.get("update") else {
            return Err(invalid_projection(
                session_id,
                event_id,
                "cached row has no update payload",
            ));
        };
        let mut payload = update.clone();
        payload.sort_all_objects();
        let payload = serde_json::to_vec(&payload)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let digest = blake3::hash(&payload).to_hex().to_string();
        let entry = seen
            .entry(event_id.to_owned())
            .or_insert((0, digest.clone()));
        entry.0 += 1;
        if entry.1 != digest {
            return Err(invalid_projection(
                session_id,
                event_id,
                "duplicate event ID has different payloads",
            ));
        }
    }
    for (event_id, projection) in expected {
        match seen.get(&event_id) {
            Some((1, digest)) if digest == &projection.payload_digest => {}
            Some((0, _)) | None => {
                return Err(invalid_projection(
                    session_id,
                    &event_id,
                    "durable projection is missing from the captured display cache",
                ));
            }
            Some((_, digest)) if digest != &projection.payload_digest => {
                return Err(invalid_projection(
                    session_id,
                    &event_id,
                    "cached payload differs from its durable expectation",
                ));
            }
            Some(_) => {
                return Err(invalid_projection(
                    session_id,
                    &event_id,
                    "event ID appears more than once in the display cache",
                ));
            }
        }
    }
    Ok(())
}

fn invalid_projection(session_id: &str, event_id: &str, reason: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("session '{session_id}' display projection '{event_id}': {reason}"),
    )
}

/// Reject a captured display ledger that dropped or altered a committed user
/// echo. The current branch's SurfaceId points to its `Input::Consumed` event;
/// its exact message identity, prompt index, block count, and canonical digest
/// bind the ordered ACP chunks. We do not infer identity from time or nearby
/// updates.
pub(super) fn validate_committed_user_echoes(
    session_id: &str,
    timeline: &Timeline,
    filtered_lines: &[&str],
) -> io::Result<()> {
    let event_by_seq = timeline
        .events()
        .iter()
        .map(|event| (event.seq.get(), event))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut expected_prompt_indices = BTreeSet::new();
    let mut expected_echoes = std::collections::BTreeMap::new();
    let (surface_ids, _) = timeline.branch_transcript_with_ids();
    for id in surface_ids {
        if id.item != 0 {
            continue;
        }
        let Some(event) = event_by_seq.get(&id.event.get()) else {
            continue;
        };
        if let TimelineEventKind::Input(InputEvent::Consumed {
            item: ConversationItem::User(user),
            display_echo,
            ..
        }) = &event.kind
        {
            if let Some(prompt_index) = user.prompt_index {
                expected_prompt_indices.insert(prompt_index);
                let Some(display_echo) = display_echo else {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "session '{session_id}' has committed user input promptIndex {prompt_index} without a durable display fingerprint"
                        ),
                    ));
                };
                if display_echo.prompt_index != prompt_index
                    || display_echo.message_id.is_empty()
                    || display_echo.block_count == 0
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "session '{session_id}' has an invalid durable display fingerprint for promptIndex {prompt_index}"
                        ),
                    ));
                }
                if expected_echoes
                    .insert(display_echo.message_id.clone(), display_echo)
                    .is_some()
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "session '{session_id}' has duplicate committed user display message identities"
                        ),
                    ));
                }
            }
        }
    }

    if expected_prompt_indices.is_empty() {
        return Ok(());
    }

    let mut cached_prompt_indices = BTreeSet::new();
    let mut cached_echoes =
        std::collections::BTreeMap::<String, (usize, Vec<serde_json::Value>)>::new();
    for line in filtered_lines {
        let envelope: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if envelope.get("method").and_then(serde_json::Value::as_str) != Some("session/update") {
            continue;
        }
        let Some(update) = envelope
            .get("params")
            .and_then(|params| params.get("update"))
        else {
            continue;
        };
        if update
            .get("sessionUpdate")
            .and_then(serde_json::Value::as_str)
            != Some("user_message_chunk")
        {
            continue;
        }
        let Some(prompt_index) = update
            .get("_meta")
            .and_then(|meta| meta.get("promptIndex"))
            .and_then(serde_json::Value::as_u64)
            .and_then(|index| usize::try_from(index).ok())
        else {
            continue;
        };
        cached_prompt_indices.insert(prompt_index);
        let Some(message_id) = update
            .get("_meta")
            .and_then(|meta| meta.get("messageId"))
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let content = update
            .get("content")
            .cloned()
            .ok_or_else(|| invalid_cached_echo(session_id, "chunk has no content"))?;
        let entry = cached_echoes
            .entry(message_id.to_owned())
            .or_insert_with(|| (prompt_index, Vec::new()));
        if entry.0 != prompt_index {
            return Err(invalid_cached_echo(
                session_id,
                "message identity carries multiple prompt indices",
            ));
        }
        entry.1.push(content);
    }

    if let Some(missing) = expected_prompt_indices
        .difference(&cached_prompt_indices)
        .next()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "session '{session_id}' has committed user input promptIndex {missing} but no matching cached UserMessageChunk; display cache is incomplete"
            ),
        ));
    }
    for (message_id, expected) in expected_echoes {
        let Some((prompt_index, blocks)) = cached_echoes.get(&message_id) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "session '{session_id}' has committed user input promptIndex {} but no cached echo with messageId '{message_id}'; display cache is incomplete",
                    expected.prompt_index
                ),
            ));
        };
        let mut canonical_blocks = serde_json::Value::Array(blocks.clone());
        canonical_blocks.sort_all_objects();
        let bytes = serde_json::to_vec(&canonical_blocks)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let digest = blake3::hash(&bytes).to_hex().to_string();
        if *prompt_index != expected.prompt_index
            || blocks.len() != expected.block_count
            || digest != expected.blake3
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "session '{session_id}' has a truncated or altered cached user echo for promptIndex {} and messageId '{message_id}'; display cache is incomplete",
                    expected.prompt_index
                ),
            ));
        }
    }
    Ok(())
}

fn invalid_cached_echo(session_id: &str, detail: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("session '{session_id}' has an invalid cached user echo: {detail}"),
    )
}

/// Require the exact user-facing terminal row for each ended turn that owns a
/// committed user input on the selected branch. This includes user messages
/// materialized by notification admission, whose TurnStarted input_ids are
/// empty but whose Notification::Consumed event binds the message to its turn.
/// Recovery terminals are internal facts and do not promise a separate ACP row.
pub(super) fn validate_ended_user_turns(
    session_id: &str,
    timeline: &Timeline,
    filtered_lines: &[&str],
) -> io::Result<()> {
    let event_by_seq = timeline
        .events()
        .iter()
        .map(|event| (event.seq.get(), event))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut committed_turns = BTreeSet::<TurnId>::new();
    for id in timeline.branch_transcript_with_ids().0 {
        if id.item != 0 {
            continue;
        }
        let Some(event) = event_by_seq.get(&id.event.get()) else {
            continue;
        };
        match &event.kind {
            TimelineEventKind::Input(InputEvent::Consumed {
                turn,
                item: ConversationItem::User(_),
                ..
            })
            | TimelineEventKind::Notification(NotificationEvent::Consumed {
                turn,
                input: Some(ConversationItem::User(_)),
                ..
            }) => {
                committed_turns.insert(*turn);
            }
            _ => {}
        }
    }

    let mut started = std::collections::BTreeMap::<TurnId, &str>::new();
    let mut ended = std::collections::BTreeMap::<TurnId, bool>::new();
    for event in timeline.events() {
        match &event.kind {
            TimelineEventKind::Turn(TurnEvent::Started { id, prompt_id, .. }) => {
                started.insert(*id, prompt_id);
            }
            TimelineEventKind::Turn(TurnEvent::Ended { id, terminal, .. }) => {
                ended.insert(*id, terminal.source != TurnTerminalSource::Recovery);
            }
            _ => {}
        }
    }
    let mut expected_prompt_ids = BTreeSet::<String>::new();
    for turn in committed_turns {
        let Some(user_facing_end) = ended.get(&turn) else {
            continue;
        };
        if !user_facing_end {
            continue;
        }
        let Some(prompt_id) = started.get(&turn) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "session '{session_id}' has an ended committed turn without its durable TurnStarted prompt identity"
                ),
            ));
        };
        expected_prompt_ids.insert((*prompt_id).to_owned());
    }
    if expected_prompt_ids.is_empty() {
        return Ok(());
    }

    let mut cached_prompt_ids = BTreeSet::new();
    for line in filtered_lines {
        let envelope: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if envelope.get("method").and_then(serde_json::Value::as_str)
            != Some(crate::session::storage::GROW_SESSION_UPDATE_METHOD)
        {
            continue;
        }
        let Some(update) = envelope
            .get("params")
            .and_then(|params| params.get("update"))
        else {
            continue;
        };
        if update
            .get("sessionUpdate")
            .and_then(serde_json::Value::as_str)
            != Some("turn_completed")
        {
            continue;
        }
        if let Some(prompt_id) = update.get("prompt_id").and_then(serde_json::Value::as_str) {
            cached_prompt_ids.insert(prompt_id.to_owned());
        }
    }
    if let Some(missing) = expected_prompt_ids.difference(&cached_prompt_ids).next() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "session '{session_id}' has an ended committed turn for prompt '{missing}' but no matching cached TurnCompleted; display cache is incomplete"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use acp_transport::protocol as acp;
    use chat_state::{
        DisplayProjectionExpectation, DisplayProjectionKind, HookAggregateDecision, HookCause,
        HookEvent, HookEventType, HookGateDecision, HookGateKind, InputAdmissionDecision,
        InputIntent, InputPayloadRef, InputRoute, MessageCause, NotificationEvent,
        NotificationPayloadRef, NotificationSource, NotificationSourceVersion, TimelineEventKind,
        TurnEvent, TurnId, TurnIdentity, TurnInputKind, TurnTerminal, TurnTerminalSource,
        WorkflowTurnHandoff,
    };

    fn expectation_timeline(update: serde_json::Value) -> (Timeline, String) {
        let payload_digest = blake3::hash(&serde_json::to_vec(&update).unwrap())
            .to_hex()
            .to_string();
        let mut timeline = Timeline::default();
        timeline
            .record(TimelineEventKind::DisplayProjectionExpected(
                DisplayProjectionExpectation {
                    event_id: "event-1".into(),
                    kind: DisplayProjectionKind::WorkflowUpdated,
                    source_id: "workflow-1".into(),
                    source_revision: 3,
                    payload_digest,
                },
            ))
            .unwrap();
        (timeline, serde_json::to_string(&update).unwrap())
    }

    fn cached_projection(update: &str) -> String {
        serde_json::json!({
            "method": "_grow/session/update",
            "params": {
                "sessionId": "session",
                "update": serde_json::from_str::<serde_json::Value>(update).unwrap(),
                "meta": {"eventId": "event-1"}
            }
        })
        .to_string()
    }

    #[test]
    fn expected_projection_requires_exact_captured_row_and_payload() {
        let update = serde_json::json!({"kind": "WorkflowUpdated", "revision": 3});
        let (timeline, update) = expectation_timeline(update);
        let exact = cached_projection(&update);
        validate_expected_display_projections("session", &timeline, &[&exact]).unwrap();

        let missing = validate_expected_display_projections("session", &timeline, &[]).unwrap_err();
        assert!(
            missing
                .to_string()
                .contains("missing from the captured display cache")
        );

        let altered = cached_projection(r#"{"kind":"WorkflowUpdated","revision":4}"#);
        let changed =
            validate_expected_display_projections("session", &timeline, &[&altered]).unwrap_err();
        assert!(
            changed
                .to_string()
                .contains("differs from its durable expectation")
        );

        let duplicate =
            validate_expected_display_projections("session", &timeline, &[&exact, &exact])
                .unwrap_err();
        assert!(duplicate.to_string().contains("more than once"));
    }

    #[test]
    fn rewind_filter_does_not_hide_a_row_from_projection_integrity_check() {
        let update = serde_json::json!({"kind": "WorkflowUpdated", "revision": 3});
        let (timeline, update) = expectation_timeline(update);
        let user_zero = cached_user(0);
        let user_one = cached_user(1);
        let projection = cached_projection(&update);
        let rewind = serde_json::json!({
            "method": "_grow/session/update",
            "params": {
                "sessionId": "session",
                "update": {
                    "sessionUpdate": "rewind_marker",
                    "target_prompt_index": 1,
                    "created_at": "2026-10-19T00:00:00Z"
                }
            }
        })
        .to_string();
        let captured = vec![
            user_zero.as_str(),
            user_one.as_str(),
            projection.as_str(),
            rewind.as_str(),
        ];
        let selected_branch = crate::session::storage::filter_rewind_lines(captured.clone());

        assert!(!selected_branch.contains(&projection.as_str()));
        validate_expected_display_projections("session", &timeline, &captured).unwrap();
        assert!(
            validate_expected_display_projections("session", &timeline, &selected_branch).is_err()
        );
    }

    fn add_user(timeline: &mut Timeline, input_id: &str, turn_id: u64, prompt_index: usize) {
        let text = format!("prompt-{prompt_index}");
        add_user_with_echo_blocks(
            timeline,
            input_id,
            turn_id,
            prompt_index,
            TurnTerminalSource::Host,
            vec![serde_json::json!({"type": "text", "text": text})],
        );
    }

    fn add_user_with_echo_blocks(
        timeline: &mut Timeline,
        input_id: &str,
        turn_id: u64,
        prompt_index: usize,
        terminal_source: TurnTerminalSource,
        blocks: Vec<serde_json::Value>,
    ) {
        timeline
            .record(TimelineEventKind::Input(InputEvent::Submitted {
                input_id: input_id.into(),
                intent: InputIntent::Prompt,
                payload_ref: InputPayloadRef {
                    blake3: blake3::hash(input_id.as_bytes()).to_hex().to_string(),
                    bytes: input_id.len() as u64,
                },
            }))
            .unwrap();
        let occurrence_id = format!("hook-{input_id}");
        timeline
            .record(TimelineEventKind::Hook(HookEvent::Triggered {
                occurrence_id: occurrence_id.clone(),
                event: HookEventType::UserPromptSubmit,
                gate: HookGateKind::Prompt,
                cause: HookCause::Input {
                    input_id: input_id.into(),
                },
                config_generation: 1,
                handlers: Vec::new(),
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Hook(HookEvent::Completed {
                occurrence_id,
                decision: HookAggregateDecision::Prompt {
                    decision: HookGateDecision::Allow,
                },
            }))
            .unwrap();
        timeline
            .record(TimelineEventKind::Input(InputEvent::AdmissionResolved {
                input_id: input_id.into(),
                decision: InputAdmissionDecision::Allow,
                route: Some(InputRoute::Fifo),
                supersedes: Vec::new(),
            }))
            .unwrap();
        let turn = TurnId(turn_id);
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                prompt_id: format!("prompt-{prompt_index}"),
                id: turn,
                input_ids: vec![input_id.into()],
                identity: TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "user".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index,
                prompt_text: format!("prompt-{prompt_index}"),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let text = format!("prompt-{prompt_index}");
        let mut user = ConversationItem::user(text.clone());
        user.set_prompt_index(prompt_index);
        let message_id = format!("prompt-{prompt_index}");
        let mut canonical_blocks = serde_json::Value::Array(blocks.clone());
        canonical_blocks.sort_all_objects();
        let digest = blake3::hash(&serde_json::to_vec(&canonical_blocks).unwrap())
            .to_hex()
            .to_string();
        timeline
            .record(TimelineEventKind::Input(InputEvent::Consumed {
                display_echo: Some(chat_state::UserDisplayEcho {
                    message_id,
                    prompt_index,
                    block_count: blocks.len(),
                    blake3: digest,
                }),
                input_ids: vec![input_id.into()],
                turn,
                item: user,
                skill_loads: Vec::new(),
            }))
            .unwrap();
        if terminal_source == TurnTerminalSource::Recovery {
            timeline.recover_interrupted().unwrap();
        } else {
            timeline
                .record(TimelineEventKind::Turn(TurnEvent::Ended {
                    id: turn,
                    outcome: "completed".into(),
                    duration_ms: 0,
                    tool_count: 0,
                    terminal: TurnTerminal {
                        source: terminal_source,
                        stop_reason: "end_turn".into(),
                        completion_kind: "completed".into(),
                    },
                    cancellation_category: None,
                    details: None,
                }))
                .unwrap();
        }
    }

    fn cached_user(prompt_index: usize) -> String {
        let text = format!("prompt-{prompt_index}");
        serde_json::json!({
            "method": "session/update",
            "params": {
                "sessionId": "session",
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": text},
                    "_meta": {"promptIndex": prompt_index, "messageId": format!("prompt-{prompt_index}")}
                }
            }
        })
        .to_string()
    }

    #[test]
    fn exact_prompt_identity_detects_missing_echo_and_ignores_unrelated_echo() {
        let mut timeline = Timeline::default();
        add_user(&mut timeline, "input-0", 1, 0);
        add_user(&mut timeline, "input-1", 2, 1);
        let unrelated = cached_user(9);
        let prompt_zero = cached_user(0);
        let error =
            validate_committed_user_echoes("session", &timeline, &[&unrelated, &prompt_zero])
                .unwrap_err();
        assert!(error.to_string().contains("promptIndex 1"));

        let prompt_one = cached_user(1);
        validate_committed_user_echoes("session", &timeline, &[&prompt_zero, &prompt_one]).unwrap();
    }

    #[test]
    fn rewind_branch_does_not_require_echo_for_discarded_input() {
        let mut timeline = Timeline::default();
        add_user(&mut timeline, "input-0", 1, 0);
        add_user(&mut timeline, "input-1", 2, 1);
        let branch = timeline.rewind_surface(0).unwrap();
        timeline.replace_all(branch, MessageCause::Rewind).unwrap();
        let cached = cached_user(0);
        validate_committed_user_echoes("session", &timeline, &[&cached]).unwrap();
    }

    fn cached_turn_completed(prompt_id: &str) -> String {
        let update = crate::session::turn_completion::build_turn_completed(
            prompt_id.into(),
            None,
            serde_json::json!("end_turn"),
            serde_json::Value::Null,
            None,
        );
        let notification = crate::extensions::notification::SessionNotification {
            session_id: acp::SessionId::new("session"),
            update,
            meta: None,
        };
        let envelope = crate::session::storage::SessionUpdate::Grow(Box::new(notification));
        serde_json::to_string(&envelope).unwrap()
    }

    fn typed_cached_turn_completed(prompt_id: &str) -> crate::session::storage::SessionUpdate {
        let cached = cached_turn_completed(prompt_id);
        let envelope: serde_json::Value = serde_json::from_str(&cached).unwrap();
        let notification: crate::extensions::notification::SessionNotification =
            serde_json::from_value(envelope["params"].clone()).unwrap();
        crate::session::storage::SessionUpdate::Grow(Box::new(notification))
    }

    fn add_workflow_notification_turn(timeline: &mut Timeline) -> String {
        let source = NotificationSource::WorkflowHandoff {
            run_id: "workflow-1".into(),
            handoff: WorkflowTurnHandoff::Completion,
        };
        let source_version = NotificationSourceVersion::Opaque {
            value: "workflow-handoff-v1:1:complete:completion".into(),
        };
        let id = chat_state::notification_id("session", &source, &source_version).unwrap();
        let payload = "workflow completed";
        timeline
            .record(TimelineEventKind::Notification(
                NotificationEvent::Received {
                    id: id.clone(),
                    owner_session_id: "session".into(),
                    source,
                    source_version,
                    payload_ref: NotificationPayloadRef {
                        blake3: blake3::hash(payload.as_bytes()).to_hex().to_string(),
                        bytes: payload.len() as u64,
                    },
                },
            ))
            .unwrap();

        let turn = TurnId(3);
        let prompt_id = "workflow-prompt".to_string();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Started {
                id: turn,
                prompt_id: prompt_id.clone(),
                input_ids: Vec::new(),
                identity: TurnIdentity {
                    origin: "workflow_handoff".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: payload.into(),
                input_kind: TurnInputKind::Prompt,
                redirect_kind: None,
            }))
            .unwrap();
        let mut input = ConversationItem::notification_drain(payload);
        input.set_prompt_index(0);
        timeline
            .record(TimelineEventKind::Notification(
                NotificationEvent::Consumed {
                    notification_ids: vec![id],
                    turn,
                    input: Some(input),
                },
            ))
            .unwrap();
        timeline
            .record(TimelineEventKind::Turn(TurnEvent::Ended {
                id: turn,
                outcome: "completed".into(),
                duration_ms: 0,
                tool_count: 0,
                terminal: TurnTerminal {
                    source: TurnTerminalSource::Host,
                    stop_reason: "end_turn".into(),
                    completion_kind: "completed".into(),
                },
                cancellation_category: None,
                details: None,
            }))
            .unwrap();
        prompt_id
    }

    #[test]
    fn ended_turn_requires_its_exact_turn_completed_identity() {
        let mut timeline = Timeline::default();
        add_user(&mut timeline, "input-0", 1, 0);

        let unrelated = cached_turn_completed("other-prompt");
        let error = validate_ended_user_turns("session", &timeline, &[&unrelated]).unwrap_err();
        assert!(error.to_string().contains("prompt-0"));
        assert!(super::super::validate_ended_user_turns(&timeline, &[]).is_err());
        assert!(
            super::super::validate_ended_user_turns(
                &timeline,
                &[typed_cached_turn_completed("other-prompt")]
            )
            .is_err()
        );

        let exact = cached_turn_completed("prompt-0");
        let envelope: serde_json::Value = serde_json::from_str(&exact).unwrap();
        assert_eq!(
            envelope["method"],
            crate::session::storage::GROW_SESSION_UPDATE_METHOD
        );
        assert_eq!(
            envelope["params"]["update"]["sessionUpdate"],
            "turn_completed"
        );
        validate_ended_user_turns("session", &timeline, &[&exact]).unwrap();
        super::super::validate_ended_user_turns(
            &timeline,
            &[typed_cached_turn_completed("prompt-0")],
        )
        .unwrap();
    }

    #[test]
    fn synthetic_workflow_turn_maps_terminal_by_durable_prompt_id() {
        let mut timeline = Timeline::default();
        let prompt_id = add_workflow_notification_turn(&mut timeline);
        let turn = timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                TimelineEventKind::Turn(TurnEvent::Started {
                    id,
                    prompt_id: actual,
                    input_ids,
                    ..
                }) if actual == &prompt_id => {
                    assert!(input_ids.is_empty());
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        assert!(timeline.branch_transcript_with_ids().1.iter().any(|item| {
            matches!(item, ConversationItem::User(user) if user.prompt_index == Some(0))
        }));

        let unrelated = cached_turn_completed("some-other-prompt");
        assert!(validate_ended_user_turns("session", &timeline, &[&unrelated]).is_err());
        assert!(
            super::super::validate_ended_user_turns(
                &timeline,
                &[typed_cached_turn_completed("some-other-prompt")]
            )
            .is_err()
        );
        let exact = cached_turn_completed(&prompt_id);
        validate_ended_user_turns("session", &timeline, &[&exact]).unwrap();
        super::super::validate_ended_user_turns(
            &timeline,
            &[typed_cached_turn_completed(&prompt_id)],
        )
        .unwrap();
        assert_eq!(turn, TurnId(3));
    }

    #[test]
    fn rewind_does_not_require_completion_for_a_discarded_turn() {
        let mut timeline = Timeline::default();
        add_user(&mut timeline, "input-0", 1, 0);
        add_user(&mut timeline, "input-1", 2, 1);
        let branch = timeline.rewind_surface(0).unwrap();
        timeline.replace_all(branch, MessageCause::Rewind).unwrap();

        let exact = cached_turn_completed("prompt-0");
        validate_ended_user_turns("session", &timeline, &[&exact]).unwrap();
        super::super::validate_ended_user_turns(
            &timeline,
            &[typed_cached_turn_completed("prompt-0")],
        )
        .unwrap();
    }

    #[test]
    fn recovery_terminal_does_not_require_turn_completed_row() {
        let mut timeline = Timeline::default();
        add_user_with_echo_blocks(
            &mut timeline,
            "input-0",
            1,
            0,
            TurnTerminalSource::Recovery,
            vec![serde_json::json!({"type": "text", "text": "prompt-0"})],
        );
        validate_ended_user_turns("session", &timeline, &[]).unwrap();
        super::super::validate_ended_user_turns(&timeline, &[]).unwrap();
    }

    #[test]
    fn echo_fingerprint_detects_truncation_and_missing_blocks() {
        let mut timeline = Timeline::default();
        add_user_with_echo_blocks(
            &mut timeline,
            "input-0",
            1,
            0,
            TurnTerminalSource::Host,
            vec![
                serde_json::json!({"type": "text", "text": "prompt-0"}),
                serde_json::json!({"type": "image", "mimeType": "image/png", "data": "aW1hZ2U="}),
            ],
        );
        let exact_text = serde_json::json!({
            "method": "session/update",
            "params": {"update": {
                "sessionUpdate": "user_message_chunk",
                "content": {"text": "prompt-0", "type": "text"},
                "_meta": {"promptIndex": 0, "messageId": "prompt-0"}
            }}
        })
        .to_string();
        let image = serde_json::json!({
            "method": "session/update",
            "params": {"update": {
                "sessionUpdate": "user_message_chunk",
                "content": {"data": "aW1hZ2U=", "mimeType": "image/png", "type": "image"},
                "_meta": {"promptIndex": 0, "messageId": "prompt-0"}
            }}
        })
        .to_string();
        validate_committed_user_echoes("session", &timeline, &[&exact_text, &image]).unwrap();

        let missing_block =
            validate_committed_user_echoes("session", &timeline, &[&exact_text]).unwrap_err();
        assert!(missing_block.to_string().contains("truncated or altered"));

        let truncated = serde_json::json!({
            "method": "session/update",
            "params": {"update": {
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": "prompt-"},
                "_meta": {"promptIndex": 0, "messageId": "prompt-0"}
            }}
        })
        .to_string();
        let error =
            validate_committed_user_echoes("session", &timeline, &[&truncated]).unwrap_err();
        assert!(error.to_string().contains("truncated or altered"));
    }
}
