use super::*;
use crate::extensions::notification::{
    SessionNotification as GrowNotification, SessionUpdate as GrowUpdate,
};
use crate::session::storage::SessionUpdate;
use chat_state::session_runtime::{
    CommandSettlementStatus, RuntimeCommandSettlement, SessionRuntimeEvent,
};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

#[path = "timeline_suffix.rs"]
mod timeline_suffix;
#[path = "window.rs"]
mod window;
pub(crate) use window::PreparedPages;

pub(crate) const HISTORY_CURSOR_KEY: &str = "grow/historyCursor";

#[cfg(test)]
thread_local! {
    static BODY_PROJECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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

pub(crate) const HISTORY_PAGE_ROWS: usize = 128;
pub(crate) const HISTORY_PAGE_BYTES: usize = 4 * 1024 * 1024;
const RESPONSE_REQUEST_KEY: &str = "samplingRequestId";
const RESPONSE_ITEM_KEY: &str = "grow/historyMessageItem";
const RESPONSE_EVENT_KEY: &str = "grow/historyMessageEvent";

impl PreparedHistory {
    /// Complete physical response records and tool call/update groups stay
    /// together even if one group crosses the preferred page size.
    pub(crate) fn take_page(&mut self, end: usize) -> io::Result<(usize, Vec<SessionUpdate>)> {
        self.take_page_with_floor(end, end.saturating_sub(HISTORY_PAGE_ROWS))
    }

    fn take_page_with_floor(
        &mut self,
        end: usize,
        mut start: usize,
    ) -> io::Result<(usize, Vec<SessionUpdate>)> {
        if end > self.updates.len() {
            return Err(invalid("history page is outside the loaded cut"));
        }
        if start == 0 {
            return Ok((0, self.updates.drain(..end).collect()));
        }

        start = page_start(&page_groups(self.updates[..end].iter().enumerate()), start);
        Ok((start, self.updates.drain(start..end).collect()))
    }
}

fn page_groups<'a>(
    updates: impl IntoIterator<Item = (usize, &'a SessionUpdate)>,
) -> Vec<(usize, usize)> {
    let mut tool_spans = HashMap::new();
    let mut response_spans = HashMap::new();
    let mut previous_user = None;
    let mut spans = Vec::new();
    for (position, update) in updates {
        if let Some(cursor) = cursor_from_update(update) {
            response_spans
                .entry(cursor.record)
                .and_modify(|span: &mut (usize, usize)| span.1 = position)
                .or_insert((position, position));
        }
        let tool = if let SessionUpdate::Acp(notification) = update {
            match &notification.update {
                acp::SessionUpdate::ToolCall(call) => Some(&call.tool_call_id),
                acp::SessionUpdate::ToolCallUpdate(update) => Some(&update.tool_call_id),
                _ => None,
            }
        } else {
            None
        };
        if let Some(id) = tool {
            tool_spans
                .entry(id)
                .and_modify(|span: &mut (usize, usize)| span.1 = position)
                .or_insert((position, position));
        }
        if matches!(update, SessionUpdate::Acp(n)
            if matches!(&n.update, acp::SessionUpdate::UserMessageChunk(_)))
        {
            if let Some(previous) = previous_user
                && previous + 1 == position
            {
                spans.push((previous, position));
            }
            previous_user = Some(position);
        } else {
            previous_user = None;
        }
    }
    spans.extend(tool_spans.into_values());
    spans.extend(response_spans.into_values());
    spans.retain(|(first, last)| first < last);
    spans.sort_unstable_by_key(|span| span.0);
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (first, last) in spans {
        if let Some(group) = groups.last_mut().filter(|group| first <= group.1) {
            group.1 = group.1.max(last);
        } else {
            groups.push((first, last));
        }
    }
    groups
}

fn page_start(groups: &[(usize, usize)], floor: usize) -> usize {
    let next = groups.partition_point(|(first, _)| *first < floor);
    groups[..next]
        .last()
        .filter(|(_, last)| floor <= *last)
        .map_or(floor, |(first, _)| *first)
}

#[derive(Debug, Clone)]
pub(crate) struct IndexedPages {
    index: crate::session::storage::jsonl::IndexedHistory,
    history: HistorySnapshot,
    rows: Vec<(PagePosition, SessionUpdate)>,
    task_completions_in_tail: HashSet<String>,
    /// Prompt metadata the published catalog does not yet rank. Empty when
    /// the retained cut adds no visible prompt.
    timeline: timeline_suffix::TimelineOverlay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PagePosition {
    pub(crate) record: u64,
    pub(crate) part: u64,
    pub(crate) offset: Option<usize>,
}

impl PagePosition {
    pub(crate) fn newest() -> Self {
        Self {
            record: u64::MAX,
            part: u64::MAX,
            offset: None,
        }
    }

    pub(crate) fn oldest() -> Self {
        Self {
            record: 0,
            part: 0,
            offset: Some(0),
        }
    }

    fn key(self) -> (u64, u64) {
        (self.record, self.part)
    }
}

enum PageRow {
    Retained(usize),
    Display(usize),
    Response(crate::session::storage::jsonl::IndexedResponseRow),
    UserEcho(crate::session::storage::jsonl::IndexedUserEchoRow),
    TaskTerminal(crate::session::storage::jsonl::IndexedTaskTerminal),
}

impl IndexedPages {
    pub(crate) fn is_empty(&self) -> io::Result<bool> {
        if !self.rows.is_empty() || self.index.omitted_count() != 0 {
            return Ok(false);
        }
        let reader = self.index.page_reader()?;
        Ok(reader
            .response_rows_before((u64::MAX, u64::MAX), 1, self.history.response_branch)?
            .is_empty()
            && reader
                .user_echo_rows_before((u64::MAX, u64::MAX), 1)?
                .is_empty()
            && reader
                .task_terminals_before((u64::MAX, u64::MAX), 1, self.history.record_count())?
                .is_empty())
    }

    pub(crate) fn page_before(
        &self,
        session: &acp::SessionId,
        mut before: PagePosition,
    ) -> io::Result<(Option<PagePosition>, Vec<SessionUpdate>)> {
        loop {
            let anchor = if before.part == 0 && before.offset.is_none() {
                let Some(previous) = before.record.checked_sub(1) else {
                    return Ok((None, Vec::new()));
                };
                previous
            } else {
                before.record
            };
            let window = self.index.visible_range(anchor)?;
            if window.is_empty() {
                return Ok((None, Vec::new()));
            }
            if *window.end() != anchor {
                before = PagePosition {
                    record: *window.end() + 1,
                    part: 0,
                    offset: None,
                };
            }
            let (older, updates) = self.page_in_window(session, before, *window.start())?;
            match older {
                Some(next) if updates.is_empty() => before = next,
                _ => return Ok((older, updates)),
            }
        }
    }

    fn page_in_window(
        &self,
        session: &acp::SessionId,
        before: PagePosition,
        lower: u64,
    ) -> io::Result<(Option<PagePosition>, Vec<SessionUpdate>)> {
        let mut reader = self.index.page_reader()?;
        let limit = HISTORY_PAGE_ROWS + 1;
        let inclusive = before.offset.is_some();
        let bound = if inclusive {
            (
                before.record,
                before
                    .part
                    .checked_add(1)
                    .ok_or_else(|| invalid("invalid history byte cursor"))?,
            )
        } else {
            before.key()
        };
        let mut candidates = Vec::with_capacity(5 * limit);
        for row in reader.response_rows_before(bound, limit, self.history.response_branch)? {
            candidates.push((row.position(), PageRow::Response(row)));
        }
        for row in reader.user_echo_rows_before(bound, limit)? {
            candidates.push((row.position(), PageRow::UserEcho(row)));
        }
        let terminals = reader.task_terminals_before(bound, limit, self.history.record_count())?;
        let terminal_floor = terminals
            .last()
            .filter(|_| terminals.len() == limit)
            .map(|row| row.position());
        for row in terminals {
            candidates.push((row.position(), PageRow::TaskTerminal(row)));
        }
        let end = self
            .rows
            .partition_point(|(position, _)| position.key() < bound);
        for index in end.saturating_sub(limit)..end {
            candidates.push((self.rows[index].0.key(), PageRow::Retained(index)));
        }
        let display_end = self.index.omitted_before(before.record)?
            + if bound.1 != 0 && before.record != u64::MAX {
                self.index.omitted_before(before.record + 1)?
                    - self.index.omitted_before(before.record)?
            } else {
                0
            };
        let first_display = self.index.omitted_before(lower)?;
        let display_start = display_end.saturating_sub(limit).max(first_display);
        let mut display_floor = None;
        for rank in display_start..display_end {
            let position = self.index.display_position(rank)?;
            if rank == display_start && display_start != first_display {
                display_floor = Some((position, 0));
            }
            candidates.push(((position, 0), PageRow::Display(rank)));
        }
        candidates.retain(|(position, _)| position.0 >= lower);
        candidates.sort_unstable_by(|(left, _), (right, _)| right.cmp(left));
        let mut updates = Vec::new();
        let mut budget = HISTORY_PAGE_BYTES - 2;
        let mut cursor = None;
        for ((record, part), row) in candidates {
            if updates.len() >= HISTORY_PAGE_ROWS {
                break;
            }
            if terminal_floor
                .max(display_floor)
                .is_some_and(|floor| (record, part) < floor)
            {
                break;
            }
            if let PageRow::TaskTerminal(row) = &row {
                if row.superseded()
                    || self.task_completions_in_tail.contains(row.task_id())
                    || !self
                        .index
                        .visible_range(row.start())?
                        .contains(&row.start())
                {
                    cursor = Some(PagePosition {
                        record,
                        part,
                        offset: None,
                    });
                    continue;
                }
            }
            let offset = ((record, part) == before.key())
                .then_some(before.offset)
                .flatten();
            let (older_offset, update) = match row {
                PageRow::Response(row) => {
                    let bytes = offset.map_or(row.encoded_bytes(), |offset| offset as u64);
                    if !updates.is_empty() && bytes.saturating_add(1024) > budget as u64 {
                        break;
                    }
                    reader.read_response_row(
                        row,
                        offset,
                        session,
                        &self.history,
                        (budget.saturating_sub(1024)) as u64,
                    )?
                }
                PageRow::Retained(index) => {
                    let mut update = self.rows[index].1.clone();
                    let total = text_body(&mut update).map(|body| body.len());
                    let end = offset.or(total);
                    if let Some(offset) = offset {
                        let text = text_body(&mut update)
                            .filter(|text| {
                                offset > 0 && offset <= text.len() && text.is_char_boundary(offset)
                            })
                            .ok_or_else(|| {
                                invalid("history continuation crosses a response character")
                            })?;
                        text.truncate(offset);
                    }
                    let older =
                        if updates.is_empty() && serde_json::to_vec(&update)?.len() + 1 > budget {
                            Some(take_response_suffix(
                                &mut update,
                                budget.saturating_sub(512),
                            )?)
                        } else {
                            None
                        };
                    if older.is_some() || offset.is_some() {
                        let meta = meta_mut(&mut update);
                        meta.insert(
                            "grow/historyResponseStartByte".into(),
                            older.unwrap_or(0).into(),
                        );
                        meta.insert(
                            "grow/historyResponseEndByte".into(),
                            end.expect("text fragment end").into(),
                        );
                        meta.insert(
                            "grow/historyResponseTotalBytes".into(),
                            total.expect("text fragment length").into(),
                        );
                    }
                    (older, update)
                }
                PageRow::UserEcho(row) => (
                    None,
                    reader.read_user_echo_row(row, session, &self.history)?,
                ),
                PageRow::TaskTerminal(row) => (
                    None,
                    reader.read_task_terminal(row, session, &self.history)?,
                ),
                PageRow::Display(rank) => {
                    let Some((older, update)) =
                        reader.read_display_page(rank, session, &self.history, offset)?
                    else {
                        cursor = Some(PagePosition {
                            record,
                            part,
                            offset: None,
                        });
                        continue;
                    };
                    (older, update)
                }
            };
            let bytes = serde_json::to_vec(&update)?.len() + 1;
            if bytes > budget {
                if updates.is_empty() {
                    return Err(invalid("history row exceeds the page byte budget"));
                }
                break;
            }
            budget -= bytes;
            updates.push(update);
            cursor = Some(PagePosition {
                record,
                part,
                offset: older_offset,
            });
            if older_offset.is_some() {
                break;
            }
        }
        updates.reverse();
        // The last row is an exclusive boundary when fully delivered. A
        // continuation remains inclusive until its oldest fragment arrives.
        if let Some(position) = cursor
            && position.offset.is_none()
            && !self
                .rows
                .iter()
                .any(|(row, _)| row.record >= lower && row.key() < position.key())
            && self.index.omitted_before(position.record)? == first_display
            && reader
                .response_rows_before(position.key(), 1, self.history.response_branch)?
                .first()
                .is_none_or(|row| row.position().0 < lower)
            && reader
                .user_echo_rows_before(position.key(), 1)?
                .first()
                .is_none_or(|row| row.position().0 < lower)
            && reader
                .task_terminals_before(position.key(), 1, self.history.record_count())?
                .first()
                .is_none_or(|row| row.position().0 < lower)
        {
            cursor = None;
        }
        if cursor.is_none() && lower != 0 {
            cursor = Some(PagePosition {
                record: lower,
                part: 0,
                offset: None,
            });
        }
        Ok((cursor, updates))
    }
}

fn text_body(update: &mut SessionUpdate) -> Option<&mut String> {
    let SessionUpdate::Acp(notification) = update else {
        return None;
    };
    let chunk = match &mut notification.update {
        acp::SessionUpdate::UserMessageChunk(chunk)
        | acp::SessionUpdate::AgentMessageChunk(chunk)
        | acp::SessionUpdate::AgentThoughtChunk(chunk) => chunk,
        _ => return None,
    };
    match &mut chunk.content {
        acp::ContentBlock::Text(text) => Some(&mut text.text),
        _ => None,
    }
}

fn take_response_suffix(update: &mut SessionUpdate, budget: usize) -> io::Result<usize> {
    let body =
        text_body(update).ok_or_else(|| invalid("history row exceeds the page byte budget"))?;
    let source = std::mem::take(body);
    let mut available = budget.saturating_sub(serde_json::to_vec(update)?.len() + 64);
    let mut split = source.len();
    for (position, ch) in source.char_indices().rev() {
        let bytes = match ch {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            '\u{0000}'..='\u{001f}' => 6,
            _ => ch.len_utf8(),
        };
        if bytes > available {
            break;
        }
        available -= bytes;
        split = position;
    }
    if split == 0 || split == source.len() {
        return Err(invalid(
            "history response cannot fit in the page byte budget",
        ));
    }
    *text_body(update).expect("response update") = source[split..].to_owned();
    Ok(split)
}

fn response_coordinates(update: &SessionUpdate) -> Option<(u64, usize)> {
    let SessionUpdate::Acp(notification) = update else {
        return None;
    };
    let meta = notification.meta.as_ref()?;
    (meta.get("responseProjection")?.as_bool()? && meta.contains_key(RESPONSE_REQUEST_KEY))
        .then_some(())?;
    let seq = meta.get(RESPONSE_EVENT_KEY)?.as_u64()?;
    let ordinal = usize::try_from(meta.get(RESPONSE_ITEM_KEY)?.as_u64()?).ok()?;
    Some((seq, ordinal))
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

pub(crate) fn command_settlement_update(
    session: &acp::SessionId,
    start: &GrowUpdate,
    start_ms: i64,
    settlement: &RuntimeCommandSettlement,
    end_ms: i64,
) -> SessionUpdate {
    let GrowUpdate::TaskBackgrounded {
        command,
        cwd,
        description,
        monitor_description,
        ..
    } = start
    else {
        unreachable!("indexed task start");
    };
    let interrupted = settlement.outcome == CommandSettlementStatus::Interrupted;
    let (goal_id, goal_definition_revision) = match &settlement.owner {
        chat_state::NotificationOwner::Session | chat_state::NotificationOwner::Plan { .. } => {
            (None, None)
        }
        chat_state::NotificationOwner::Goal {
            goal_id,
            definition_revision,
        } => (Some(goal_id.clone()), Some(*definition_revision)),
    };
    SessionUpdate::Grow(Box::new(GrowNotification {
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
                end_time: settlement.end_time.or_else(|| Some(time_from_ms(end_ms))),
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
                description: description.clone().or_else(|| monitor_description.clone()),
                is_backgrounded: true,
            },
        },
        meta: None,
    }))
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
    #[cfg(test)]
    pub(crate) fn body_projection_count_for_test() -> usize {
        BODY_PROJECTIONS.get()
    }

    /// Restore the task surface from the selected cut without projecting response bodies.
    pub(crate) fn background_task_updates(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
    ) -> io::Result<Vec<SessionUpdate>> {
        Self::validate_owner(timeline, session.0.as_ref())?;
        let cut = self.record_count();
        let mut facts = BTreeMap::new();
        if let Some(index) = &self.indexed {
            for record in index.background_task_history()? {
                let JournalFact::Presentation { at_ms, event } = record.fact else {
                    return Err(invalid("background task index names a domain fact"));
                };
                facts.insert(record.seq, (at_ms, event));
            }
        }
        let mut settlements = Vec::new();
        let mut pending_rewind = None;
        let mut recovered_markers = Vec::new();
        for (offset, source) in self.sources.iter().enumerate() {
            let position = self.positions.get(offset).copied().unwrap_or(offset as u64);
            match source {
                HistorySource::Domain { event, .. } => {
                    if let Some(target) =
                        timeline.rewind_display_target(chat_state::EventSeq::new(*event))
                    {
                        recovered_markers.extend(pending_rewind.replace((position, target)));
                    }
                    let domain = timeline
                        .event(chat_state::EventSeq::new(*event))
                        .ok_or_else(|| invalid("task history domain source is absent"))?;
                    if background_command_settlements(domain).next().is_some() {
                        settlements.push((position, domain));
                    }
                }
                HistorySource::Presentation { at_ms, fact } => {
                    let retain = match &fact.content {
                        HistoryContent::User(_) => true,
                        HistoryContent::Grow(value) => match value.as_ref() {
                            GrowHistoryFact::TaskBackgrounded { .. }
                            | GrowHistoryFact::TaskCompleted { .. } => true,
                            GrowHistoryFact::RewindMarker { .. } => {
                                pending_rewind = None;
                                true
                            }
                            _ => false,
                        },
                        _ => false,
                    };
                    if retain {
                        if let Some(index) = &self.indexed
                            && position < index.records()
                            && !index.visible_range(position)?.contains(&position)
                        {
                            continue;
                        }
                        facts
                            .entry(position)
                            .or_insert_with(|| (*at_ms, fact.as_ref().clone()));
                    }
                }
            }
        }
        let mut updates = Vec::with_capacity(facts.len());
        for (position, (at_ms, fact)) in facts {
            if let Some(mut update) = fact.into_update(session)? {
                self.stamp_cursor(session, cut, Some(at_ms), position, 0, true, &mut update)?;
                updates.push(update);
            }
        }
        recovered_markers.extend(pending_rewind);
        for (position, target_prompt_index) in recovered_markers {
            let mut marker = SessionUpdate::Grow(Box::new(GrowNotification {
                session_id: session.clone(),
                update: GrowUpdate::RewindMarker {
                    target_prompt_index,
                    created_at: String::new(),
                },
                meta: None,
            }));
            self.stamp_cursor(session, cut, None, position, 0, true, &mut marker)?;
            updates.push(marker);
        }
        updates.sort_by_key(|update| {
            let cursor = cursor_from_update(update).expect("stamped task history");
            (cursor.record, cursor.part)
        });
        // Indexed hydration falls back to dense history for an unpublished
        // domain rewind or presentation marker. Its prefix visibility is thus
        // authoritative; dense history retains the prompt boundaries below.
        let mut updates = crate::session::storage::filter_rewind_updates(updates);
        if let Some(index) = &self.indexed {
            let completed: HashSet<_> = updates
                .iter()
                .filter_map(|update| match update {
                    SessionUpdate::Grow(notification) => match &notification.update {
                        GrowUpdate::TaskCompleted { task_snapshot } => {
                            Some(task_snapshot.task_id.clone())
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect();
            let mut reader = index.page_reader()?;
            let mut before = (index.records(), 0);
            loop {
                let rows = reader.task_terminals_before(before, 256, cut)?;
                if rows.is_empty() {
                    break;
                }
                before = rows.last().expect("nonempty terminal batch").position();
                for row in rows {
                    if row.superseded()
                        || completed.contains(row.task_id())
                        || !index.visible_range(row.start())?.contains(&row.start())
                        || !index
                            .visible_range(row.position().0)?
                            .contains(&row.position().0)
                    {
                        continue;
                    }
                    updates.push(reader.read_task_terminal(row, session, self)?);
                }
            }
            updates.sort_by_key(|update| {
                let cursor = cursor_from_update(update).expect("stamped task history");
                (cursor.record, cursor.part)
            });
        }
        settlements.sort_by_key(|(position, _)| *position);
        let mut updates = self.project_command_terminals(session, cut, updates, settlements)?;
        updates.retain(|update| matches!(update, SessionUpdate::Grow(notification)
            if matches!(&notification.update, GrowUpdate::TaskBackgrounded { .. } | GrowUpdate::TaskCompleted { .. })));
        Ok(updates)
    }

    pub(crate) fn indexed_pages(
        &self,
        prepared: PreparedHistory,
    ) -> io::Result<Option<IndexedPages>> {
        let Some(index) = self.indexed.clone() else {
            return Ok(None);
        };
        let mut rows = Vec::with_capacity(prepared.updates.len());
        let mut pending = HashMap::<acp::ToolCallId, acp::ToolCall>::new();
        let mut reader = index.page_reader()?;
        let task_completions_in_tail = self
            .sources
            .iter()
            .zip(self.positions.iter())
            .filter_map(|(source, position)| match source {
                HistorySource::Presentation { fact, .. } if *position >= index.records() => {
                    match &fact.content {
                        HistoryContent::Grow(value) => match value.as_ref() {
                            super::GrowHistoryFact::TaskCompleted { task_snapshot } => {
                                Some(task_snapshot.task_id.clone())
                            }
                            _ => None,
                        },
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect();
        for mut update in prepared.updates {
            let cursor = cursor_from_update(&update).expect("projected row has a cursor");
            if let SessionUpdate::Acp(notification) = &mut update {
                match &mut notification.update {
                    acp::SessionUpdate::ToolCall(call)
                        if !matches!(
                            call.status,
                            acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed
                        ) =>
                    {
                        pending.insert(call.tool_call_id.clone(), call.clone());
                        continue;
                    }
                    acp::SessionUpdate::ToolCallUpdate(change) => {
                        let id = &change.tool_call_id;
                        let terminal = matches!(
                            change.fields.status,
                            Some(acp::ToolCallStatus::Completed | acp::ToolCallStatus::Failed)
                        );
                        let prior = match pending.remove(id) {
                            Some(call) => Some(call),
                            None => reader.tool_before(id, cursor.record)?,
                        };
                        if let Some(mut call) = prior {
                            call.update(std::mem::take(&mut change.fields));
                            if terminal {
                                notification.update = acp::SessionUpdate::ToolCall(call);
                            } else {
                                pending.insert(id.clone(), call);
                            }
                        }
                        if !terminal {
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            rows.push((
                PagePosition {
                    record: cursor.record,
                    part: cursor.part as u64,
                    offset: None,
                },
                update,
            ));
        }
        drop(reader);
        rows.sort_unstable_by_key(|(position, _)| position.key());
        let timeline = timeline_suffix::capture(&index, &rows)?;
        Ok(Some(IndexedPages {
            index,
            history: HistorySnapshot {
                bytes: self.bytes,
                prefix_hash: self.prefix_hash.clone(),
                indexed: self.indexed.clone(),
                positions: self.positions.last().copied().into_iter().collect(),
                notice_anchors_in_tail: self.notice_anchors_in_tail.clone(),
                response_branch: self.response_branch,
                ..Default::default()
            },
            rows,
            task_completions_in_tail,
            timeline,
        }))
    }
    /// One pure projector for live resume, offline readers and child views.
    /// Canonical lookup borrows the selected Timeline; there is no digest or
    /// response cache to construct, decode or compare.
    pub(crate) fn prepare(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
        cursor: Option<&str>,
    ) -> io::Result<PreparedHistory> {
        self.prepare_for_projection(session, timeline, cursor, false)
    }

    pub(crate) fn prepare_indexed(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
    ) -> io::Result<PreparedHistory> {
        self.prepare_for_projection(session, timeline, None, true)
    }

    fn prepare_for_projection(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
        cursor: Option<&str>,
        indexed_page: bool,
    ) -> io::Result<PreparedHistory> {
        let _timer = crate::instrumentation_timer!("session.load.history_projection");
        let (anchored, notice_refs) = Self::source_indexes(&self.sources);
        self.project(
            session,
            timeline,
            cursor,
            self.record_count(),
            self.sources.iter().map(Cow::Borrowed),
            anchored,
            notice_refs,
            indexed_page,
            None,
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
        let cut = self.record_count();
        let sources = std::mem::take(&mut self.sources);
        self.project(
            session,
            timeline,
            cursor,
            cut,
            sources.into_iter().map(Cow::Owned),
            anchored,
            notice_refs,
            false,
            None,
        )
    }

    /// Capture the exact physical intervals discarded by the existing display
    /// projector during an explicit branch publication.
    pub(crate) fn rewind_visibility(
        &self,
        session: &acp::SessionId,
        timeline: &chat_state::Timeline,
    ) -> io::Result<Vec<std::ops::Range<u64>>> {
        let (anchored, notice_refs) = Self::source_indexes(&self.sources);
        let mut discarded = Vec::new();
        self.project(
            session,
            timeline,
            None,
            self.record_count(),
            self.sources.iter().map(Cow::Borrowed),
            anchored,
            notice_refs,
            false,
            Some(&mut discarded),
        )?;
        discarded.sort_unstable_by_key(|range| range.start);
        let mut combined: Vec<std::ops::Range<u64>> = Vec::new();
        for range in discarded {
            if let Some(previous) = combined
                .last_mut()
                .filter(|previous| previous.end >= range.start)
            {
                previous.end = previous.end.max(range.end);
            } else {
                combined.push(range);
            }
        }
        Ok(combined)
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
        indexed_page: bool,
        rewind_visibility: Option<&mut Vec<std::ops::Range<u64>>>,
    ) -> io::Result<PreparedHistory> {
        Self::validate_owner(timeline, session.0.as_ref())?;
        let admitted = match self.indexed.as_ref().filter(|_| indexed_page) {
            Some(index) => timeline
                .admitted_response_views_since(chat_state::EventSeq::new(index.published_domain())),
            None => timeline.admitted_response_views(),
        };
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
        let mut pending_rewind = None;
        let mut recovered_markers = Vec::new();
        for (position, row) in sources.into_iter().enumerate() {
            let source = row.as_ref();
            let at_ms = match source {
                HistorySource::Presentation { at_ms, .. } => Some(*at_ms),
                _ => None,
            };
            let position = self
                .positions
                .get(position)
                .copied()
                .unwrap_or(position as u64);
            match source {
                HistorySource::Domain { event, .. } => {
                    if let Some(target) =
                        timeline.rewind_display_target(chat_state::EventSeq::new(*event))
                    {
                        recovered_markers.extend(pending_rewind.replace((position, target)));
                    }
                }
                HistorySource::Presentation { fact, .. }
                    if matches!(&fact.content, HistoryContent::Grow(value)
                        if matches!(value.as_ref(), super::GrowHistoryFact::RewindMarker { .. })) =>
                {
                    pending_rewind = None;
                }
                _ => {}
            }
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
                #[cfg(test)]
                BODY_PROJECTIONS.set(BODY_PROJECTIONS.get() + 1);
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
                            timeline
                                .event(chat_state::EventSeq::new(*event))
                                .expect("validated response source")
                                .at_ms
                                .into(),
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
                let domain = timeline
                    .event(chat_state::EventSeq::new(*event))
                    .expect("validated domain source");
                if self.indexed.is_some()
                    && let chat_state::TimelineEventKind::Hook(chat_state::HookEvent::Completed {
                        occurrence_id,
                        ..
                    }) = &domain.kind
                    && let Some(projection) = timeline.hook_projection(occurrence_id)
                {
                    let name = crate::session::actor::timeline_hook_event_name(projection.event);
                    if let Some(update) =
                        crate::session::actor::SessionActor::project_hook_execution(
                            name,
                            None,
                            None,
                            &projection,
                            true,
                        )
                    {
                        let mut update = SessionUpdate::Grow(Box::new(GrowNotification {
                            session_id: session.clone(),
                            update,
                            meta: None,
                        }));
                        self.stamp_cursor(
                            session,
                            cut,
                            Some(domain.at_ms),
                            position,
                            0,
                            true,
                            &mut update,
                        )?;
                        updates.push(update);
                    }
                }
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
                            serde_json::json!({"eventId": format!("{}-receipt-{event}", session.0), "grow/historyTimestampMs": domain.at_ms}),
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
            if indexed_page
                && let Some(index) = &self.indexed
                && position < index.records()
                && let HistoryContent::User(chunk) = &fact.content
                && super::user_echo_identity(chunk).is_some()
                && !super::provisional_user_echo(chunk)
            {
                // These rows were fetched as evidence for a suffix consume.
                // The index already owns their original presentation positions.
                continue;
            }
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
                    #[cfg(test)]
                    BODY_PROJECTIONS.set(BODY_PROJECTIONS.get() + 1);
                    let event = timeline
                        .exact_event(chat_state::EventSeq::new(*event))?
                        .ok_or_else(|| invalid("history message source is absent"))?;
                    let ordinal = *item;
                    let item = fact.inherited_item(&event)?;
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
        recovered_markers.extend(pending_rewind);
        if !recovered_markers.is_empty() {
            for (record, target_prompt_index) in recovered_markers {
                let mut marker = SessionUpdate::Grow(Box::new(GrowNotification {
                    session_id: session.clone(),
                    update: GrowUpdate::RewindMarker {
                        target_prompt_index,
                        created_at: String::new(),
                    },
                    meta: None,
                }));
                self.stamp_cursor(session, cut, None, record, 0, true, &mut marker)?;
                updates.push(marker);
            }
            updates.sort_by_key(|update| {
                let cursor = cursor_from_update(update).expect("projected row has a cursor");
                (cursor.record, cursor.part)
            });
        }
        let updates = if let Some(discarded) = rewind_visibility {
            crate::session::storage::filter_rewind_updates_with(updates, |first, marker| {
                let end = cursor_from_update(marker)
                    .expect("projected marker has a cursor")
                    .record;
                let start = first
                    .and_then(cursor_from_update)
                    .map_or(end, |first| first.record);
                discarded.push(start..end + 1);
            })
        } else {
            crate::session::storage::filter_rewind_updates(updates)
        };
        let mut updates =
            self.project_command_terminals(session, cut, updates, settlement_sources)?;
        if indexed_page && let Some(index) = &self.indexed {
            updates.retain(|update| {
                let Some(cursor) = cursor_from_update(update) else {
                    return true;
                };
                if cursor.record >= index.records() {
                    return true;
                }
                // An unresolved start joins a tail settlement in the
                // execution projection. Its start row is already on disk.
                let Ok(i) = self.positions.binary_search(&cursor.record) else {
                    return true;
                };
                !matches!(&self.sources[i], HistorySource::Presentation { fact, .. }
                    if super::indexes_background_task(fact))
            });
        }
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
                let GrowUpdate::TaskBackgrounded { tool_call_id, .. } = &notification.update else {
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
                completions.push(command_settlement_update(
                    session,
                    &notification.update,
                    start_ms,
                    settlement,
                    event.at_ms,
                ));
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

    pub(crate) fn stamp_cursor(
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
