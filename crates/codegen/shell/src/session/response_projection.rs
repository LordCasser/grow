use acp::SessionId;
use acp_transport::protocol as acp;
use chat_state::AdmittedResponse;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub(crate) const RESPONSE_REPLAY_PROJECTION_VERSION: u16 = 1;
pub(crate) const RESPONSE_REPLAY_PROJECTION_METHOD: &str = "_grow/response-replay-projection";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseReplayDisposition {
    Admitted,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResponseReplayProjection {
    pub version: u16,
    pub request_id: String,
    pub attempt: u32,
    pub timeline_event: u64,
    pub digest: String,
    pub disposition: ResponseReplayDisposition,
    pub updates: Vec<acp::SessionNotification>,
}

#[derive(Serialize)]
struct DigestInput<'a> {
    version: u16,
    request_id: &'a str,
    attempt: u32,
    timeline_event: u64,
    quarantined_tool_exchanges: usize,
    items: &'a [sampling_types::ConversationItem],
}

pub(crate) fn project_admitted_response(
    session_id: &SessionId,
    response: &AdmittedResponse,
) -> Result<ResponseReplayProjection, serde_json::Error> {
    let digest_bytes = serde_json::to_vec(&DigestInput {
        version: RESPONSE_REPLAY_PROJECTION_VERSION,
        request_id: &response.identity.request_id,
        attempt: response.identity.attempt,
        timeline_event: response.event_seq.get(),
        quarantined_tool_exchanges: response.quarantined_tool_exchanges,
        items: &response.items,
    })?;
    let digest = format!("{:x}", Sha256::digest(digest_bytes));
    let disposition = if response.quarantined_tool_exchanges == 0 {
        ResponseReplayDisposition::Admitted
    } else {
        ResponseReplayDisposition::Discarded
    };
    let mut updates = Vec::new();
    if disposition == ResponseReplayDisposition::Admitted {
        for item in &response.items {
            let (kind, text) = match item {
                sampling_types::ConversationItem::Reasoning(reasoning) => {
                    (true, reasoning.text.as_ref())
                }
                sampling_types::ConversationItem::Assistant(assistant) => {
                    (false, assistant.content.as_ref())
                }
                _ => continue,
            };
            if text.is_empty() {
                continue;
            }
            let update = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                text.to_owned(),
            )));
            let update = if kind {
                acp::SessionUpdate::AgentThoughtChunk(update)
            } else {
                acp::SessionUpdate::AgentMessageChunk(update)
            };
            let mut notification = acp::SessionNotification::new(session_id.clone(), update);
            notification.meta = Some(
                serde_json::json!({
                    "responseProjection": true,
                    "samplingRequestId": response.identity.request_id,
                    "samplingAttempt": response.identity.attempt,
                })
                .as_object()
                .cloned()
                .expect("object literal"),
            );
            updates.push(notification);
        }
    }
    Ok(ResponseReplayProjection {
        version: RESPONSE_REPLAY_PROJECTION_VERSION,
        request_id: response.identity.request_id.clone(),
        attempt: response.identity.attempt,
        timeline_event: response.event_seq.get(),
        digest,
        disposition,
        updates,
    })
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ProjectionObservation<'a> {
    Candidate(&'a (String, u32)),
    Projection(&'a ResponseReplayProjection),
    Other,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PlannedProjection {
    Existing(usize),
    Canonical(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectionPlan {
    pub(crate) entries: Vec<PlannedProjection>,
    pub(crate) changed: bool,
}

pub(crate) fn plan_response_projections(
    timeline: &chat_state::Timeline,
    canonicals: &[ResponseReplayProjection],
    observations: &[ProjectionObservation<'_>],
) -> std::io::Result<ProjectionPlan> {
    let admitted = timeline.admitted_responses();
    let active_by_identity = admitted
        .iter()
        .enumerate()
        .map(|(index, response)| {
            (
                (
                    response.identity.request_id.as_str(),
                    response.identity.attempt,
                ),
                index,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut candidate_anchor = vec![None; canonicals.len()];
    let mut exact_indices = vec![Vec::new(); canonicals.len()];
    let mut suppressed = vec![false; observations.len()];
    let mut changed = false;

    for (physical_index, observation) in observations.iter().enumerate() {
        match observation {
            ProjectionObservation::Candidate(identity) => {
                suppressed[physical_index] = true;
                changed = true;
                if let Some(&canonical_index) =
                    active_by_identity.get(&(identity.0.as_str(), identity.1))
                {
                    candidate_anchor[canonical_index].get_or_insert(physical_index);
                }
            }
            ProjectionObservation::Projection(existing) => {
                let identity = (existing.request_id.as_str(), existing.attempt);
                let Some(&canonical_index) = active_by_identity.get(&identity) else {
                    suppressed[physical_index] = true;
                    changed = true;
                    continue;
                };
                if !existing.exact_match(&canonicals[canonical_index]) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "response replay projection conflict",
                    ));
                }
                exact_indices[canonical_index].push(physical_index);
                suppressed[physical_index] = true;
            }
            ProjectionObservation::Other => {}
        }
    }

    let mut insertions = std::collections::BTreeMap::<usize, Vec<PlannedProjection>>::new();
    for (canonical_index, response) in admitted.iter().enumerate() {
        let exact = &exact_indices[canonical_index];
        let anchor = candidate_anchor[canonical_index]
            .or_else(|| exact.first().copied())
            .unwrap_or(observations.len());
        if anchor == observations.len()
            && !timeline.response_projection_tail_safe(response.event_seq)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing response projection cannot be safely ordered at cache tail",
            ));
        }
        let retain_physical = candidate_anchor[canonical_index].is_none() && exact.len() == 1;
        if !retain_physical {
            changed = true;
        }
        insertions
            .entry(anchor)
            .or_default()
            .push(if retain_physical {
                PlannedProjection::Existing(exact[0])
            } else {
                PlannedProjection::Canonical(canonical_index)
            });
    }

    let mut entries = Vec::with_capacity(observations.len() + canonicals.len());
    for physical_index in 0..=observations.len() {
        if let Some(items) = insertions.get(&physical_index) {
            entries.extend(items.iter().copied());
        }
        if physical_index < observations.len() && !suppressed[physical_index] {
            entries.push(PlannedProjection::Existing(physical_index));
        }
    }
    Ok(ProjectionPlan { entries, changed })
}

pub(crate) fn reconcile_response_projections(
    session_id: &SessionId,
    timeline: &chat_state::Timeline,
    updates: Vec<crate::session::storage::SessionUpdate>,
) -> std::io::Result<Vec<crate::session::storage::SessionUpdate>> {
    use crate::session::storage::SessionUpdate;
    let canonicals = timeline
        .admitted_responses()
        .iter()
        .map(|response| {
            project_admitted_response(session_id, response)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let identities = updates
        .iter()
        .map(|update| match update {
            SessionUpdate::Acp(notification) => candidate_identity(notification),
            _ => None,
        })
        .collect::<Vec<_>>();
    let observations = updates
        .iter()
        .enumerate()
        .map(|(index, update)| match update {
            SessionUpdate::Acp(_) => identities[index].as_ref().map_or(
                ProjectionObservation::Other,
                ProjectionObservation::Candidate,
            ),
            SessionUpdate::ResponseReplayProjection(projection) => {
                ProjectionObservation::Projection(projection)
            }
            _ => ProjectionObservation::Other,
        })
        .collect::<Vec<_>>();
    let plan = plan_response_projections(timeline, &canonicals, &observations)?;
    Ok(plan
        .entries
        .into_iter()
        .map(|entry| match entry {
            PlannedProjection::Existing(index) => updates[index].clone(),
            PlannedProjection::Canonical(index) => {
                SessionUpdate::ResponseReplayProjection(Box::new(canonicals[index].clone()))
            }
        })
        .collect())
}

fn candidate_identity(notification: &acp::SessionNotification) -> Option<(String, u32)> {
    let meta = notification.meta.as_ref()?;
    let request_id = meta
        .get("samplingRequestId")
        .and_then(serde_json::Value::as_str)?;
    let attempt = meta
        .get("samplingAttempt")
        .and_then(serde_json::Value::as_u64)?;
    Some((request_id.to_owned(), u32::try_from(attempt).ok()?))
}

impl ResponseReplayProjection {
    pub(crate) fn same_key(&self, other: &Self) -> bool {
        self.request_id == other.request_id
            && self.attempt == other.attempt
            && self.timeline_event == other.timeline_event
    }

    pub(crate) fn exact_match(&self, other: &Self) -> bool {
        self.same_key(other)
            && self.version == other.version
            && self.digest == other.digest
            && self.disposition == other.disposition
            && self.updates == other.updates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admitted(
        items: Vec<sampling_types::ConversationItem>,
        quarantined: usize,
    ) -> AdmittedResponse {
        AdmittedResponse {
            event_seq: chat_state::EventSeq::new(17),
            identity: chat_state::ResponseAdmissionIdentity {
                request_id: "request-1".into(),
                attempt: 2,
            },
            items,
            quarantined_tool_exchanges: quarantined,
        }
    }

    #[test]
    fn healthy_projection_is_deterministic_and_has_no_live_event_ids() {
        let response = admitted(
            vec![
                sampling_types::ConversationItem::Reasoning(
                    sampling_types::synthesized_reasoning_item("visible thought"),
                ),
                sampling_types::ConversationItem::assistant("answer"),
            ],
            0,
        );
        let session_id = SessionId::new("session-1");
        let first = project_admitted_response(&session_id, &response).unwrap();
        let second = project_admitted_response(&session_id, &response).unwrap();
        assert!(first.exact_match(&second));
        assert_eq!(first.disposition, ResponseReplayDisposition::Admitted);
        assert_eq!(first.updates.len(), 2);
        assert!(first.updates.iter().all(|notification| {
            notification
                .meta
                .as_ref()
                .is_none_or(|meta| !meta.contains_key("eventId"))
        }));
    }

    fn validated_timeline(
        request_id: &str,
        attempt: u32,
        items: Vec<sampling_types::ConversationItem>,
        quarantined_tool_exchanges: usize,
    ) -> chat_state::Timeline {
        let mut timeline = chat_state::Timeline::default();
        let turn = chat_state::TurnId(1);
        let step = chat_state::StepId { turn, index: 0 };
        timeline
            .record(chat_state::TimelineEventKind::Turn(
                chat_state::TurnEvent::Started {
                    id: turn,
                    input_ids: Vec::new(),
                    identity: chat_state::TurnIdentity {
                        origin: "user".into(),
                        turn_kind: "internal".into(),
                        goal_id: None,
                        goal_definition_revision: None,
                        stage_id: None,
                    },
                    model_id: "model".into(),
                    input_message_count: 0,
                    prompt_index: 0,
                    prompt_text: "test".into(),
                    input_kind: chat_state::TurnInputKind::Prompt,
                    redirect_kind: None,
                },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Step(
                chat_state::StepEvent::Started { id: step },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Request(
                chat_state::RequestEvent::Started {
                    id: request_id.into(),
                    turn,
                    step,
                    model_id: "model".into(),
                    input_message_count: 0,
                    tool_count: 0,
                },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Request(
                chat_state::RequestEvent::Completed {
                    id: request_id.into(),
                    duration_ms: 1,
                    time_to_first_token_ms: None,
                    usage: chat_state::RequestUsage::default(),
                    response_message_count: items.len(),
                    attempt,
                    provider_terminal: None,
                },
            ))
            .unwrap();
        timeline
            .record(chat_state::TimelineEventKind::Messages(
                chat_state::MessageEvent {
                    cause: chat_state::MessageCause::Assistant,
                    items,
                    surface: chat_state::SurfaceOp::Append,
                    response_admission: Some(chat_state::ResponseAdmission {
                        identity: chat_state::ResponseAdmissionIdentity {
                            request_id: request_id.into(),
                            attempt,
                        },
                        quarantined_tool_exchanges,
                    }),
                },
            ))
            .unwrap();
        timeline
    }

    fn response_projection_update(
        projection: ResponseReplayProjection,
    ) -> crate::session::storage::SessionUpdate {
        crate::session::storage::SessionUpdate::ResponseReplayProjection(Box::new(projection))
    }

    fn candidate_update(
        session_id: &SessionId,
        request_id: &str,
        attempt: u32,
        text: &str,
    ) -> crate::session::storage::SessionUpdate {
        let mut notification = acp::SessionNotification::new(
            session_id.clone(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
        );
        notification.meta = Some(
            serde_json::json!({
                "samplingRequestId": request_id,
                "samplingAttempt": attempt,
            })
            .as_object()
            .cloned()
            .unwrap(),
        );
        crate::session::storage::SessionUpdate::Acp(Box::new(notification))
    }

    fn independent_update(
        session_id: &SessionId,
        text: &str,
    ) -> crate::session::storage::SessionUpdate {
        crate::session::storage::SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            session_id.clone(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
        )))
    }

    fn tool_update(session_id: &SessionId) -> crate::session::storage::SessionUpdate {
        let tool = acp::ToolCallUpdate::new(
            acp::ToolCallId::new("tool-1"),
            acp::ToolCallUpdateFields::new().title(Some("tool".into())),
        );
        crate::session::storage::SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
            session_id.clone(),
            acp::SessionUpdate::ToolCallUpdate(tool),
        )))
    }

    #[test]
    fn reconciliation_synthesizes_missing_projection_once() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let output = reconcile_response_projections(&session_id, &timeline, Vec::new()).unwrap();
        assert_eq!(output.len(), 1);
        assert!(matches!(
            output[0],
            crate::session::storage::SessionUpdate::ResponseReplayProjection(_)
        ));
    }

    #[test]
    fn reconciliation_collapses_exact_duplicates() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let projection = project_admitted_response(&session_id, &response).unwrap();
        let updates = vec![
            response_projection_update(projection.clone()),
            response_projection_update(projection),
        ];
        let output = reconcile_response_projections(&session_id, &timeline, updates).unwrap();
        assert_eq!(output.len(), 1);
    }

    #[test]
    fn reconciliation_rejects_wrong_timeline_event() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let mut projection = project_admitted_response(&session_id, &response).unwrap();
        projection.timeline_event += 1;
        let error = reconcile_response_projections(
            &session_id,
            &timeline,
            vec![response_projection_update(projection)],
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn reconciliation_rejects_changed_digest() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let mut projection = project_admitted_response(&session_id, &response).unwrap();
        projection.digest = "changed".into();
        let error = reconcile_response_projections(
            &session_id,
            &timeline,
            vec![response_projection_update(projection)],
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn reconciliation_anchors_before_later_independent_and_tool_rows() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let projection = project_admitted_response(&session_id, &response).unwrap();
        let updates = vec![
            candidate_update(&session_id, "request-1", 2, "preview"),
            independent_update(&session_id, "independent"),
            tool_update(&session_id),
            response_projection_update(projection.clone()),
        ];
        let output = reconcile_response_projections(&session_id, &timeline, updates).unwrap();
        assert!(matches!(
            output[0],
            crate::session::storage::SessionUpdate::ResponseReplayProjection(_)
        ));
        assert_eq!(output.len(), 3);
        assert!(matches!(
            output[1],
            crate::session::storage::SessionUpdate::Acp(_)
        ));
        assert!(matches!(
            output[2],
            crate::session::storage::SessionUpdate::Acp(_)
        ));
    }

    #[test]
    fn reconciliation_discards_earlier_candidate_and_reconstructs_admitted_attempt() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let output = reconcile_response_projections(
            &session_id,
            &timeline,
            vec![candidate_update(&session_id, "request-1", 1, "discarded")],
        )
        .unwrap();
        assert_eq!(output.len(), 1);
        let crate::session::storage::SessionUpdate::ResponseReplayProjection(projection) =
            &output[0]
        else {
            panic!("expected projection")
        };
        assert_eq!(projection.attempt, 2);
    }

    #[test]
    fn reconciliation_rejects_unsafe_unanchored_projection() {
        let session_id = SessionId::new("session-1");
        let mut timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        timeline
            .record(chat_state::TimelineEventKind::Request(
                chat_state::RequestEvent::Started {
                    id: "request-2".into(),
                    turn: chat_state::TurnId(1),
                    step: chat_state::StepId {
                        turn: chat_state::TurnId(1),
                        index: 0,
                    },
                    model_id: "model".into(),
                    input_message_count: 1,
                    tool_count: 0,
                },
            ))
            .unwrap();
        let error = reconcile_response_projections(&session_id, &timeline, Vec::new()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn quarantine_records_discard_without_raw_preview() {
        let response = admitted(
            vec![sampling_types::ConversationItem::assistant(
                "malformed preview",
            )],
            1,
        );
        let projection =
            project_admitted_response(&SessionId::new("session-1"), &response).unwrap();
        assert_eq!(projection.disposition, ResponseReplayDisposition::Discarded);
        assert!(projection.updates.is_empty());
    }

    fn raw_line(update: &crate::session::storage::SessionUpdate) -> String {
        serde_json::to_string(update).unwrap()
    }

    fn serialized_updates(updates: Vec<crate::session::storage::SessionUpdate>) -> Vec<String> {
        updates
            .iter()
            .map(|update| serde_json::to_string(update).unwrap())
            .collect()
    }

    fn raw_typed_parity(
        session_id: &SessionId,
        timeline: &chat_state::Timeline,
        updates: &[crate::session::storage::SessionUpdate],
    ) -> (Vec<String>, Vec<String>) {
        let typed = reconcile_response_projections(session_id, timeline, updates.to_vec()).unwrap();
        let lines = updates.iter().map(raw_line).collect::<Vec<_>>();
        let raw = crate::session::storage::reconcile_raw_replay_lines(
            session_id,
            timeline,
            lines.iter().map(String::as_str).collect(),
        )
        .unwrap();
        let raw_typed = raw
            .lines
            .iter()
            .map(|line| {
                crate::session::storage::SessionUpdateEnvelope::from_str(line.as_str()).unwrap()
            })
            .collect::<Vec<_>>();
        (
            serialized_updates(typed),
            raw_typed.into_iter().map(|u| raw_line(&u)).collect(),
        )
    }

    #[test]
    fn raw_and_typed_reconciliation_have_identical_projection_semantics() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let projection = project_admitted_response(&session_id, &response).unwrap();

        let scenarios = [
            Vec::new(),
            vec![
                candidate_update(&session_id, "request-1", 2, "preview"),
                independent_update(&session_id, "independent"),
                tool_update(&session_id),
                response_projection_update(projection.clone()),
            ],
            vec![
                response_projection_update(projection.clone()),
                response_projection_update(projection.clone()),
            ],
            vec![candidate_update(&session_id, "request-1", 1, "discarded")],
        ];

        for updates in scenarios {
            let (typed, raw) = raw_typed_parity(&session_id, &timeline, &updates);
            assert_eq!(raw, typed);
        }
    }

    #[test]
    fn raw_and_typed_reconciliation_both_fail_closed_on_projection_conflict() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let mut conflict = project_admitted_response(&session_id, &response).unwrap();
        conflict.digest = "conflict".into();
        let update = response_projection_update(conflict);
        assert!(
            reconcile_response_projections(&session_id, &timeline, vec![update.clone()]).is_err()
        );
        let line = raw_line(&update);
        assert!(
            crate::session::storage::reconcile_raw_replay_lines(
                &session_id,
                &timeline,
                vec![line.as_str()],
            )
            .is_err()
        );
    }

    #[test]
    fn raw_reconciliation_marks_only_healthy_exact_projection_unchanged() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let response = timeline.admitted_responses().pop().unwrap();
        let projection = project_admitted_response(&session_id, &response).unwrap();
        let line = raw_line(&response_projection_update(projection));
        let result = crate::session::storage::reconcile_raw_replay_lines(
            &session_id,
            &timeline,
            vec![line.as_str()],
        )
        .unwrap();
        assert!(!result.changed);
        assert!(matches!(
            result.lines[0],
            crate::session::storage::ReconciledReplayLine::Borrowed(_)
        ));
    }

    #[test]
    fn raw_candidate_peek_does_not_decode_unknown_update_body() {
        let session_id = SessionId::new("session-1");
        let timeline = validated_timeline(
            "request-1",
            2,
            vec![sampling_types::ConversationItem::assistant("answer")],
            0,
        );
        let line = format!(
            r#"{{"method":"session/update","params":{{"sessionId":"session-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"unknown","payload":{{"not":"acp"}}}}}},"_meta":{{"samplingRequestId":"request-1","samplingAttempt":2}}}}}}"#
        );
        let result = crate::session::storage::reconcile_raw_replay_lines(
            &session_id,
            &timeline,
            vec![line.as_str()],
        )
        .unwrap();
        assert!(result.changed);
        assert!(result.lines.iter().any(|line| matches!(
            line,
            crate::session::storage::ReconciledReplayLine::Owned(_)
        )));
    }
}
