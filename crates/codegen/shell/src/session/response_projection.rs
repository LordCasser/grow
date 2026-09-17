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

pub(crate) fn reconcile_response_projections(
    session_id: &SessionId,
    timeline: &chat_state::Timeline,
    updates: Vec<crate::session::storage::SessionUpdate>,
) -> std::io::Result<Vec<crate::session::storage::SessionUpdate>> {
    use crate::session::storage::SessionUpdate;
    let admitted = timeline.admitted_responses();
    let canonicals = admitted
        .iter()
        .map(|response| {
            project_admitted_response(session_id, response)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let active_by_identity = admitted
        .iter()
        .enumerate()
        .map(|(index, response)| {
            (
                (
                    response.identity.request_id.clone(),
                    response.identity.attempt,
                ),
                index,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut candidate_anchor = vec![None; canonicals.len()];
    let mut exact_anchor = vec![None; canonicals.len()];
    let mut suppressed = vec![false; updates.len()];

    // Reconcile against the original physical order. Candidate rows are
    // provisional cache only; every candidate is removed, while active rows
    // provide an insertion anchor and active projection rows are validated
    // before any dead-branch cache is suppressed.
    for (physical_index, update) in updates.iter().enumerate() {
        match update {
            SessionUpdate::Acp(notification) => {
                let Some(identity) = candidate_identity(notification) else {
                    continue;
                };
                suppressed[physical_index] = true;
                if let Some(&canonical_index) = active_by_identity.get(&identity) {
                    candidate_anchor[canonical_index].get_or_insert(physical_index);
                }
            }
            SessionUpdate::ResponseReplayProjection(existing) => {
                let identity = (existing.request_id.clone(), existing.attempt);
                let Some(&canonical_index) = active_by_identity.get(&identity) else {
                    // Projection rows not owned by the selected branch are
                    // dead-branch cache and must not replay.
                    suppressed[physical_index] = true;
                    continue;
                };
                let canonical = &canonicals[canonical_index];
                if !existing.exact_match(canonical) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "response replay projection conflict",
                    ));
                }
                exact_anchor[canonical_index].get_or_insert(physical_index);
                suppressed[physical_index] = true;
            }
            _ => {}
        }
    }

    let mut anchors = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for (canonical_index, response) in admitted.iter().enumerate() {
        let anchor = candidate_anchor[canonical_index]
            .or(exact_anchor[canonical_index])
            .unwrap_or(updates.len());
        if anchor == updates.len() && !timeline.response_projection_tail_safe(response.event_seq) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "missing response projection cannot be safely ordered at cache tail",
            ));
        }
        anchors.entry(anchor).or_default().push(canonical_index);
    }

    let mut reconciled = Vec::with_capacity(updates.len() + canonicals.len());
    for physical_index in 0..=updates.len() {
        if let Some(canonical_indices) = anchors.get(&physical_index) {
            for &canonical_index in canonical_indices {
                reconciled.push(SessionUpdate::ResponseReplayProjection(Box::new(
                    canonicals[canonical_index].clone(),
                )));
            }
        }
        if physical_index < updates.len() && !suppressed[physical_index] {
            reconciled.push(updates[physical_index].clone());
        }
    }
    Ok(reconciled)
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
}
