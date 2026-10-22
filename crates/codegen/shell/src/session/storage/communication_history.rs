//! Merge Timeline-only communication at causal history anchors. Physical rows
//! keep their order; timestamps are presentation data, never sorting authority.

use super::*;
use crate::extensions::notification::{AgentMessageNotice, SessionUpdate as GrowUpdate, UiNotice};
use chat_state::{InputEvent, TimelineEvent, TimelineEventKind, ToolEvent};
use std::collections::{HashMap, HashSet};

pub(crate) fn notice_identity(notice: &UiNotice) -> (String, Option<String>, Option<String>) {
    let source = crate::coordination::IncomingInquiryAudit::from_notice(notice)
        .map(|audit| audit.source_session_id);
    (
        notice.correlation_id.clone(),
        notice.subject.clone(),
        source,
    )
}

/// Compact binding retained by the existing writer receipt index. A display
/// reference may omit body bytes only after they match the domain source.
#[derive(Debug, Clone)]
pub(crate) struct ReceiptSource {
    pub event: u64,
    notice_digest: blake3::Hash,
    payload: Option<chat_state::NotificationPayloadRef>,
}

impl ReceiptSource {
    pub(crate) fn from_event(event: &TimelineEvent) -> io::Result<Self> {
        let (notice, payload) = match &event.kind {
            TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
                source,
                payload_ref,
                ..
            }) if source.agent_message().is_some() => (
                crate::session::notification_inbox::parent_message_notice(event, None)
                    .expect("agent message source has a notice"),
                Some(payload_ref.clone()),
            ),
            _ => (
                crate::coordination::InquiryEvent::from_timeline(event)
                    .map_err(io::Error::other)?
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "notice has no domain source")
                    })?
                    .notice(),
                None,
            ),
        };
        Ok(Self {
            event: event.seq.get(),
            notice_digest: Self::digest(&notice)?,
            payload,
        })
    }

    fn digest(notice: &UiNotice) -> io::Result<blake3::Hash> {
        Ok(blake3::hash(
            &serde_json::to_vec(notice).map_err(io::Error::other)?,
        ))
    }

    pub(crate) fn validate_notice(&self, notice: &UiNotice) -> io::Result<()> {
        use crate::extensions::notification::UiNoticeTone;
        let mut normalized = notice.clone();
        if let Some(payload) = &self.payload {
            let mut details = AgentMessageNotice::from_notice(notice).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "notice source details are invalid",
                )
            })?;
            let body = details.message.take().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "notice body is absent before publication",
                )
            })?;
            if body.len() as u64 != payload.bytes
                || blake3::hash(body.as_bytes()).to_hex().as_str() != payload.blake3
                || notice.message != "Received"
                || notice.tone != UiNoticeTone::Info
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "notice differs from its canonical receipt",
                ));
            }
            normalized.message = "Message unavailable".into();
            normalized.tone = UiNoticeTone::Warning;
            normalized.details = Some(serde_json::to_string(&details).map_err(io::Error::other)?);
        }
        if Self::digest(&normalized)? != self.notice_digest {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "notice differs from its canonical receipt",
            ));
        }
        Ok(())
    }
}

type ReceiptIdentity = (String, Option<String>, Option<String>);

pub(crate) fn receipt_identity(event: &TimelineEvent) -> io::Result<Option<ReceiptIdentity>> {
    match &event.kind {
        TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
            id,
            source,
            ..
        }) if source.agent_message().is_some() => Ok(Some((
            id.clone(),
            Some(AgentMessageNotice::SUBJECT.into()),
            None,
        ))),
        TimelineEventKind::Observation(observation)
            if observation.scope == "coordination" && observation.name == "inquiry" =>
        {
            Ok(crate::coordination::InquiryEvent::from_timeline(event)
                .map_err(io::Error::other)?
                .filter(|fact| matches!(fact, crate::coordination::InquiryEvent::Incoming { .. }))
                .map(|fact| notice_identity(&fact.notice())))
        }
        _ => Ok(None),
    }
}

pub(crate) fn read_notice(
    directory: &ContainedDirectory,
    event: &TimelineEvent,
) -> io::Result<Option<UiNotice>> {
    if let Some(notice) =
        crate::session::notification_inbox::read_parent_message_notice(directory, event)
    {
        return Ok(Some(notice));
    }
    Ok(crate::coordination::InquiryEvent::from_timeline(event)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
        .filter(|fact| matches!(fact, crate::coordination::InquiryEvent::Incoming { .. }))
        .map(|fact| fact.notice()))
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn restore<'a>(
    session_id: &acp::SessionId,
    timeline: &chat_state::Timeline,
    mut history: RawReconciliation<'a>,
    mut read: impl FnMut(&TimelineEvent) -> io::Result<Option<UiNotice>>,
) -> io::Result<RawReconciliation<'a>> {
    let mut receipts = Vec::new();
    let mut receipt_sequences = HashMap::new();
    for event in timeline.events() {
        let identity = match &event.kind {
            TimelineEventKind::Notification(chat_state::NotificationEvent::Received {
                id,
                owner_session_id,
                source,
                ..
            }) if source.agent_message().is_some() => {
                if owner_session_id != session_id.0.as_ref() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "foreign communication receipt in session history",
                    ));
                }
                (id.clone(), Some(AgentMessageNotice::SUBJECT.into()), None)
            }
            TimelineEventKind::Observation(observation)
                if observation.scope == "coordination" && observation.name == "inquiry" =>
            {
                let Some(fact @ crate::coordination::InquiryEvent::Incoming { .. }) =
                    crate::coordination::InquiryEvent::from_timeline(event)
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                else {
                    continue;
                };
                notice_identity(&fact.notice())
            }
            _ => continue,
        };
        receipt_sequences
            .entry(identity.clone())
            .or_insert(event.seq.get());
        receipts.push((event, identity));
    }
    if receipts.is_empty() {
        return Ok(history);
    }
    let mut inputs = HashMap::new();
    let mut starts = HashMap::new();
    let mut ends = HashMap::new();
    for event in timeline.events() {
        match &event.kind {
            TimelineEventKind::Input(InputEvent::Submitted { input_id, .. }) => {
                inputs.insert(
                    input_id.strip_prefix("input-").unwrap_or(input_id),
                    event.seq.get(),
                );
            }
            TimelineEventKind::Tool(ToolEvent::Started { call_id, .. }) => {
                starts.insert(call_id.as_str(), event.seq.get());
            }
            TimelineEventKind::Tool(ToolEvent::Completed { call_id, .. }) => {
                ends.insert(call_id.as_str(), event.seq.get());
            }
            _ => {}
        }
    }
    let mut seen = HashSet::new();
    let mut anchors = Vec::with_capacity(history.lines.len());
    history.lines.retain(|line| {
        let Ok(env) = serde_json::from_str::<RawLinePeek<'_>>(line.as_str()) else {
            anchors.push(None);
            return true;
        };
        let mut notice_seq = None;
        if env.method == GROW_SESSION_UPDATE_METHOD {
            // Decode only notices, never large tool output bodies.
            if let Ok(params) = serde_json::from_str::<RawParamsPeek<'_>>(env.params.get())
                && params
                    .update
                    .is_some_and(|u| u.session_update == "ui_notice")
                && let Ok(n) = serde_json::from_str::<SessionNotification>(env.params.get())
                && let GrowUpdate::UiNotice(notice) = n.update
            {
                let parent = notice.subject.as_deref() == Some(AgentMessageNotice::SUBJECT);
                let identity = notice_identity(&notice);
                notice_seq = receipt_sequences.get(&identity).copied();
                let new = seen.insert(identity);
                if parent && !new {
                    history.changed = true;
                    return false;
                }
            }
        }
        let seq = if notice_seq.is_some() {
            notice_seq
        } else if env.method
            == crate::session::response_projection::RESPONSE_REPLAY_PROJECTION_METHOD
        {
            #[derive(serde::Deserialize)]
            struct Projection {
                timeline_event: u64,
            }
            serde_json::from_str::<Projection>(env.params.get())
                .ok()
                .map(|p| p.timeline_event)
        } else if env.method == ACP_SESSION_UPDATE_METHOD {
            #[derive(serde::Deserialize)]
            struct Params<'a> {
                #[serde(borrow)]
                update: Update<'a>,
                #[serde(default, borrow, rename = "_meta")]
                meta: Option<Meta<'a>>,
            }
            #[derive(serde::Deserialize)]
            struct Update<'a> {
                #[serde(rename = "sessionUpdate")]
                kind: &'a str,
                #[serde(default, borrow, rename = "toolCallId")]
                call: Option<&'a str>,
                #[serde(default)]
                status: Option<&'a str>,
                #[serde(default, borrow, rename = "_meta")]
                meta: Option<ChunkMeta<'a>>,
            }
            #[derive(serde::Deserialize)]
            struct Meta<'a> {
                #[serde(default, borrow, rename = "promptId")]
                prompt: Option<&'a str>,
            }
            #[derive(serde::Deserialize)]
            struct ChunkMeta<'a> {
                #[serde(default, borrow, rename = "messageId")]
                message: Option<&'a str>,
            }
            serde_json::from_str::<Params<'_>>(env.params.get())
                .ok()
                .and_then(|p| match p.update.kind {
                    "tool_call" | "tool_call_update" => p.update.call.and_then(|id| {
                        if matches!(p.update.status, Some("completed" | "failed")) {
                            ends.get(id).or_else(|| starts.get(id)).copied()
                        } else {
                            starts.get(id).copied()
                        }
                    }),
                    "user_message_chunk" => p
                        .meta
                        .and_then(|m| m.prompt)
                        .or_else(|| p.update.meta.and_then(|m| m.message))
                        .and_then(|id| inputs.get(id).copied()),
                    _ => None,
                })
        } else {
            None
        };
        anchors.push(seq);
        true
    });
    let mut insertions = std::collections::BTreeMap::<usize, Vec<String>>::new();
    let anchored = anchors.iter().any(Option::is_some);
    let mut insertion_cursor = 0;
    for (receipt, identity) in receipts {
        if !seen.insert(identity) {
            continue;
        }
        let Some(notice) = read(receipt)? else {
            continue;
        };
        // Timeline receipt seq increases, so the first greater physical
        // anchor cannot move left, even when physical anchors are unsorted.
        // Scan each row once instead of rescanning a large history per receipt.
        while insertion_cursor < anchors.len()
            && anchors[insertion_cursor].is_none_or(|seq| seq <= receipt.seq.get())
        {
            insertion_cursor += 1;
        }
        let position = insertion_cursor;
        let mut meta = serde_json::Map::new();
        meta.insert("transient".into(), true.into());
        meta.insert("timelineEvent".into(), receipt.seq.get().into());
        if let Ok(at) = u64::try_from(receipt.at_ms) {
            meta.insert("agentTimestampMs".into(), at.into());
        }
        if !anchored {
            meta.insert("historyOrderEstimated".into(), true.into());
        }
        let n = SessionNotification {
            session_id: session_id.clone(),
            update: GrowUpdate::UiNotice(notice),
            meta: Some(serde_json::Value::Object(meta)),
        };
        let envelope = SessionUpdateEnvelope {
            timestamp: u64::try_from(receipt.at_ms).unwrap_or(0) / 1000,
            method: GROW_SESSION_UPDATE_METHOD.into(),
            params: serde_json::to_value(n)?,
        };
        insertions
            .entry(position)
            .or_default()
            .push(serde_json::to_string(&envelope)?);
        history.changed = true;
    }
    if insertions.is_empty() {
        return Ok(history);
    }
    let mut result =
        Vec::with_capacity(history.lines.len() + insertions.values().map(Vec::len).sum::<usize>());
    for (index, line) in history.lines.into_iter().enumerate() {
        if let Some(rows) = insertions.remove(&index) {
            result.extend(
                rows.into_iter()
                    .map(|row| ReconciledReplayLine::Synthesized(row.into())),
            );
        }
        result.push(line);
    }
    for rows in insertions.into_values() {
        result.extend(
            rows.into_iter()
                .map(|row| ReconciledReplayLine::Synthesized(row.into())),
        );
    }
    history.lines = result;
    Ok(history)
}

#[cfg(test)]
mod tests;
