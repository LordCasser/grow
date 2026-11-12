use super::*;
use crate::extensions::notification::{
    SessionNotification as GrowNotification, SessionUpdate as GrowUpdate,
};
use crate::session::storage::SessionUpdate;
use chat_state::session_runtime::{
    CommandSettlementStatus, RuntimeCommandSettlement, SessionRuntimeEvent,
};
use std::borrow::Cow;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

pub(crate) const HISTORY_CURSOR_KEY: &str = "grow/historyCursor";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    session: String,
    cut: u64,
    hash: String,
    record: u64,
    part: usize,
    complete: bool,
}

pub(crate) struct PreparedHistory {
    pub updates: Vec<SessionUpdate>,
    pub mark_replay: bool,
}

fn meta_mut(update: &mut SessionUpdate) -> &mut acp::Meta {
    match update {
        SessionUpdate::Acp(notification) => notification.meta.get_or_insert_with(acp::Meta::new),
        SessionUpdate::Grow(notification) => {
            let meta = notification
                .meta
                .get_or_insert_with(|| serde_json::Value::Object(acp::Meta::new()));
            meta.as_object_mut().expect("validated Grow metadata")
        }
        #[cfg(any(test, feature = "test-support"))]
        SessionUpdate::ResponseReplayProjection(_) => unreachable!("not a presentation fact"),
    }
}

fn cursor_from_update(update: &SessionUpdate) -> Option<Cursor> {
    let meta = match update {
        SessionUpdate::Acp(value) => value.meta.as_ref(),
        SessionUpdate::Grow(value) => value.meta.as_ref().and_then(serde_json::Value::as_object),
        #[cfg(any(test, feature = "test-support"))]
        SessionUpdate::ResponseReplayProjection(_) => None,
    };
    let raw = meta?.get(HISTORY_CURSOR_KEY)?.as_str()?;
    serde_json::from_str(raw).ok()
}

pub(crate) fn background_command_settlements(
    event: &chat_state::TimelineEvent,
) -> impl Iterator<Item = &RuntimeCommandSettlement> {
    let commands = match &event.kind {
        chat_state::TimelineEventKind::Runtime(SessionRuntimeEvent::CommandsSettled {
            commands,
            ..
        }) => commands.as_slice(),
        _ => &[],
    };
    commands.iter().filter(|command| command.is_backgrounded)
}

fn time_from_ms(at_ms: i64) -> SystemTime {
    let duration = Duration::from_millis(at_ms.unsigned_abs());
    if at_ms < 0 {
        SystemTime::UNIX_EPOCH - duration
    } else {
        SystemTime::UNIX_EPOCH + duration
    }
}

impl HistorySnapshot {
    /// One pure projector for live resume, offline readers and child views.
    /// Canonical lookup borrows the selected Timeline; there is no digest or
    /// response cache to construct, decode or compare.
    pub(crate) fn prepare(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
        cursor: Option<&str>,
    ) -> io::Result<PreparedHistory> {
        let _timer = crate::instrumentation_timer!("session.load.history_projection");
        let (anchored, notice_refs) = Self::source_indexes(&self.sources);
        self.project(
            session,
            timeline,
            cursor,
            self.sources.len() as u64,
            self.sources.iter().map(Cow::Borrowed),
            anchored,
            notice_refs,
        )
    }

    /// Offline readers own their validated cut. Move independent payloads to
    /// the wire output rather than retaining a second whole body copy.
    pub(crate) fn into_prepare(
        mut self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
        cursor: Option<&str>,
    ) -> io::Result<PreparedHistory> {
        let _timer = crate::instrumentation_timer!("session.load.history_projection");
        let (anchored, notice_refs) = Self::source_indexes(&self.sources);
        let sources = std::mem::take(&mut self.sources);
        self.project(
            session,
            timeline,
            cursor,
            sources.len() as u64,
            sources.into_iter().map(Cow::Owned),
            anchored,
            notice_refs,
        )
    }

    fn source_indexes(
        sources: &[HistorySource],
    ) -> (BTreeMap<u64, u64>, std::collections::BTreeSet<u64>) {
        let anchored = sources
            .iter()
            .filter_map(|source| match source {
                HistorySource::Domain {
                    event,
                    response_anchor: Some(ResponseAnchor::Candidate { record }),
                } => Some((*event, *record)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let notice_refs = sources
            .iter()
            .filter_map(|source| match source {
                HistorySource::Presentation { fact, .. } => match &fact.content {
                    HistoryContent::Notice { event } => Some(*event),
                    _ => None,
                },
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        (anchored, notice_refs)
    }

    fn project<'a>(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
        cursor: Option<&str>,
        cut: u64,
        sources: impl IntoIterator<Item = Cow<'a, HistorySource>>,
        anchored: BTreeMap<u64, u64>,
        notice_refs: std::collections::BTreeSet<u64>,
    ) -> io::Result<PreparedHistory> {
        Self::validate_owner(timeline, session.0.as_ref())?;
        let admitted = timeline.admitted_response_views();
        let by_key = admitted
            .iter()
            .map(|response| {
                (
                    (
                        response.identity.request_id.as_str(),
                        response.identity.attempt,
                    ),
                    response,
                )
            })
            .collect::<BTreeMap<_, _>>();
        let by_event = admitted
            .iter()
            .map(|response| (response.event_seq.get(), response))
            .collect::<BTreeMap<_, _>>();
        let consumed_echoes = timeline
            .events()
            .iter()
            .filter_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Input(chat_state::InputEvent::Consumed {
                    display_echo: Some(echo),
                    ..
                }) => Some(echo.message_id.as_str()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut updates = Vec::new();
        let mut settlement_sources = Vec::new();
        for (position, row) in sources.into_iter().enumerate() {
            let source = row.as_ref();
            let at_ms = match source {
                HistorySource::Presentation { at_ms, .. } => Some(*at_ms),
                _ => None,
            };
            let position = position as u64;
            let response = match source {
                HistorySource::Presentation { fact, .. } => match &fact.content {
                    HistoryContent::Candidate(key) => by_key
                        .get(&(key.request_id.as_str(), key.attempt))
                        .copied()
                        .copied()
                        .filter(|response| {
                            anchored.get(&response.event_seq.get()) == Some(&position)
                        }),
                    _ => None,
                },
                HistorySource::Domain {
                    event,
                    response_anchor: Some(ResponseAnchor::Admission),
                } => by_event.get(event).copied().copied(),
                _ => None,
            };
            if let Some(response) = response {
                if response.quarantined_tool_exchanges != 0 {
                    continue;
                }
                let texts = response
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(ordinal, item)| match item {
                        sampling_types::ConversationItem::Assistant(value)
                            if !value.content.is_empty() =>
                        {
                            Some((TextChannel::Assistant, &value.content, ordinal))
                        }
                        sampling_types::ConversationItem::Reasoning(value)
                            if !value.text.is_empty() =>
                        {
                            Some((TextChannel::Reasoning, &value.text, ordinal))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                let parts = texts.len();
                for (part, (channel, text, ordinal)) in texts.into_iter().enumerate() {
                    let content = acp::ContentChunk::new(acp::ContentBlock::Text(
                        acp::TextContent::new(text.to_string()),
                    ));
                    let update = match channel {
                        TextChannel::Assistant => acp::SessionUpdate::AgentMessageChunk(content),
                        TextChannel::Reasoning => acp::SessionUpdate::AgentThoughtChunk(content),
                        TextChannel::User => unreachable!(),
                    };
                    let mut meta = match source {
                        HistorySource::Presentation { fact, .. } => {
                            fact.meta.clone().unwrap_or_default()
                        }
                        _ => acp::Meta::new(),
                    };
                    // The first preview event identifies an anchor, not every
                    // expanded canonical part. Its numeric highwater would drop
                    // later parts during cursor-tail replay; the verified cursor
                    // and message source identify each canonical row instead.
                    meta.remove("eventId");
                    meta.insert(
                        "grow/historyMessageEvent".into(),
                        response.event_seq.get().into(),
                    );
                    meta.insert("grow/historyMessageItem".into(), ordinal.into());
                    if let HistorySource::Domain { event, .. } = source {
                        meta.insert(
                            "grow/historyTimestampMs".into(),
                            timeline.events()[*event as usize].at_ms.into(),
                        );
                    }
                    meta.extend(serde_json::json!({"responseProjection":true,"samplingRequestId":response.identity.request_id,"samplingAttempt":response.identity.attempt}).as_object().cloned().unwrap());
                    let mut update = SessionUpdate::Acp(Box::new(
                        acp::SessionNotification::new(session.clone(), update).meta(Some(meta)),
                    ));
                    self.stamp_cursor(
                        session,
                        cut,
                        at_ms,
                        position,
                        part,
                        part + 1 == parts,
                        &mut update,
                    )?;
                    updates.push(update);
                }
                continue;
            }
            if let HistorySource::Domain { event, .. } = source {
                let domain = &timeline.events()[*event as usize];
                if background_command_settlements(domain).next().is_some() {
                    settlement_sources.push((position, domain));
                }
                if !notice_refs.contains(event)
                    && let Some(notice) = self.notices.get(event)
                {
                    let notification = crate::extensions::notification::SessionNotification {
                        session_id: session.clone(),
                        update: crate::extensions::notification::SessionUpdate::UiNotice(
                            notice.clone(),
                        ),
                        meta: Some(
                            serde_json::json!({"eventId": format!("{}-receipt-{event}", session.0), "grow/historyTimestampMs": timeline.events()[*event as usize].at_ms}),
                        ),
                    };
                    let mut update = SessionUpdate::Grow(Box::new(notification));
                    self.stamp_cursor(session, cut, at_ms, position, 0, true, &mut update)?;
                    updates.push(update);
                }
                continue;
            }
            let HistorySource::Presentation { fact, .. } = source else {
                continue;
            };
            if let HistoryContent::User(chunk) = &fact.content
                && let Some(meta) = &chunk.meta
                && meta
                    .get("grow/consumedInputEcho")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
                && !meta
                    .get("messageId")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| consumed_echoes.contains(id))
            {
                continue;
            }
            let mut update = match &fact.content {
                HistoryContent::Notice { event } => {
                    let notice = self.notices.get(event).ok_or_else(|| {
                        invalid("notice source is absent from the validated Timeline")
                    })?;
                    Some(SessionUpdate::Grow(Box::new(
                        crate::extensions::notification::SessionNotification {
                            session_id: session.clone(),
                            update: crate::extensions::notification::SessionUpdate::UiNotice(
                                notice.clone(),
                            ),
                            meta: fact.meta.clone().map(serde_json::Value::Object),
                        },
                    )))
                }
                HistoryContent::Message {
                    event,
                    item,
                    channel,
                    chunk_meta,
                } => {
                    let event = timeline
                        .events()
                        .get(*event as usize)
                        .filter(|value| value.seq.get() == *event)
                        .ok_or_else(|| invalid("history message source is absent"))?;
                    let message = event
                        .messages()
                        .filter(|message| message.response_admission.is_none())
                        .ok_or_else(|| {
                            invalid("inherited history cannot acquire an admission identity")
                        })?;
                    let ordinal = *item;
                    let item = message
                        .items
                        .get(ordinal)
                        .ok_or_else(|| invalid("history message ordinal is absent"))?;
                    let text = match (channel, item) {
                        (
                            TextChannel::Assistant,
                            sampling_types::ConversationItem::Assistant(item),
                        ) => item.content.to_string(),
                        (
                            TextChannel::Reasoning,
                            sampling_types::ConversationItem::Reasoning(item),
                        ) => item.text.to_string(),
                        (TextChannel::User, sampling_types::ConversationItem::User(_)) => {
                            item.text_content()
                        }
                        _ => {
                            return Err(invalid(
                                "history source channel differs from canonical item",
                            ));
                        }
                    };
                    let content = acp::ContentChunk::new(acp::ContentBlock::Text(
                        acp::TextContent::new(text),
                    ))
                    .meta(chunk_meta.clone());
                    let update = match channel {
                        TextChannel::User => acp::SessionUpdate::UserMessageChunk(content),
                        TextChannel::Assistant => acp::SessionUpdate::AgentMessageChunk(content),
                        TextChannel::Reasoning => acp::SessionUpdate::AgentThoughtChunk(content),
                    };
                    let mut meta = fact.meta.clone().unwrap_or_default();
                    meta.insert("grow/historyMessageEvent".into(), event.seq.get().into());
                    meta.insert("grow/historyMessageItem".into(), ordinal.into());
                    Some(SessionUpdate::Acp(Box::new(
                        acp::SessionNotification::new(session.clone(), update).meta(Some(meta)),
                    )))
                }
                _ => match row.into_owned() {
                    HistorySource::Presentation { fact, .. } => fact.into_update(session)?,
                    HistorySource::Domain { .. } => unreachable!("domain handled above"),
                },
            };
            if let Some(mut update) = update.take() {
                self.stamp_cursor(session, cut, at_ms, position, 0, true, &mut update)?;
                updates.push(update);
            }
        }
        let updates = crate::session::storage::filter_rewind_updates(updates);
        let mut updates =
            self.project_command_terminals(session, cut, updates, settlement_sources)?;
        // A proof refers to both authority cut and completed expanded row.
        // A newer cut or a half-response never authorizes skipping history.
        let cursor = cursor.and_then(|raw| {
            (raw.len() <= 2048)
                .then(|| serde_json::from_str::<Cursor>(raw).ok())
                .flatten()
        });
        let mut mark_replay = true;
        if let Some(cursor) = cursor.filter(|cursor| {
            cursor.version == PROJECTOR_VERSION
                && cursor.session == session.0.as_ref()
                && cursor.cut == cut
                && cursor.hash == self.prefix_hash
                && cursor.complete
        }) && let Some(index) = updates.iter().position(|update| {
            cursor_from_update(update).is_some_and(|row| {
                row.complete && row.record == cursor.record && row.part == cursor.part
            })
        }) {
            updates.drain(..=index);
            mark_replay = false;
        }
        Ok(PreparedHistory {
            updates,
            mark_replay,
        })
    }

    fn project_command_terminals(
        &self,
        session: &acp::SessionId,
        cut: u64,
        updates: Vec<SessionUpdate>,
        settlements: Vec<(u64, &chat_state::TimelineEvent)>,
    ) -> io::Result<Vec<SessionUpdate>> {
        if settlements.is_empty() {
            return Ok(updates);
        }
        // Match against visible starts after rewind. A rewind can hide a start
        // or its ordinary completion, but it cannot restart a settled process.
        let mut starts = HashMap::new();
        for update in &updates {
            let SessionUpdate::Grow(notification) = update else {
                continue;
            };
            match &notification.update {
                GrowUpdate::TaskBackgrounded { task_id, .. } => {
                    starts.insert(task_id.as_str(), (update, notification));
                }
                GrowUpdate::TaskCompleted { task_snapshot } => {
                    starts.remove(task_snapshot.task_id.as_str());
                }
                _ => {}
            }
        }
        let mut insertions = Vec::new();
        for (position, event) in settlements {
            let mut completions = Vec::new();
            for settlement in background_command_settlements(event) {
                let Some(&(start, notification)) = starts.get(settlement.command_id.as_str())
                else {
                    continue;
                };
                let GrowUpdate::TaskBackgrounded {
                    tool_call_id,
                    command,
                    cwd,
                    description,
                    monitor_description,
                    ..
                } = &notification.update
                else {
                    unreachable!("indexed task start")
                };
                if tool_call_id != &settlement.call_id
                    || cursor_from_update(start).is_none_or(|cursor| cursor.record >= position)
                {
                    continue;
                }
                starts.remove(settlement.command_id.as_str());
                let start_ms = notification
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get("grow/historyTimestampMs"))
                    .and_then(serde_json::Value::as_i64)
                    .expect("stamped task start");
                let interrupted = settlement.outcome == CommandSettlementStatus::Interrupted;
                let (goal_id, goal_definition_revision) = match &settlement.owner {
                    chat_state::NotificationOwner::Session
                    | chat_state::NotificationOwner::Plan { .. } => (None, None),
                    chat_state::NotificationOwner::Goal {
                        goal_id,
                        definition_revision,
                    } => (Some(goal_id.clone()), Some(*definition_revision)),
                };
                completions.push(SessionUpdate::Grow(Box::new(GrowNotification {
                    session_id: session.clone(),
                    update: GrowUpdate::TaskCompleted {
                        task_snapshot: tools::types::TaskSnapshot {
                            task_id: settlement.command_id.clone(),
                            command: command.clone(),
                            display_command: None,
                            cwd: cwd.clone(),
                            start_time: settlement
                                .start_time
                                .unwrap_or_else(|| time_from_ms(start_ms)),
                            end_time: settlement
                                .end_time
                                .or_else(|| Some(time_from_ms(event.at_ms))),
                            output: String::new(),
                            output_file: settlement.output_file.clone().into(),
                            truncated: false,
                            exit_code: settlement
                                .exit_code
                                .filter(|code| !interrupted || *code != 0),
                            signal: settlement
                                .signal
                                .clone()
                                .or_else(|| interrupted.then(|| "interrupted".into())),
                            completed: true,
                            kind: match settlement.task_kind {
                                chat_state::NotificationTaskKind::Task => {
                                    tools::computer::types::TaskKind::Bash
                                }
                                chat_state::NotificationTaskKind::Monitor => {
                                    tools::computer::types::TaskKind::Monitor
                                }
                            },
                            block_waited: false,
                            explicitly_killed: false,
                            owner_session_id: Some(session.0.to_string()),
                            goal_id,
                            goal_definition_revision,
                            description: description
                                .clone()
                                .or_else(|| monitor_description.clone()),
                            is_backgrounded: true,
                        },
                    },
                    meta: None,
                })));
            }
            if completions.is_empty() {
                continue;
            }
            let index = updates.partition_point(|update| {
                cursor_from_update(update)
                    .expect("stamped history update")
                    .record
                    < position
            });
            let count = completions.len();
            for (part, mut update) in completions.into_iter().enumerate() {
                self.stamp_cursor(
                    session,
                    cut,
                    Some(event.at_ms),
                    position,
                    part,
                    part + 1 == count,
                    &mut update,
                )?;
                insertions.push((index, update));
            }
        }
        if insertions.is_empty() {
            return Ok(updates);
        }
        let mut merged = Vec::with_capacity(updates.len() + insertions.len());
        let mut original = updates.into_iter();
        let mut consumed = 0;
        for (index, update) in insertions {
            merged.extend(original.by_ref().take(index - consumed));
            merged.push(update);
            consumed = index;
        }
        merged.extend(original);
        Ok(merged)
    }

    fn stamp_cursor(
        &self,
        session: &acp::SessionId,
        cut: u64,
        at_ms: Option<i64>,
        record: u64,
        part: usize,
        complete: bool,
        update: &mut SessionUpdate,
    ) -> io::Result<()> {
        let cursor = Cursor {
            version: PROJECTOR_VERSION,
            session: session.0.to_string(),
            cut,
            hash: self.prefix_hash.clone(),
            record,
            part,
            complete,
        };
        if let Some(at_ms) = at_ms {
            meta_mut(update).insert("grow/historyTimestampMs".into(), at_ms.into());
        }
        meta_mut(update).insert(
            HISTORY_CURSOR_KEY.into(),
            serde_json::to_string(&cursor)
                .map_err(|error| invalid(error.to_string()))?
                .into(),
        );
        Ok(())
    }
}
