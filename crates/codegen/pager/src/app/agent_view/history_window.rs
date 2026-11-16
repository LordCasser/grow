//! One ordered history interval in the main transcript.
//!
//! Live entries stay in the scrollback tail and in `AgentSession`'s tracker.
//! Pages are merged here, then reprojected once. An Around response replaces
//! the interval in `apply_history_page`: the merge discards the previous
//! interval, the single reproject installs the replacement, and failure
//! restores the prior snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};

use shell::extensions::notification::SessionUpdate as GrowUpdate;
use shell::session::storage::{
    HistoryEntity, HistoryFragmentCoordinate, HistoryPageQuery, HistoryPageResponse, HistoryRow,
    HistorySourceKey, SessionUpdate,
};

use super::AgentView;
use crate::acp::tracker::ObservedSamplingAttempt;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::entry::EntryId;
use crate::scrollback::state::ScrollbackState;
use crate::transcript_projection::TranscriptProjection;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum StableId {
    Gap,
    Tool(String),
    Message {
        event_seq: u64,
        item_ordinal: u64,
    },
    User {
        message_id: String,
        record: u64,
        part: u64,
    },
    Source {
        record: u64,
        part: u64,
    },
}

#[derive(Clone)]
struct Span {
    start: u64,
    end: u64,
    bytes: Vec<u8>,
    update: SessionUpdate,
}

#[derive(Clone)]
struct FragmentBody {
    coordinate: HistoryFragmentCoordinate,
    total: u64,
    entity: Option<HistoryEntity>,
    digest: Option<String>,
    tool_title: Option<String>,
    tool_kind: Option<acp_transport::protocol::ToolKind>,
    tool_status: Option<acp_transport::protocol::ToolCallStatus>,
    tool_id: Option<String>,
    spans: Vec<Span>,
}

#[derive(Clone)]
enum SourceSlot {
    Whole {
        entity: Option<HistoryEntity>,
        update: SessionUpdate,
    },
    Fragmented(FragmentBody),
}

#[derive(Clone)]
pub(super) struct HistoryWindow {
    older_cursor: Option<String>,
    newer_cursor: Option<String>,
    sources: BTreeMap<HistorySourceKey, SourceSlot>,
    installed: HashMap<StableId, Vec<EntryId>>,
    prefix_len: usize,
    /// The active interval replaced an earlier one. The gap row stays until
    /// the next replacement; adjacent pages do not set this.
    gap: bool,
    /// The next successful page establishes a new interval.
    fresh: bool,
}

impl Default for HistoryWindow {
    fn default() -> Self {
        Self {
            older_cursor: None,
            newer_cursor: None,
            sources: BTreeMap::new(),
            installed: HashMap::new(),
            prefix_len: 0,
            gap: false,
            fresh: false,
        }
    }
}

impl HistoryWindow {
    fn bounds(&self) -> Option<(HistorySourceKey, HistorySourceKey)> {
        let mut keys = self.sources.keys().copied();
        let first = keys.next()?;
        Some((first, keys.last().unwrap_or(first)))
    }
}

pub(crate) enum HistoryPageOutcome {
    Adopted,
    Discarded,
    Rejected(&'static str),
}

impl AgentView {
    pub(in crate::app) fn retire_history_window(&mut self) {
        self.history_generation = self.history_generation.wrapping_add(1);
        self.history_window = HistoryWindow::default();
        self.history_cut = None;
        self.history_page_cursor = None;
        self.history_latest_pending = false;
        self.history_is_partial = false;
        self.history_page_request = None;
        self.history_page_viewer = None;
        self.retire_history_timeline();
    }

    pub(in crate::app) fn arm_history_load(&mut self, cut: Option<String>) {
        self.retire_history_window();
        self.history_cut = cut.clone();
        self.history_latest_pending = cut.is_some();
        self.history_is_partial = cut.is_some();
    }

    pub(in crate::app) fn request_latest_history_page(
        &mut self,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> bool {
        self.push_history_request(effects, HistoryPageQuery::Latest)
    }

    pub(in crate::app) fn request_history_before(
        &mut self,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> bool {
        let Some(cursor) = self
            .history_window
            .older_cursor
            .clone()
            .or_else(|| self.history_page_cursor.clone())
        else {
            return false;
        };
        self.push_history_request(effects, HistoryPageQuery::Before { cursor })
    }

    pub(in crate::app) fn request_history_after(
        &mut self,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> bool {
        let Some(cursor) = self.history_window.newer_cursor.clone() else {
            return false;
        };
        self.push_history_request(effects, HistoryPageQuery::After { cursor })
    }

    /// Seek a signed target. The generation bump drops an older in-flight page.
    /// The current prefix stays until the response is adopted.
    pub(in crate::app) fn request_history_around(
        &mut self,
        target: &str,
        effects: &mut Vec<crate::app::actions::Effect>,
    ) -> bool {
        if self.history_cut.is_none() || self.session.session_id.is_none() {
            return false;
        }
        self.history_generation = self.history_generation.wrapping_add(1);
        self.history_page_request = None;
        self.push_history_request(
            effects,
            HistoryPageQuery::Around {
                target: target.to_string(),
            },
        )
    }

    fn push_history_request(
        &mut self,
        effects: &mut Vec<crate::app::actions::Effect>,
        query: HistoryPageQuery,
    ) -> bool {
        if self.history_page_request.is_some() {
            return false;
        }
        let Some(cut) = self.history_cut.clone() else {
            return false;
        };
        let Some(session_id) = self.session.session_id.clone() else {
            return false;
        };
        self.history_page_request_seq = self.history_page_request_seq.wrapping_add(1);
        let request_id = self.history_page_request_seq;
        self.history_page_request = Some(request_id);
        effects.push(crate::app::actions::Effect::ReadHistoryPage {
            agent_id: self.session.id,
            session_id,
            binding_epoch: self.session_binding_epoch,
            request_id,
            generation: self.history_generation,
            cut,
            query,
        });
        true
    }

    pub(in crate::app) fn apply_history_page(
        &mut self,
        generation: u64,
        query: &HistoryPageQuery,
        response: HistoryPageResponse,
    ) -> HistoryPageOutcome {
        if generation != self.history_generation {
            return HistoryPageOutcome::Discarded;
        }
        if self.history_cut.as_deref() != Some(response.cut.as_str()) {
            return HistoryPageOutcome::Discarded;
        }
        let snapshot = self.history_window.clone();
        if let Err(reason) = self.history_window.merge_page(query, &response) {
            self.history_window = snapshot;
            return HistoryPageOutcome::Rejected(reason);
        }
        let latest = matches!(query, HistoryPageQuery::Latest);
        let commit_history = !(latest && self.history_latest_pending);
        if let Err(reason) = self.project_history(commit_history) {
            self.history_window = snapshot;
            return HistoryPageOutcome::Rejected(reason);
        }
        self.history_page_cursor = self.history_window.older_cursor.clone();
        if latest {
            self.history_latest_pending = false;
        }
        self.history_is_partial = self.history_cut.is_some();
        HistoryPageOutcome::Adopted
    }

    fn project_history(&mut self, commit_history: bool) -> Result<(), &'static str> {
        let canonical = canonical_updates(&self.history_window)?;
        // The default projection collapses a message's later chunks by
        // messageId — the same single-entity rule as the live tracker, an
        // eager full projection, and the timeline catalog.
        let mut projection = TranscriptProjection::default();
        // fresh_continuation already carries the live appearance. The default
        // projection disables timestamps, and copying that back would wrap
        // history at a different width than the anchor was captured.
        projection.scrollback = self.scrollback.fresh_continuation();
        if self.history_window.gap {
            projection
                .scrollback
                .push_block(RenderBlock::notice("History gap"));
        }
        let mut groups: Vec<(StableId, Vec<EntryId>)> = Vec::new();
        if self.history_window.gap {
            let id = projection.scrollback.entry(0).map(|entry| entry.id);
            if let Some(id) = id {
                groups.push((StableId::Gap, vec![id]));
            }
        }
        let mut next_id = projection.scrollback.id_floor();
        for (stable, update) in canonical {
            if self.live_owns_update(&update) {
                continue;
            }
            projection.apply(shell::session::storage::transcript::TranscriptEvent {
                update,
                timestamp_ms: None,
                simulated_source: false,
            });
            let end_id = projection.scrollback.id_floor();
            let added = (next_id..end_id)
                .map(EntryId::new)
                .filter(|id| projection.scrollback.index_of_id(*id).is_some())
                .collect::<Vec<_>>();
            next_id = end_id;
            if let Some((_, ids)) = groups.iter_mut().find(|(existing, _)| existing == &stable) {
                for id in added {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
            } else if !added.is_empty() {
                groups.push((stable, added));
            }
        }
        projection.scrollback.finish_all_running();
        let map = retarget_map(
            &groups,
            &self.history_window.installed,
            &self.scrollback,
            self.history_window.prefix_len,
        );
        projection.scrollback.retarget_entry_ids(&map);
        let prefix_len = projection.scrollback.len();
        let installed = groups
            .into_iter()
            .map(|(stable, ids)| {
                let ids = ids
                    .into_iter()
                    .map(|id| map.get(&id).copied().unwrap_or(id))
                    .collect();
                (stable, ids)
            })
            .collect();
        self.scrollback.replace_history_prefix(
            projection.scrollback,
            self.history_window.prefix_len,
            commit_history,
        );
        self.history_window.installed = installed;
        self.history_window.prefix_len = prefix_len;
        Ok(())
    }

    pub(in crate::app) fn history_cut(&self) -> Option<&str> {
        self.history_cut.as_deref()
    }

    pub(in crate::app) fn history_older_cursor(&self) -> Option<&str> {
        self.history_window.older_cursor.as_deref()
    }

    pub(in crate::app) fn history_newer_cursor(&self) -> Option<&str> {
        self.history_window.newer_cursor.as_deref()
    }

    pub(in crate::app) fn history_has_gap(&self) -> bool {
        self.history_window.gap
    }

    pub(in crate::app) fn history_prefix_len(&self) -> usize {
        self.history_window.prefix_len
    }

    /// User-prompt entry installed for one catalog source. `display_segment`
    /// selects a combined bubble of that physical source; it is not a part.
    pub(in crate::app) fn history_entry_id_for_timeline(
        &self,
        source: HistorySourceKey,
        message_id: Option<&str>,
        display_segment: Option<u32>,
    ) -> Option<EntryId> {
        let ids = self
            .history_window
            .installed
            .iter()
            .find_map(|(stable, ids)| {
                let matched = match stable {
                    StableId::User {
                        message_id: id,
                        record,
                        part,
                    } => {
                        *record == source.record
                            && *part == source.part
                            && message_id.map(|expected| expected == id).unwrap_or(true)
                    }
                    StableId::Source { record, part } => {
                        message_id.is_none() && *record == source.record && *part == source.part
                    }
                    _ => false,
                };
                matched.then_some(ids)
            })?;
        let prompts: Vec<EntryId> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.scrollback
                    .get_by_id(*id)
                    .is_some_and(|entry| entry.block.is_user_prompt())
            })
            .collect();
        match display_segment {
            Some(segment) => prompts.get(segment as usize).copied(),
            None => (prompts.len() == 1).then_some(prompts[0]),
        }
    }

    /// First and last real entries of the installed history prefix.
    /// The explicit gap row is not an interval boundary. `None` when the
    /// prefix has no projected history entry, including a gap-only prefix.
    pub(crate) fn history_interval_entry_ids(&self) -> Option<(EntryId, EntryId)> {
        let prefix = self.history_window.prefix_len.min(self.scrollback.len());
        let gap = self
            .history_window
            .installed
            .get(&StableId::Gap)
            .map(|ids| ids.iter().copied().collect::<HashSet<_>>())
            .unwrap_or_default();
        let mut ids = (0..prefix).filter_map(|index| {
            let id = self.scrollback.entry(index)?.id;
            (!gap.contains(&id)).then_some(id)
        });
        let first = ids.next()?;
        Some((first, ids.last().unwrap_or(first)))
    }

    fn live_owns_update(&self, update: &SessionUpdate) -> bool {
        if let Some(tool_id) = tool_id(update)
            && let Some(entry_id) = self
                .session
                .tracker
                .tool_entry_id(&tool_id, &self.scrollback)
            && self
                .scrollback
                .index_of_id(entry_id)
                .is_some_and(|index| index >= self.history_window.prefix_len)
        {
            return true;
        }
        if let Some((request_id, attempt)) = sampling_identity(update)
            && matches!(
                self.session
                    .tracker
                    .observed_sampling_attempt(&request_id, attempt),
                Some(ObservedSamplingAttempt::Accepted | ObservedSamplingAttempt::Previewing)
            )
        {
            return true;
        }
        false
    }
}

impl HistoryWindow {
    fn merge_page(
        &mut self,
        query: &HistoryPageQuery,
        response: &HistoryPageResponse,
    ) -> Result<(), &'static str> {
        if matches!(query, HistoryPageQuery::Around { .. }) {
            self.sources.clear();
            self.gap = true;
            self.fresh = true;
        }
        let page_bounds = bounds_of(&response.rows);
        let window_bounds = self.bounds();
        if !self.fresh {
            match query {
                HistoryPageQuery::Latest => {
                    if let (Some(window), Some(page)) = (window_bounds, page_bounds)
                        && (page.1 < window.0 || page.0 > window.1)
                    {
                        return Err("history page is not in the active interval");
                    }
                }
                HistoryPageQuery::Before { .. } => {
                    let Some(window) = window_bounds else {
                        return Err("history page has no active interval");
                    };
                    if let Some(page) = page_bounds
                        && page.0 > window.1
                    {
                        return Err("history page is not adjacent");
                    }
                }
                HistoryPageQuery::After { .. } => {
                    let Some(window) = window_bounds else {
                        return Err("history page has no active interval");
                    };
                    if let Some(page) = page_bounds
                        && page.1 < window.0
                    {
                        return Err("history page is not adjacent");
                    }
                }
                HistoryPageQuery::Around { .. } => {}
            }
        }
        let mut sources = self.sources.clone();
        for row in &response.rows {
            insert_row(&mut sources, row)?;
        }
        self.sources = sources;
        match query {
            HistoryPageQuery::Latest if window_bounds.is_none() || self.fresh => {
                self.older_cursor = response.older_cursor.clone();
                self.newer_cursor = response.newer_cursor.clone();
            }
            HistoryPageQuery::Before { .. } => {
                self.older_cursor = response.older_cursor.clone();
            }
            HistoryPageQuery::After { .. } => {
                self.newer_cursor = response.newer_cursor.clone();
            }
            HistoryPageQuery::Around { .. } => {
                self.older_cursor = response.older_cursor.clone();
                self.newer_cursor = response.newer_cursor.clone();
            }
            HistoryPageQuery::Latest => {}
        }
        self.fresh = false;
        Ok(())
    }
}

fn bounds_of(rows: &[HistoryRow]) -> Option<(HistorySourceKey, HistorySourceKey)> {
    let mut keys = rows.iter().map(|row| row.source);
    let first = keys.next()?;
    Some(keys.fold((first, first), |(low, high), key| {
        (low.min(key), high.max(key))
    }))
}

fn insert_row(
    sources: &mut BTreeMap<HistorySourceKey, SourceSlot>,
    row: &HistoryRow,
) -> Result<(), &'static str> {
    let Some(fragment) = row.fragment else {
        return insert_whole(sources, row);
    };
    if fragment.start > fragment.end_exclusive {
        return Err("history fragment is inverted");
    }
    let (bytes, header) = fragment_payload(
        &row.update,
        fragment.coordinate,
        fragment.start,
        fragment.end_exclusive,
    )?;
    match sources.get_mut(&row.source) {
        None => {
            let mut body = FragmentBody {
                coordinate: fragment.coordinate,
                total: header.total,
                entity: row.entity.clone(),
                digest: header.digest,
                tool_title: header.title,
                tool_kind: header.kind,
                tool_status: header.status,
                tool_id: header.tool_id,
                spans: Vec::new(),
            };
            body.insert_span(Span {
                start: fragment.start,
                end: fragment.end_exclusive,
                bytes,
                update: row.update.clone(),
            })?;
            sources.insert(row.source, SourceSlot::Fragmented(body));
            Ok(())
        }
        Some(SourceSlot::Whole { .. }) => Err("history fragment overlaps a complete row"),
        Some(SourceSlot::Fragmented(body)) => {
            if body.coordinate != fragment.coordinate {
                return Err("history fragment coordinate changed");
            }
            if body.total != header.total {
                return Err("history fragment length changed");
            }
            if body.digest.is_some() && header.digest.is_some() && body.digest != header.digest {
                return Err("history fragment digest changed");
            }
            if let (Some(existing), Some(incoming)) = (&body.entity, &row.entity)
                && existing != incoming
            {
                return Err("history fragment entity changed");
            }
            if body.entity.is_none() {
                body.entity = row.entity.clone();
            }
            body.insert_span(Span {
                start: fragment.start,
                end: fragment.end_exclusive,
                bytes,
                update: row.update.clone(),
            })
        }
    }
}

fn insert_whole(
    sources: &mut BTreeMap<HistorySourceKey, SourceSlot>,
    row: &HistoryRow,
) -> Result<(), &'static str> {
    let encoded =
        serde_json::to_vec(&row.update).map_err(|_| "history row could not be compared")?;
    match sources.get(&row.source) {
        None => {
            sources.insert(
                row.source,
                SourceSlot::Whole {
                    entity: row.entity.clone(),
                    update: row.update.clone(),
                },
            );
            Ok(())
        }
        Some(SourceSlot::Fragmented(_)) => Err("history row overlaps a fragment"),
        Some(SourceSlot::Whole { update, .. }) => {
            let existing =
                serde_json::to_vec(update).map_err(|_| "history row could not be compared")?;
            if existing == encoded {
                Ok(())
            } else {
                Err("history row changed for the same source")
            }
        }
    }
}

struct FragmentHeader {
    total: u64,
    digest: Option<String>,
    title: Option<String>,
    kind: Option<acp_transport::protocol::ToolKind>,
    status: Option<acp_transport::protocol::ToolCallStatus>,
    tool_id: Option<String>,
}

fn fragment_payload(
    update: &SessionUpdate,
    coordinate: HistoryFragmentCoordinate,
    start: u64,
    end: u64,
) -> Result<(Vec<u8>, FragmentHeader), &'static str> {
    match coordinate {
        HistoryFragmentCoordinate::Utf8Bytes => {
            let text = chunk_text(update)?;
            if text.len() as u64 != end.saturating_sub(start) {
                return Err("history text fragment does not match its utf-8 range");
            }
            Ok((
                text.into_bytes(),
                FragmentHeader {
                    total: response_total(update)?,
                    digest: None,
                    title: None,
                    kind: None,
                    status: None,
                    tool_id: None,
                },
            ))
        }
        HistoryFragmentCoordinate::EncodedResponseBytes => {
            let text = chunk_text(update)?;
            let bytes = json_string_bytes(&text)?;
            if bytes.len() as u64 != end.saturating_sub(start) {
                return Err("history text fragment does not match its encoded range");
            }
            Ok((
                bytes,
                FragmentHeader {
                    total: response_total(update)?,
                    digest: None,
                    title: None,
                    kind: None,
                    status: None,
                    tool_id: None,
                },
            ))
        }
        HistoryFragmentCoordinate::ToolPayloadJsonBytes => {
            let SessionUpdate::Grow(notification) = update else {
                return Err("tool fragment is not a tool payload");
            };
            let GrowUpdate::ToolOutputFragment(fragment) = &notification.update else {
                return Err("tool fragment is not a tool payload");
            };
            if fragment.byte_start != start
                || fragment
                    .byte_start
                    .saturating_add(fragment.json.len() as u64)
                    != end
            {
                return Err("tool fragment does not match its json range");
            }
            Ok((
                fragment.json.as_bytes().to_vec(),
                FragmentHeader {
                    total: fragment.total_bytes,
                    digest: Some(fragment.digest.clone()),
                    title: Some(fragment.title.clone()),
                    kind: Some(fragment.kind.clone()),
                    status: Some(fragment.status.clone()),
                    tool_id: Some(fragment.tool_call_id.0.to_string()),
                },
            ))
        }
    }
}

impl FragmentBody {
    fn insert_span(&mut self, span: Span) -> Result<(), &'static str> {
        if span.start == span.end {
            return Ok(());
        }
        let mut merged = span;
        let mut kept = Vec::with_capacity(self.spans.len() + 1);
        for existing in std::mem::take(&mut self.spans) {
            if merged.end <= existing.start || existing.end <= merged.start {
                kept.push(existing);
                continue;
            }
            let overlap_start = merged.start.max(existing.start);
            let overlap_end = merged.end.min(existing.end);
            let incoming = &merged.bytes
                [(overlap_start - merged.start) as usize..(overlap_end - merged.start) as usize];
            let previous = &existing.bytes[(overlap_start - existing.start) as usize
                ..(overlap_end - existing.start) as usize];
            if incoming != previous {
                self.spans = kept;
                self.spans.push(existing);
                self.spans.push(merged);
                return Err("overlapping history fragments differ");
            }
            merged = union_span(merged, existing);
        }
        kept.push(merged);
        kept.sort_by_key(|span| span.start);
        self.spans = kept;
        Ok(())
    }
}

fn union_span(left: Span, right: Span) -> Span {
    let start = left.start.min(right.start);
    let end = left.end.max(right.end);
    let mut bytes = vec![0u8; (end - start) as usize];
    copy_span(&left, &mut bytes, start);
    copy_span(&right, &mut bytes, start);
    Span {
        start,
        end,
        bytes,
        update: right.update,
    }
}

fn copy_span(span: &Span, dest: &mut [u8], origin: u64) {
    let offset = (span.start - origin) as usize;
    dest[offset..offset + span.bytes.len()].copy_from_slice(&span.bytes);
}

fn canonical_updates(
    window: &HistoryWindow,
) -> Result<Vec<(StableId, SessionUpdate)>, &'static str> {
    let mut updates = Vec::new();
    for (source, slot) in &window.sources {
        match slot {
            SourceSlot::Whole { entity, update } => {
                updates.push((stable_id(source, entity.as_ref(), update), update.clone()));
            }
            SourceSlot::Fragmented(body) => {
                updates.push(canonical_fragment(source, body)?);
            }
        }
    }
    Ok(updates)
}

fn canonical_fragment(
    source: &HistorySourceKey,
    body: &FragmentBody,
) -> Result<(StableId, SessionUpdate), &'static str> {
    let template = body
        .spans
        .last()
        .ok_or("history fragment has no bytes")?
        .update
        .clone();
    let assembled = assemble_text(body)?;
    match body.coordinate {
        HistoryFragmentCoordinate::ToolPayloadJsonBytes => {
            let tool_id = body
                .tool_id
                .clone()
                .or_else(|| tool_entity(&body.entity))
                .ok_or("tool fragment has no tool call")?;
            let update = if assembled.complete {
                decode_tool(body, &assembled.text, &tool_id, &template)?
            } else {
                incomplete_tool(body, &assembled, &tool_id, &template)?
            };
            Ok((StableId::Tool(tool_id), update))
        }
        HistoryFragmentCoordinate::Utf8Bytes | HistoryFragmentCoordinate::EncodedResponseBytes => {
            let mut update = template;
            set_chunk_text(&mut update, assembled.text)?;
            Ok((stable_id(source, body.entity.as_ref(), &update), update))
        }
    }
}

struct Assembled {
    text: String,
    complete: bool,
    missing: String,
}

fn assemble_text(body: &FragmentBody) -> Result<Assembled, &'static str> {
    let mut runs: Vec<(u64, u64, Vec<u8>)> = Vec::new();
    for span in &body.spans {
        if let Some((_, end, bytes)) = runs.last_mut()
            && span.start == *end
        {
            *end = span.end;
            bytes.extend_from_slice(&span.bytes);
            continue;
        }
        if runs.last().is_some_and(|(_, end, _)| span.start < *end) {
            return Err("history fragments overlap after merge");
        }
        runs.push((span.start, span.end, span.bytes.clone()));
    }
    let mut missing = String::new();
    let mut note = |start: u64, end: u64| {
        if !missing.is_empty() {
            missing.push_str(", ");
        }
        missing.push_str(&format!("missing bytes {start}..{end}"));
    };
    let mut cursor = 0u64;
    let mut text = String::new();
    let tool = body.coordinate == HistoryFragmentCoordinate::ToolPayloadJsonBytes;
    for (start, end, bytes) in &runs {
        if *start > cursor {
            note(cursor, *start);
            if !tool {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&format!("missing bytes {cursor}..{start}\n"));
            }
        }
        if !tool {
            text.push_str(&decode_bytes(body.coordinate, bytes)?);
        }
        cursor = *end;
    }
    if cursor < body.total {
        note(cursor, body.total);
        if !tool && cursor != 0 {
            text.push('\n');
            text.push_str(&format!("missing bytes {cursor}..{}", body.total));
        }
    }
    let covered = runs.first().is_some_and(|(start, _, _)| *start == 0)
        && cursor == body.total
        && missing.is_empty();
    if tool && covered {
        text = decode_bytes(body.coordinate, &runs[0].2)?;
    }
    Ok(Assembled {
        complete: covered,
        text,
        missing,
    })
}

fn decode_bytes(
    coordinate: HistoryFragmentCoordinate,
    bytes: &[u8],
) -> Result<String, &'static str> {
    match coordinate {
        HistoryFragmentCoordinate::Utf8Bytes | HistoryFragmentCoordinate::ToolPayloadJsonBytes => {
            String::from_utf8(bytes.to_vec()).map_err(|_| "history fragment is not utf-8")
        }
        HistoryFragmentCoordinate::EncodedResponseBytes => {
            let mut quoted = Vec::with_capacity(bytes.len() + 2);
            quoted.push(b'"');
            quoted.extend_from_slice(bytes);
            quoted.push(b'"');
            serde_json::from_slice(&quoted).map_err(|_| "history text fragment is not encoded text")
        }
    }
}

fn decode_tool(
    body: &FragmentBody,
    json: &str,
    tool_id: &str,
    template: &SessionUpdate,
) -> Result<SessionUpdate, &'static str> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct ToolPayload {
        #[serde(default)]
        content: Vec<acp_transport::protocol::ToolCallContent>,
        #[serde(default)]
        locations: Vec<acp_transport::protocol::ToolCallLocation>,
        #[serde(default)]
        raw_input: Option<serde_json::Value>,
        #[serde(default)]
        raw_output: Option<serde_json::Value>,
    }
    let payload: ToolPayload =
        serde_json::from_str(json).map_err(|_| "tool fragment json is incomplete")?;
    let session = session_id_of(template).ok_or("tool fragment has no session")?;
    let call = acp_transport::protocol::ToolCall::new(
        acp_transport::protocol::ToolCallId::new(tool_id.to_owned()),
        body.tool_title.clone().unwrap_or_default(),
    )
    .kind(
        body.tool_kind
            .clone()
            .unwrap_or(acp_transport::protocol::ToolKind::Other),
    )
    .status(
        body.tool_status
            .clone()
            .unwrap_or(acp_transport::protocol::ToolCallStatus::Completed),
    )
    .content(payload.content)
    .locations(payload.locations)
    .raw_input(payload.raw_input)
    .raw_output(payload.raw_output);
    Ok(SessionUpdate::Acp(Box::new(
        acp_transport::protocol::SessionNotification::new(
            session,
            acp_transport::protocol::SessionUpdate::ToolCall(call),
        ),
    )))
}

fn incomplete_tool(
    body: &FragmentBody,
    assembled: &Assembled,
    tool_id: &str,
    template: &SessionUpdate,
) -> Result<SessionUpdate, &'static str> {
    let session = session_id_of(template).ok_or("tool fragment has no session")?;
    let message = if assembled.missing.is_empty() {
        "Tool output incomplete".to_owned()
    } else {
        format!("Tool output incomplete; {}", assembled.missing)
    };
    let call = acp_transport::protocol::ToolCall::new(
        acp_transport::protocol::ToolCallId::new(tool_id.to_owned()),
        body.tool_title.clone().unwrap_or_else(|| "Tool".to_owned()),
    )
    .kind(
        body.tool_kind
            .clone()
            .unwrap_or(acp_transport::protocol::ToolKind::Other),
    )
    .status(acp_transport::protocol::ToolCallStatus::Pending)
    .content(vec![acp_transport::protocol::ToolCallContent::from(
        acp_transport::protocol::ContentBlock::Text(acp_transport::protocol::TextContent::new(
            message,
        )),
    )]);
    Ok(SessionUpdate::Acp(Box::new(
        acp_transport::protocol::SessionNotification::new(
            session,
            acp_transport::protocol::SessionUpdate::ToolCall(call),
        ),
    )))
}

fn stable_id(
    source: &HistorySourceKey,
    entity: Option<&HistoryEntity>,
    update: &SessionUpdate,
) -> StableId {
    let derived = entity_of(update);
    match entity.or(derived.as_ref()) {
        Some(HistoryEntity::Tool { tool_call_id }) => StableId::Tool(tool_call_id.clone()),
        Some(HistoryEntity::Message {
            event_seq,
            item_ordinal,
        }) => StableId::Message {
            event_seq: *event_seq,
            item_ordinal: *item_ordinal,
        },
        Some(HistoryEntity::UserMessage { message_id }) => StableId::User {
            message_id: message_id.clone(),
            record: source.record,
            part: source.part,
        },
        None => StableId::Source {
            record: source.record,
            part: source.part,
        },
    }
}

fn entity_of(update: &SessionUpdate) -> Option<HistoryEntity> {
    let SessionUpdate::Acp(notification) = update else {
        return None;
    };
    match &notification.update {
        acp_transport::protocol::SessionUpdate::ToolCall(call) => Some(HistoryEntity::Tool {
            tool_call_id: call.tool_call_id.0.to_string(),
        }),
        acp_transport::protocol::SessionUpdate::ToolCallUpdate(call) => Some(HistoryEntity::Tool {
            tool_call_id: call.tool_call_id.0.to_string(),
        }),
        acp_transport::protocol::SessionUpdate::UserMessageChunk(chunk) => chunk
            .meta
            .as_ref()
            .and_then(|meta| meta.get("messageId"))
            .and_then(|value| value.as_str())
            .map(|message_id| HistoryEntity::UserMessage {
                message_id: message_id.to_owned(),
            }),
        _ => None,
    }
}

fn tool_entity(entity: &Option<HistoryEntity>) -> Option<String> {
    match entity {
        Some(HistoryEntity::Tool { tool_call_id }) => Some(tool_call_id.clone()),
        _ => None,
    }
}

fn tool_id(update: &SessionUpdate) -> Option<String> {
    tool_entity(&entity_of(update))
}

fn sampling_identity(update: &SessionUpdate) -> Option<(String, u32)> {
    let SessionUpdate::Acp(notification) = update else {
        return None;
    };
    let meta = notification.meta.as_ref()?;
    let request_id = meta.get("samplingRequestId")?.as_str()?.to_owned();
    let attempt = u32::try_from(meta.get("samplingAttempt")?.as_u64()?).ok()?;
    Some((request_id, attempt))
}

fn chunk_text(update: &SessionUpdate) -> Result<String, &'static str> {
    let SessionUpdate::Acp(notification) = update else {
        return Err("history fragment is not text");
    };
    let chunk = match &notification.update {
        acp_transport::protocol::SessionUpdate::UserMessageChunk(chunk)
        | acp_transport::protocol::SessionUpdate::AgentMessageChunk(chunk)
        | acp_transport::protocol::SessionUpdate::AgentThoughtChunk(chunk) => chunk,
        _ => return Err("history fragment is not text"),
    };
    match &chunk.content {
        acp_transport::protocol::ContentBlock::Text(text) => Ok(text.text.clone()),
        _ => Err("history fragment is not text"),
    }
}

fn set_chunk_text(update: &mut SessionUpdate, text: String) -> Result<(), &'static str> {
    let SessionUpdate::Acp(notification) = update else {
        return Err("history fragment is not text");
    };
    let chunk = match &mut notification.update {
        acp_transport::protocol::SessionUpdate::UserMessageChunk(chunk)
        | acp_transport::protocol::SessionUpdate::AgentMessageChunk(chunk)
        | acp_transport::protocol::SessionUpdate::AgentThoughtChunk(chunk) => chunk,
        _ => return Err("history fragment is not text"),
    };
    match &mut chunk.content {
        acp_transport::protocol::ContentBlock::Text(body) => {
            body.text = text;
            Ok(())
        }
        _ => Err("history fragment is not text"),
    }
}

fn response_total(update: &SessionUpdate) -> Result<u64, &'static str> {
    let SessionUpdate::Acp(notification) = update else {
        return Err("history fragment has no length");
    };
    notification
        .meta
        .as_ref()
        .and_then(|meta| meta.get("grow/historyResponseTotalBytes"))
        .and_then(|value| value.as_u64())
        .ok_or("history fragment has no length")
}

fn session_id_of(update: &SessionUpdate) -> Option<acp_transport::protocol::SessionId> {
    match update {
        SessionUpdate::Acp(notification) => Some(notification.session_id.clone()),
        SessionUpdate::Grow(notification) => Some(notification.session_id.clone()),
        // Shell keeps `ResponseReplayProjection` behind its own `test`/`test-support`
        // cfg, so its visibility here follows dev-dependency feature unification and
        // cannot be expressed with this crate's cfg; test-only projections map to None.
        #[allow(unreachable_patterns)] // Only the test-only projection lands here.
        _ => None,
    }
}

fn json_string_bytes(text: &str) -> Result<Vec<u8>, &'static str> {
    let quoted = serde_json::to_string(text).map_err(|_| "history text is not encodable")?;
    let bytes = quoted.as_bytes();
    if bytes.len() < 2 {
        return Err("history text is not encodable");
    }
    Ok(bytes[1..bytes.len() - 1].to_vec())
}

/// Entry-id rewrite for one reprojection. Rows installed by an earlier page
/// keep their ids so anchors, selection, and fold state survive. A user
/// entity's identity is its `messageId`: an earlier page may move its first
/// visible chunk to an earlier source, and the ids follow the entity. A user
/// entity the live tail already echoed adopts that echo's id (display order
/// for coalesced segments); `replace_history_prefix` consumes the tail copy
/// in the same merge, so the id stays unique and the echo's UI state moves to
/// the canonical history position.
fn retarget_map(
    groups: &[(StableId, Vec<EntryId>)],
    previous: &HashMap<StableId, Vec<EntryId>>,
    scrollback: &ScrollbackState,
    prefix_len: usize,
) -> HashMap<EntryId, EntryId> {
    let live = (prefix_len..scrollback.len())
        .filter_map(|index| scrollback.entry(index).map(|entry| entry.id))
        .collect::<HashSet<_>>();
    let installed_user = previous
        .iter()
        .filter_map(|(stable, ids)| match stable {
            StableId::User { message_id, .. } => Some((message_id.as_str(), ids)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut map = HashMap::new();
    for (stable, new_ids) in groups {
        if let StableId::User { message_id, .. } = stable {
            if let Some(old_ids) = installed_user.get(message_id.as_str()) {
                for (new_id, old_id) in new_ids.iter().zip(old_ids.iter()) {
                    if new_id != old_id && !live.contains(old_id) {
                        map.insert(*new_id, *old_id);
                    }
                }
                continue;
            }
            let echoed = (prefix_len..scrollback.len())
                .filter_map(|index| scrollback.entry(index))
                .filter(|entry| {
                    matches!(
                        &entry.block,
                        RenderBlock::UserPrompt(block)
                            if block.message_id.as_deref() == Some(message_id.as_str())
                    )
                })
                .map(|entry| entry.id);
            for (new_id, old_id) in new_ids.iter().zip(echoed) {
                map.insert(*new_id, old_id);
            }
            continue;
        }
        let Some(old_ids) = previous.get(stable) else {
            continue;
        };
        for (new_id, old_id) in new_ids.iter().zip(old_ids.iter()) {
            if new_id != old_id && !live.contains(old_id) {
                map.insert(*new_id, *old_id);
            }
        }
    }
    map
}

#[cfg(test)]
mod history_live_overlap_tests;
