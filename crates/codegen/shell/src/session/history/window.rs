//! Fixed-cut window envelopes and the forward indexed read path.
use super::*;
use crate::session::history::{
    HistoryEntity, HistoryFragment, HistoryFragmentCoordinate, HistoryRange, HistoryRow,
    HistorySourceKey, PageWindow,
};

fn update_meta(update: &SessionUpdate) -> Option<&acp::Meta> {
    match update {
        SessionUpdate::Acp(value) => value.meta.as_ref(),
        SessionUpdate::Grow(value) => value.meta.as_ref()?.as_object(),
        #[cfg(any(test, feature = "test-support"))]
        SessionUpdate::ResponseReplayProjection(_) => None,
    }
}

fn envelope(source: (u64, u64), update: SessionUpdate) -> io::Result<HistoryRow> {
    let meta = update_meta(&update);
    let message = || {
        Some(HistoryEntity::Message {
            event_seq: meta?.get(RESPONSE_EVENT_KEY)?.as_u64()?,
            item_ordinal: meta?.get(RESPONSE_ITEM_KEY)?.as_u64()?,
        })
    };
    let mut fragment = None;
    if let Some(meta) = meta {
        for (coordinate, start_key, end_key) in [
            (
                HistoryFragmentCoordinate::Utf8Bytes,
                "grow/historyResponseStartByte",
                "grow/historyResponseEndByte",
            ),
            (
                HistoryFragmentCoordinate::EncodedResponseBytes,
                "grow/historyResponseEncodedStartByte",
                "grow/historyResponseEncodedEndByte",
            ),
        ] {
            if let Some(start) = meta.get(start_key).and_then(serde_json::Value::as_u64) {
                let end = meta
                    .get(end_key)
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| invalid("history fragment has no byte end"))?;
                let total = meta
                    .get("grow/historyResponseTotalBytes")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| invalid("history fragment has no body length"))?;
                if start > end || end > total {
                    return Err(invalid("history fragment crosses its body"));
                }
                fragment = Some(HistoryFragment {
                    coordinate,
                    start,
                    end_exclusive: end,
                    complete: start == 0 && end == total,
                });
            }
        }
    }
    let entity = match &update {
        SessionUpdate::Acp(notification) => match &notification.update {
            acp::SessionUpdate::ToolCall(call) => Some(HistoryEntity::Tool {
                tool_call_id: call.tool_call_id.0.to_string(),
            }),
            acp::SessionUpdate::ToolCallUpdate(call) => Some(HistoryEntity::Tool {
                tool_call_id: call.tool_call_id.0.to_string(),
            }),
            acp::SessionUpdate::UserMessageChunk(chunk) => chunk
                .meta
                .as_ref()
                .and_then(|meta| meta.get("messageId"))
                .and_then(serde_json::Value::as_str)
                .map(|message_id| HistoryEntity::UserMessage {
                    message_id: message_id.to_owned(),
                })
                .or_else(message),
            _ => message(),
        },
        SessionUpdate::Grow(notification) => match &notification.update {
            GrowUpdate::ToolOutputFragment(value) => {
                let end = value.byte_start + value.json.len() as u64;
                if end > value.total_bytes {
                    return Err(invalid("Tool fragment crosses its body"));
                }
                fragment = Some(HistoryFragment {
                    coordinate: HistoryFragmentCoordinate::ToolPayloadJsonBytes,
                    start: value.byte_start,
                    end_exclusive: end,
                    complete: value.byte_start == 0 && end == value.total_bytes,
                });
                Some(HistoryEntity::Tool {
                    tool_call_id: value.tool_call_id.0.to_string(),
                })
            }
            _ => None,
        },
        #[cfg(any(test, feature = "test-support"))]
        SessionUpdate::ResponseReplayProjection(_) => {
            return Err(invalid("unexpanded history response"));
        }
    };
    Ok(HistoryRow {
        source: HistorySourceKey {
            record: source.0,
            part: source.1,
        },
        entity,
        fragment,
        update,
    })
}

fn position(row: &HistoryRow, forward: bool) -> PagePosition {
    let offset = row.fragment.and_then(|fragment| {
        if forward {
            let total = match &row.update {
                SessionUpdate::Grow(notification) => match &notification.update {
                    GrowUpdate::ToolOutputFragment(value) => Some(value.total_bytes),
                    _ => None,
                },
                _ => update_meta(&row.update)
                    .and_then(|meta| meta.get("grow/historyResponseTotalBytes"))
                    .and_then(serde_json::Value::as_u64),
            };
            total
                .filter(|total| fragment.end_exclusive < *total)
                .map(|_| fragment.end_exclusive as usize)
        } else {
            (fragment.start != 0).then_some(fragment.start as usize)
        }
    });
    PagePosition {
        record: row.source.record,
        part: row.source.part,
        offset,
    }
}

fn window(
    rows: Vec<HistoryRow>,
    older_position: Option<PagePosition>,
    newer_position: Option<PagePosition>,
) -> PageWindow {
    positioned_window(rows, older_position, newer_position, None)
}

fn positioned_window(
    rows: Vec<HistoryRow>,
    older_position: Option<PagePosition>,
    newer_position: Option<PagePosition>,
    target_source: Option<HistorySourceKey>,
) -> PageWindow {
    let range = rows
        .first()
        .zip(rows.last())
        .map(|(first, last)| HistoryRange {
            start: first.source,
            end: last.source,
        });
    PageWindow {
        rows,
        range,
        older_position,
        newer_position,
        target_source,
    }
}

/// One before page and one after page, then the row/byte budget. Cursors come
/// from the rows that remain, so a trimmed neighbor is still reachable.
fn finish_around(
    before: PageWindow,
    after: PageWindow,
    target: HistorySourceKey,
    mut has_before: impl FnMut(PagePosition) -> io::Result<bool>,
    mut has_after: impl FnMut(PagePosition) -> io::Result<bool>,
) -> io::Result<PageWindow> {
    if after.rows.first().is_none_or(|row| row.source != target) {
        return Err(invalid("history around target is not visible"));
    }
    let target_index = before.rows.len();
    let mut rows = before.rows;
    rows.extend(after.rows);
    let rows = trim_around(rows, target_index)?;
    let (older, newer) = row_boundaries(&rows, &mut has_before, &mut has_after)?;
    Ok(positioned_window(rows, older, newer, Some(target)))
}

fn trim_around(mut rows: Vec<HistoryRow>, mut target_index: usize) -> io::Result<Vec<HistoryRow>> {
    let drop_far = |rows: &mut Vec<HistoryRow>, target_index: &mut usize| {
        let left = *target_index;
        let right = rows.len() - *target_index - 1;
        if left >= right && *target_index > 0 {
            rows.remove(0);
            *target_index -= 1;
            true
        } else if right > 0 {
            rows.pop();
            true
        } else {
            false
        }
    };
    while rows.len() > HISTORY_PAGE_ROWS && drop_far(&mut rows, &mut target_index) {}
    while rows.len() > 1
        && serde_json::to_vec(&rows)?.len() > HISTORY_PAGE_BYTES
        && drop_far(&mut rows, &mut target_index)
    {}
    if serde_json::to_vec(&rows)?.len() > HISTORY_PAGE_BYTES {
        return Err(invalid("history row envelope exceeds the page byte budget"));
    }
    Ok(rows)
}

fn row_boundaries(
    rows: &[HistoryRow],
    has_before: &mut impl FnMut(PagePosition) -> io::Result<bool>,
    has_after: &mut impl FnMut(PagePosition) -> io::Result<bool>,
) -> io::Result<(Option<PagePosition>, Option<PagePosition>)> {
    let older = match rows.first() {
        Some(row) => {
            let boundary = position(row, false);
            (boundary.offset.is_some() || has_before(boundary)?).then_some(boundary)
        }
        None => None,
    };
    let newer = match rows.last() {
        Some(row) => {
            let boundary = position(row, true);
            (boundary.offset.is_some() || has_after(boundary)?).then_some(boundary)
        }
        None => None,
    };
    Ok((older, newer))
}

fn next_key(key: (u64, u64)) -> (u64, u64) {
    key.1
        .checked_add(1)
        .map_or((key.0.saturating_add(1), 0), |part| (key.0, part))
}

impl IndexedPages {
    /// Before uses the start of a delivered fragment as its exclusive byte end.
    pub(crate) fn read_before(
        &self,
        session: &acp::SessionId,
        mut before: PagePosition,
    ) -> io::Result<PageWindow> {
        if before.offset == Some(0) {
            before.offset = None;
        }
        let (mut older, updates) = self.page_before(session, before)?;
        let mut rows = updates
            .into_iter()
            .map(|update| {
                let cursor = cursor_from_update(&update)
                    .ok_or_else(|| invalid("history row has no source"))?;
                envelope((cursor.record, cursor.part as u64), update)
            })
            .collect::<io::Result<Vec<_>>>()?;
        // The wire envelope is part of the existing page byte budget.
        let mut trimmed = false;
        while serde_json::to_vec(&rows)?.len() > HISTORY_PAGE_BYTES && rows.len() > 1 {
            rows.remove(0);
            trimmed = true;
        }
        if trimmed {
            older = match rows.first() {
                Some(row) => {
                    let boundary = position(row, false);
                    (boundary.offset.is_some() || self.has_before(boundary)?).then_some(boundary)
                }
                None => None,
            };
        }
        if serde_json::to_vec(&rows)?.len() > HISTORY_PAGE_BYTES {
            return Err(invalid("history row envelope exceeds the page byte budget"));
        }
        let newer = match rows.last() {
            Some(row) => {
                let boundary = position(row, true);
                (boundary.offset.is_some() || self.has_after(boundary)?).then_some(boundary)
            }
            None => None,
        };
        Ok(window(rows, older, newer))
    }

    /// After uses the endExclusive of a delivered fragment as its byte start.
    pub(crate) fn read_after(
        &self,
        session: &acp::SessionId,
        mut after: PagePosition,
    ) -> io::Result<PageWindow> {
        loop {
            let count = self.history.record_count();
            if count == 0 || after.record >= count {
                return Ok(window(Vec::new(), None, None));
            }
            let visible = self.index.visible_range_after(after.record, count - 1)?;
            if visible.is_empty() {
                return Ok(window(Vec::new(), None, None));
            }
            if *visible.start() != after.record {
                after = PagePosition {
                    record: *visible.start(),
                    part: 0,
                    offset: Some(0),
                };
            }
            let (next, rows) = self.after_in_window(session, after, *visible.end())?;
            if rows.is_empty() {
                if let Some(next) = next {
                    after = next;
                    continue;
                }
            }
            let older = match rows.first() {
                Some(row) => {
                    let boundary = position(row, false);
                    (boundary.offset.is_some() || self.has_before(boundary)?).then_some(boundary)
                }
                None => None,
            };
            return Ok(window(rows, older, next));
        }
    }

    /// Seek the target source. A missing or invisible source is an error, not a neighbor.
    pub(crate) fn read_around(
        &self,
        session: &acp::SessionId,
        target: HistorySourceKey,
    ) -> io::Result<PageWindow> {
        let before = self.read_before(
            session,
            PagePosition {
                record: target.record,
                part: target.part,
                offset: None,
            },
        )?;
        let after = self.read_after(
            session,
            PagePosition {
                record: target.record,
                part: target.part,
                offset: Some(0),
            },
        )?;
        finish_around(
            before,
            after,
            target,
            |position| self.has_before(position),
            |position| self.has_after(position),
        )
    }

    pub(crate) fn timeline_newest(
        &self,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        self.timeline.page(
            &self.index,
            crate::session::history::TimelineQuery::Newest,
            limit,
        )
    }

    pub(crate) fn timeline_before(
        &self,
        before: u64,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        self.timeline.page(
            &self.index,
            crate::session::history::TimelineQuery::Before(before),
            limit,
        )
    }

    pub(crate) fn timeline_around_ordinal(
        &self,
        ordinal: u64,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        self.timeline.page(
            &self.index,
            crate::session::history::TimelineQuery::Around(ordinal),
            limit,
        )
    }

    fn after_candidates(
        &self,
        after: PagePosition,
        upper: u64,
        limit: usize,
    ) -> io::Result<(Vec<((u64, u64), PageRow)>, Option<(u64, u64)>)> {
        let reader = self.index.page_reader()?;
        let bound = if after.offset.is_some() {
            after.key()
        } else {
            next_key(after.key())
        };
        let mut candidates = Vec::with_capacity(5 * limit);
        let mut ceiling = None;
        let mut cap = |key, count| {
            if count == limit {
                ceiling = Some(ceiling.map_or(key, |old: (u64, u64)| old.min(key)));
            }
        };
        let responses = reader.response_rows_after(bound, limit, self.history.response_branch)?;
        if let Some(last) = responses.last() {
            cap(last.position(), responses.len());
        }
        candidates.extend(
            responses
                .into_iter()
                .map(|row| (row.position(), PageRow::Response(row))),
        );
        let echoes = reader.user_echo_rows_after(bound, limit)?;
        if let Some(last) = echoes.last() {
            cap(last.position(), echoes.len());
        }
        candidates.extend(
            echoes
                .into_iter()
                .map(|row| (row.position(), PageRow::UserEcho(row))),
        );
        let terminals = reader.task_terminals_after(bound, limit, self.history.record_count())?;
        if let Some(last) = terminals.last() {
            cap(last.position(), terminals.len());
        }
        candidates.extend(
            terminals
                .into_iter()
                .map(|row| (row.position(), PageRow::TaskTerminal(row))),
        );
        let start = self
            .rows
            .partition_point(|(position, _)| position.key() < bound);
        let end = (start + limit).min(self.rows.len());
        if end > start {
            cap(self.rows[end - 1].0.key(), end - start);
        }
        candidates.extend((start..end).map(|i| (self.rows[i].0.key(), PageRow::Retained(i))));
        let display_start = self
            .index
            .omitted_before(bound.0.saturating_add(u64::from(bound.1 > 0)))?;
        let display_end = self
            .index
            .omitted_before(upper.saturating_add(1))?
            .min(display_start.saturating_add(limit));
        for rank in display_start..display_end {
            let key = (self.index.display_position(rank)?, 0);
            if rank + 1 == display_end {
                cap(key, display_end - display_start);
            }
            candidates.push((key, PageRow::Display(rank)));
        }
        candidates.retain(|(key, _)| key.0 <= upper);
        candidates.sort_unstable_by_key(|(key, _)| *key);
        Ok((candidates, ceiling))
    }

    fn after_in_window(
        &self,
        session: &acp::SessionId,
        after: PagePosition,
        upper: u64,
    ) -> io::Result<(Option<PagePosition>, Vec<HistoryRow>)> {
        let (candidates, ceiling) = self.after_candidates(after, upper, HISTORY_PAGE_ROWS + 1)?;
        let mut reader = self.index.page_reader()?;
        let mut rows = Vec::new();
        let mut budget = HISTORY_PAGE_BYTES - 2;
        let mut cursor = None;
        for ((record, part), row) in candidates {
            if rows.len() == HISTORY_PAGE_ROWS
                || ceiling.is_some_and(|ceiling| (record, part) > ceiling)
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
            let offset = ((record, part) == after.key())
                .then_some(after.offset)
                .flatten();
            let (newer_offset, update) = match row {
                PageRow::Response(row) => {
                    let bytes = row
                        .encoded_bytes()
                        .saturating_sub(offset.unwrap_or(0) as u64);
                    if !rows.is_empty() && bytes.saturating_add(1024) > budget as u64 {
                        break;
                    }
                    reader.read_response_row_after(
                        row,
                        offset,
                        session,
                        &self.history,
                        budget.saturating_sub(1024) as u64,
                    )?
                }
                PageRow::Retained(index) => {
                    let mut update = self.rows[index].1.clone();
                    let newer = slice_text(
                        &mut update,
                        offset.unwrap_or(0),
                        budget.saturating_sub(512),
                        true,
                    )?;
                    (newer, update)
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
                    let Some(row) =
                        reader.read_display_page_after(rank, session, &self.history, offset)?
                    else {
                        cursor = Some(PagePosition {
                            record,
                            part,
                            offset: None,
                        });
                        continue;
                    };
                    row
                }
            };
            let row = envelope((record, part), update)?;
            let bytes = serde_json::to_vec(&row)?.len() + 1;
            if bytes > budget {
                if rows.is_empty() {
                    return Err(invalid("history row envelope exceeds the page byte budget"));
                }
                break;
            }
            budget -= bytes;
            rows.push(row);
            cursor = Some(PagePosition {
                record,
                part,
                offset: newer_offset,
            });
            if newer_offset.is_some() {
                break;
            }
        }
        if let Some(boundary) = cursor {
            if boundary.offset.is_none() && !self.has_after(boundary)? {
                cursor = None;
            }
        } else if upper + 1 < self.history.record_count() {
            cursor = Some(PagePosition {
                record: upper + 1,
                part: 0,
                offset: Some(0),
            });
        }
        Ok((cursor, rows))
    }

    fn has_after(&self, mut after: PagePosition) -> io::Result<bool> {
        let count = self.history.record_count();
        while after.record < count {
            let visible = self.index.visible_range_after(after.record, count - 1)?;
            if visible.is_empty() {
                return Ok(false);
            }
            if *visible.start() != after.record {
                after = PagePosition {
                    record: *visible.start(),
                    part: 0,
                    offset: Some(0),
                };
            }
            let (rows, _) = self.after_candidates(after, *visible.end(), 1)?;
            if !rows.is_empty() {
                return Ok(true);
            }
            if *visible.end() + 1 == count {
                break;
            }
            after = PagePosition {
                record: *visible.end() + 1,
                part: 0,
                offset: Some(0),
            };
        }
        Ok(false)
    }

    fn has_before(&self, mut before: PagePosition) -> io::Result<bool> {
        let reader = self.index.page_reader()?;
        loop {
            let anchor = if before.part == 0 {
                match before.record.checked_sub(1) {
                    Some(value) => value,
                    None => return Ok(false),
                }
            } else {
                before.record
            };
            let visible = self.index.visible_range(anchor)?;
            if visible.is_empty() {
                return Ok(false);
            }
            if *visible.end() != anchor {
                before = PagePosition {
                    record: *visible.end() + 1,
                    part: 0,
                    offset: None,
                };
            }
            let lower = *visible.start();
            if self
                .rows
                .iter()
                .any(|(row, _)| row.record >= lower && row.key() < before.key())
                || self.index.omitted_before(before.record)? > self.index.omitted_before(lower)?
                || reader
                    .response_rows_before(before.key(), 1, self.history.response_branch)?
                    .first()
                    .is_some_and(|row| row.position().0 >= lower)
                || reader
                    .user_echo_rows_before(before.key(), 1)?
                    .first()
                    .is_some_and(|row| row.position().0 >= lower)
                || reader
                    .task_terminals_before(before.key(), 1, self.history.record_count())?
                    .first()
                    .is_some_and(|row| row.position().0 >= lower)
            {
                return Ok(true);
            }
            if lower == 0 {
                return Ok(false);
            }
            before = PagePosition {
                record: lower,
                part: 0,
                offset: None,
            };
        }
    }
}

/// Stable prepared source for publications without disk indexes. Capture once
/// from the validated cut, then query without reopening its journal.
#[derive(Debug, Clone)]
pub(crate) struct PreparedPages {
    rows: Vec<HistoryRow>,
    prompts: Vec<crate::session::history::HistoryTimelineEntry>,
}

impl PreparedHistory {
    pub(crate) fn into_pages(self) -> io::Result<PreparedPages> {
        let mut rows = self
            .updates
            .into_iter()
            .map(|update| {
                let cursor = cursor_from_update(&update)
                    .ok_or_else(|| invalid("prepared row has no source"))?;
                envelope((cursor.record, cursor.part as u64), update)
            })
            .collect::<io::Result<Vec<_>>>()?;
        rows.sort_by_key(|row| row.source);
        let prompts = super::super::timeline::capture(&rows);
        Ok(PreparedPages { rows, prompts })
    }
}

impl PreparedPages {
    pub(crate) fn read_before(&self, before: PagePosition) -> io::Result<PageWindow> {
        self.read(before, false)
    }

    pub(crate) fn read_after(&self, after: PagePosition) -> io::Result<PageWindow> {
        self.read(after, true)
    }

    pub(crate) fn read_around(&self, target: HistorySourceKey) -> io::Result<PageWindow> {
        let before = self.read_before(PagePosition {
            record: target.record,
            part: target.part,
            offset: None,
        })?;
        let after = self.read_after(PagePosition {
            record: target.record,
            part: target.part,
            offset: Some(0),
        })?;
        finish_around(
            before,
            after,
            target,
            |position| Ok(self.has_content_before(position)),
            |position| Ok(self.has_content_after(position)),
        )
    }

    fn has_content_before(&self, before: PagePosition) -> bool {
        let key = HistorySourceKey {
            record: before.record,
            part: before.part,
        };
        self.rows.iter().any(|row| row.source < key)
    }

    fn has_content_after(&self, after: PagePosition) -> bool {
        let key = HistorySourceKey {
            record: after.record,
            part: after.part,
        };
        self.rows.iter().any(|row| row.source > key)
    }

    fn read(&self, boundary: PagePosition, forward: bool) -> io::Result<PageWindow> {
        let key = HistorySourceKey {
            record: boundary.record,
            part: boundary.part,
        };
        let inclusive = boundary.offset.is_some_and(|offset| forward || offset != 0);
        let start = self.rows.partition_point(|row| {
            row.source < key || (forward && !inclusive && row.source == key)
        });
        let end = self.rows.partition_point(|row| {
            row.source < key || (!forward && inclusive && row.source == key)
        });
        let indices: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(start..self.rows.len())
        } else {
            Box::new((0..end).rev())
        };
        let mut rows = Vec::new();
        let mut budget = HISTORY_PAGE_BYTES - 2;
        let mut first_index = None;
        let mut last_index = None;
        for index in indices {
            if rows.len() == HISTORY_PAGE_ROWS {
                break;
            }
            let mut row = self.rows[index].clone();
            let at_boundary = row.source == key;
            let offset = if at_boundary { boundary.offset } else { None };
            let partial = slice_text(
                &mut row.update,
                offset.unwrap_or(if forward { 0 } else { usize::MAX }),
                budget.saturating_sub(512),
                forward,
            )?;
            row = envelope((row.source.record, row.source.part), row.update)?;
            let bytes = serde_json::to_vec(&row)?.len() + 1;
            if bytes > budget {
                if rows.is_empty() {
                    return Err(invalid("prepared history row exceeds the page byte budget"));
                }
                break;
            }
            budget -= bytes;
            first_index.get_or_insert(index);
            last_index = Some(index);
            rows.push(row);
            if partial.is_some() {
                break;
            }
        }
        if !forward {
            rows.reverse();
        }
        let low = first_index
            .zip(last_index)
            .map(|(first, last)| first.min(last));
        let high = first_index
            .zip(last_index)
            .map(|(first, last)| first.max(last));
        let older = rows.first().and_then(|row| {
            let boundary = position(row, false);
            (boundary.offset.is_some() || low.is_some_and(|index| index != 0)).then_some(boundary)
        });
        let newer = rows.last().and_then(|row| {
            let boundary = position(row, true);
            (boundary.offset.is_some() || high.is_some_and(|index| index + 1 < self.rows.len()))
                .then_some(boundary)
        });
        Ok(window(rows, older, newer))
    }

    pub(crate) fn timeline_newest(
        &self,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        crate::session::history::page_from(
            &self.prompts,
            crate::session::history::TimelineQuery::Newest,
            limit,
        )
    }

    pub(crate) fn timeline_before(
        &self,
        before: u64,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        crate::session::history::page_from(
            &self.prompts,
            crate::session::history::TimelineQuery::Before(before),
            limit,
        )
    }

    pub(crate) fn timeline_around_ordinal(
        &self,
        ordinal: u64,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        crate::session::history::page_from(
            &self.prompts,
            crate::session::history::TimelineQuery::Around(ordinal),
            limit,
        )
    }
}

/// Slice only already prepared UTF-8 bodies. Disk response/Tool bytes never use
/// this path; their indexes preserve the encoded or serialized coordinates.
fn slice_text(
    update: &mut SessionUpdate,
    offset: usize,
    budget: usize,
    forward: bool,
) -> io::Result<Option<usize>> {
    // Fork-inherited text has no sampling request id. The slice follows the
    // UTF-8 body, never response_coordinates.
    let Some(body) = text_body(update) else {
        return Ok(None);
    };
    let total = body.len();
    let offset = if !forward && offset == usize::MAX {
        total
    } else {
        offset
    };
    if offset > total || !body.is_char_boundary(offset) {
        return Err(invalid("history continuation crosses a text character"));
    }
    let mut start = if forward { offset } else { 0 };
    let mut end = if forward { total } else { offset };
    if start == end && total != 0 {
        return Err(invalid("history text cursor is outside its body"));
    }
    *body = body[start..end].to_owned();
    if serde_json::to_vec(update)?.len() > budget {
        if forward {
            let body = text_body(update).expect("text body");
            let source = std::mem::take(body);
            let available = budget.saturating_sub(serde_json::to_vec(update)?.len() + 128);
            let mut used = 0;
            let mut split = 0;
            for (index, ch) in source.char_indices() {
                let bytes = match ch {
                    '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
                    '\u{0000}'..='\u{001f}' => 6,
                    _ => ch.len_utf8(),
                };
                if used + bytes > available {
                    break;
                }
                used += bytes;
                split = index + ch.len_utf8();
            }
            if split == 0 {
                return Err(invalid("history text cannot fit in the page byte budget"));
            }
            *text_body(update).expect("text body") = source[..split].to_owned();
            end = start + split;
        } else {
            start = take_response_suffix(update, budget)?;
        }
    }
    if start != 0 || end != total {
        let meta = meta_mut(update);
        meta.insert("grow/historyResponseStartByte".into(), start.into());
        meta.insert("grow/historyResponseEndByte".into(), end.into());
        meta.insert("grow/historyResponseTotalBytes".into(), total.into());
    }
    Ok(if forward {
        (end < total).then_some(end)
    } else {
        (start > 0).then_some(start)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::notification::{
        SessionNotification as GrowNotification, SessionUpdate as GrowUpdate, ToolOutputFragment,
    };
    use crate::session::storage::SessionUpdate;

    fn text_row(record: u64, text: &str) -> HistoryRow {
        HistoryRow {
            source: HistorySourceKey { record, part: 0 },
            entity: None,
            fragment: None,
            update: SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
                acp::SessionId::new("around"),
                acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
                    acp::ContentBlock::Text(acp::TextContent::new(text)),
                )),
            ))),
        }
    }

    fn tool_row(record: u64) -> HistoryRow {
        let json = "{\"rawOutput\":\"continued\"}";
        let fragment = HistoryFragment {
            coordinate: HistoryFragmentCoordinate::ToolPayloadJsonBytes,
            start: 40,
            end_exclusive: 40 + json.len() as u64,
            complete: false,
        };
        HistoryRow {
            source: HistorySourceKey { record, part: 0 },
            entity: Some(HistoryEntity::Tool {
                tool_call_id: "fragmented-tool".into(),
            }),
            fragment: Some(fragment),
            update: SessionUpdate::Grow(Box::new(GrowNotification {
                session_id: acp::SessionId::new("around"),
                update: GrowUpdate::ToolOutputFragment(ToolOutputFragment {
                    tool_call_id: acp::ToolCallId::new("fragmented-tool"),
                    source_position: record,
                    digest: "digest".into(),
                    title: "Run migration".into(),
                    kind: acp::ToolKind::Execute,
                    status: acp::ToolCallStatus::Completed,
                    byte_start: 40,
                    total_bytes: 200,
                    json: json.into(),
                }),
                meta: None,
            })),
        }
    }

    fn bare(rows: Vec<HistoryRow>) -> PageWindow {
        PageWindow {
            rows,
            range: None,
            older_position: None,
            newer_position: None,
            target_source: None,
        }
    }

    #[test]
    fn around_trim_keeps_tool_json_bytes_and_the_remaining_boundary() {
        let tool = tool_row(200);
        let original = serde_json::to_value(&tool.update).unwrap();
        let before = bare(
            (0..200)
                .map(|record| text_row(record, "neighbor"))
                .collect(),
        );
        let after = bare(vec![tool.clone()]);
        let window =
            finish_around(before, after, tool.source, |_| Ok(true), |_| Ok(false)).unwrap();
        assert_eq!(window.rows.len(), HISTORY_PAGE_ROWS);
        assert_eq!(window.target_source, Some(tool.source));
        let kept = window.rows.last().unwrap();
        assert_eq!(kept.source, tool.source);
        assert_eq!(kept.fragment, tool.fragment);
        assert_eq!(serde_json::to_value(&kept.update).unwrap(), original);
        assert_ne!(window.rows[0].source.record, 0);
        assert_eq!(
            window.older_position.unwrap().record,
            window.rows[0].source.record
        );
        assert_eq!(
            window.newer_position.unwrap().offset,
            Some(tool.fragment.unwrap().end_exclusive as usize)
        );
    }
}
