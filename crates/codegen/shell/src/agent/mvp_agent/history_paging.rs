//! Fixed history cut, `grow/read_history_page`, and `grow/read_history_timeline`.
//!
//! Wire, camelCase except the query tag:
//! ```text
//! ReadHistoryPage { sessionId, cut, query }
//! query: { kind: latest }
//!      | { kind: before, cursor }
//!      | { kind: after, cursor }
//!      | { kind: around, target }
//! HistoryPageResponse { sessionId, cut, rows, range, olderCursor?, newerCursor?, targetSource? }
//!
//! ReadHistoryTimeline { sessionId, cut, limit, query }
//! query: { kind: newest }
//!      | { kind: before, cursor }
//!      | { kind: around_ordinal, ordinal }
//! HistoryTimelineResponse { sessionId, cut, totalEntries, entries, olderCursor? }
//! entry: { ordinal, source, messageId?, displaySegment?, preview, target }
//! ```
//! Page cursors, the timeline before cursor, and around targets use different
//! roles. A token signed for one role is rejected by the others.

use std::{io, sync::Arc};

use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};

use super::MvpAgent;
use crate::session::history::{
    HistoryRow, HistorySnapshot, HistorySourceKey, HistoryTimelineEntry, IndexedPages,
    PROJECTOR_VERSION, PagePosition, PreparedPages,
};
use crate::session::storage::{
    HistoryPageQuery, HistoryPageResponse, HistoryTimelineQuery, HistoryTimelineResponse,
    HistoryTimelineTarget, ReadHistoryPage, ReadHistoryTimeline, SessionUpdate,
};

#[cfg(test)]
static PREPARE_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(test)]
static PREPARE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn note_prepare() {
    #[cfg(test)]
    PREPARE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CursorRole {
    Before,
    After,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CursorWire {
    role: CursorRole,
    record: u64,
    part: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<usize>,
    proof: String,
}

#[derive(Clone, Copy)]
enum ResolvedQuery {
    Latest,
    Before(PagePosition),
    After(PagePosition),
    Around(HistorySourceKey),
}

#[derive(Debug, Clone, Copy)]
enum SignedToken {
    Before(PagePosition),
    After(PagePosition),
    TimelineBefore(u64),
    Target {
        source: HistorySourceKey,
        display_segment: Option<u32>,
        ordinal: u64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TargetWire {
    role: String,
    record: u64,
    part: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display_segment: Option<u32>,
    ordinal: u64,
    proof: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimelineCursorWire {
    role: String,
    ordinal: u64,
    proof: String,
}

#[derive(Clone)]
enum CutSource {
    Indexed(Arc<IndexedPages>),
    Prepared(Arc<PreparedPages>),
}

#[derive(Clone)]
pub(super) struct HistoryPageCut {
    id: uuid::Uuid,
    session: acp::SessionId,
    bytes: u64,
    hash: String,
    records: u64,
    branch: Option<chat_state::EventSeq>,
    source: CutSource,
    ownership: Arc<crate::session::storage::DescendantOwnership>,
    indexed: Option<crate::session::storage::jsonl::IndexedHistory>,
}

pub(super) struct CapturedHistoryCut {
    pub coverage: crate::session::storage::SubagentProjectionState,
    pub cut: HistoryPageCut,
}

impl HistoryPageCut {
    /// One prepare of an already validated snapshot. Later pages reuse it.
    pub(super) fn capture(
        session: &acp::SessionId,
        history: &HistorySnapshot,
        timeline: &chat_state::Timeline,
    ) -> io::Result<CapturedHistoryCut> {
        note_prepare();
        let (coverage, source) = if history.indexed.is_some() {
            let prepared = history.prepare_indexed(session, timeline)?;
            let coverage = coverage_from(session, &prepared.updates);
            let pages = history.indexed_pages(prepared)?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "indexed history cut is missing")
            })?;
            (coverage, CutSource::Indexed(Arc::new(pages)))
        } else {
            let prepared = history.prepare(session, timeline, None)?;
            let coverage = coverage_from(session, &prepared.updates);
            (
                coverage,
                CutSource::Prepared(Arc::new(prepared.into_pages()?)),
            )
        };
        Ok(CapturedHistoryCut {
            coverage,
            cut: Self {
                id: uuid::Uuid::new_v4(),
                session: session.clone(),
                bytes: history.bytes,
                hash: history.prefix_hash.clone(),
                records: history.record_count(),
                branch: timeline.history_branch(),
                source,
                ownership: Arc::new(crate::session::storage::DescendantOwnership::from_validated(
                    session.0.as_ref(), timeline, history,
                )),
                indexed: history.indexed.clone(),
            },
        })
    }

    pub(super) fn token(&self) -> String {
        self.proof(None)
    }

    #[cfg(test)]
    fn set_branch_for_test(&mut self, branch: Option<chat_state::EventSeq>) {
        self.branch = branch;
    }

    pub(super) fn branch_matches(&self, branch: Option<chat_state::EventSeq>) -> bool {
        self.branch == branch
    }

    fn cursor(&self, role: CursorRole, position: PagePosition) -> String {
        serde_json::to_string(&CursorWire {
            role,
            record: position.record,
            part: position.part,
            offset: position.offset,
            proof: self.proof(Some((role, position))),
        })
        .expect("history cursor serializes")
    }

    fn timeline_cursor(&self, ordinal: u64) -> String {
        serde_json::to_string(&TimelineCursorWire {
            role: "timeline_before".to_owned(),
            ordinal,
            proof: self.proof_timeline_before(ordinal),
        })
        .expect("history timeline cursor serializes")
    }

    fn target_token(&self, entry: &HistoryTimelineEntry) -> String {
        self.sign_target(entry.source, entry.display_segment, entry.ordinal)
    }

    fn sign_target(
        &self,
        source: HistorySourceKey,
        display_segment: Option<u32>,
        ordinal: u64,
    ) -> String {
        serde_json::to_string(&TargetWire {
            role: "target".to_owned(),
            record: source.record,
            part: source.part,
            display_segment,
            ordinal,
            proof: self.proof_target(source, display_segment, ordinal),
        })
        .expect("history target serializes")
    }

    fn decode_token(&self, raw: &str) -> Result<SignedToken, ()> {
        let value: serde_json::Value = serde_json::from_str(raw).map_err(|_| ())?;
        let role = value
            .get("role")
            .and_then(serde_json::Value::as_str)
            .ok_or(())?;
        match role {
            "before" | "after" => {
                let wire: CursorWire = serde_json::from_value(value).map_err(|_| ())?;
                let position = PagePosition {
                    record: wire.record,
                    part: wire.part,
                    offset: wire.offset,
                };
                if self.proof(Some((wire.role, position))) != wire.proof {
                    return Err(());
                }
                Ok(match wire.role {
                    CursorRole::Before => SignedToken::Before(position),
                    CursorRole::After => SignedToken::After(position),
                })
            }
            "timeline_before" => {
                let wire: TimelineCursorWire = serde_json::from_value(value).map_err(|_| ())?;
                if wire.role != "timeline_before"
                    || self.proof_timeline_before(wire.ordinal) != wire.proof
                {
                    return Err(());
                }
                Ok(SignedToken::TimelineBefore(wire.ordinal))
            }
            "target" => {
                let wire: TargetWire = serde_json::from_value(value).map_err(|_| ())?;
                let source = HistorySourceKey {
                    record: wire.record,
                    part: wire.part,
                };
                if wire.role != "target"
                    || self.proof_target(source, wire.display_segment, wire.ordinal) != wire.proof
                {
                    return Err(());
                }
                Ok(SignedToken::Target {
                    source,
                    display_segment: wire.display_segment,
                    ordinal: wire.ordinal,
                })
            }
            _ => Err(()),
        }
    }

    fn proof(&self, cursor: Option<(CursorRole, PagePosition)>) -> String {
        self.digest(|hasher| match cursor {
            None => {
                hasher.update(b"cut");
            }
            Some((role, position)) => {
                hasher.update(match role {
                    CursorRole::Before => b"before",
                    CursorRole::After => b"after",
                });
                hasher.update(&position.record.to_le_bytes());
                hasher.update(&position.part.to_le_bytes());
                if let Some(offset) = position.offset {
                    hasher.update(&[1]);
                    hasher.update(&(offset as u64).to_le_bytes());
                } else {
                    hasher.update(&[0]);
                }
            }
        })
    }

    fn proof_timeline_before(&self, ordinal: u64) -> String {
        self.digest(|hasher| {
            hasher.update(b"timeline-before");
            hasher.update(&ordinal.to_le_bytes());
        })
    }

    fn proof_target(
        &self,
        source: HistorySourceKey,
        display_segment: Option<u32>,
        ordinal: u64,
    ) -> String {
        self.digest(|hasher| {
            hasher.update(b"target");
            hasher.update(&source.record.to_le_bytes());
            hasher.update(&source.part.to_le_bytes());
            match display_segment {
                Some(segment) => {
                    hasher.update(&[1]);
                    hasher.update(&segment.to_le_bytes());
                }
                None => {
                    hasher.update(&[0]);
                }
            }
            hasher.update(&ordinal.to_le_bytes());
        })
    }

    fn digest(&self, body: impl FnOnce(&mut blake3::Hasher)) -> String {
        let key = blake3::derive_key("Grow session history page cut", self.id.as_bytes());
        let mut hasher = blake3::Hasher::new_keyed(&key);
        hasher.update(self.session.0.as_bytes());
        hasher.update(&[PROJECTOR_VERSION]);
        hasher.update(&self.bytes.to_le_bytes());
        hasher.update(self.hash.as_bytes());
        hasher.update(&self.records.to_le_bytes());
        match self.branch {
            Some(seq) => {
                hasher.update(&[1]);
                hasher.update(&seq.get().to_le_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
        body(&mut hasher);
        hasher.finalize().to_hex().to_string()
    }

    fn read(
        &self,
        session: &acp::SessionId,
        query: ResolvedQuery,
    ) -> io::Result<crate::session::history::PageWindow> {
        match (&self.source, query) {
            (CutSource::Indexed(pages), ResolvedQuery::Latest) => {
                pages.read_before(session, PagePosition::newest())
            }
            (CutSource::Indexed(pages), ResolvedQuery::Before(position)) => {
                pages.read_before(session, position)
            }
            (CutSource::Indexed(pages), ResolvedQuery::After(position)) => {
                pages.read_after(session, position)
            }
            (CutSource::Prepared(pages), ResolvedQuery::Latest) => {
                pages.read_before(PagePosition::newest())
            }
            (CutSource::Prepared(pages), ResolvedQuery::Before(position)) => {
                pages.read_before(position)
            }
            (CutSource::Prepared(pages), ResolvedQuery::After(position)) => {
                pages.read_after(position)
            }
            (CutSource::Indexed(pages), ResolvedQuery::Around(source)) => {
                pages.read_around(session, source)
            }
            (CutSource::Prepared(pages), ResolvedQuery::Around(source)) => {
                pages.read_around(source)
            }
        }
    }

    fn read_timeline(
        &self,
        query: crate::session::history::TimelineQuery,
        limit: usize,
    ) -> io::Result<crate::session::history::HistoryTimelinePage> {
        match &self.source {
            CutSource::Indexed(pages) => match query {
                crate::session::history::TimelineQuery::Newest => pages.timeline_newest(limit),
                crate::session::history::TimelineQuery::Before(ordinal) => {
                    pages.timeline_before(ordinal, limit)
                }
                crate::session::history::TimelineQuery::Around(ordinal) => {
                    pages.timeline_around_ordinal(ordinal, limit)
                }
            },
            CutSource::Prepared(pages) => match query {
                crate::session::history::TimelineQuery::Newest => pages.timeline_newest(limit),
                crate::session::history::TimelineQuery::Before(ordinal) => {
                    pages.timeline_before(ordinal, limit)
                }
                crate::session::history::TimelineQuery::Around(ordinal) => {
                    pages.timeline_around_ordinal(ordinal, limit)
                }
            },
        }
    }

    fn response(
        &self,
        session_id: acp::SessionId,
        window: crate::session::history::PageWindow,
    ) -> io::Result<HistoryPageResponse> {
        let descendant_ownership = self.ownership.for_page(&window.rows, self.indexed.as_ref())?;
        Ok(HistoryPageResponse {
            session_id,
            cut: self.token(),
            rows: window.rows,
            range: window.range,
            older_cursor: window
                .older_position
                .map(|position| self.cursor(CursorRole::Before, position)),
            newer_cursor: window
                .newer_position
                .map(|position| self.cursor(CursorRole::After, position)),
            target_source: window.target_source,
            descendant_ownership,
        })
    }
}

fn coverage_from(
    session: &acp::SessionId,
    updates: &[SessionUpdate],
) -> crate::session::storage::SubagentProjectionState {
    let mut coverage = crate::session::storage::SubagentProjectionState::default();
    for update in updates {
        let SessionUpdate::Grow(notification) = update else {
            continue;
        };
        use crate::extensions::notification::SessionUpdate as Update;
        match &notification.update {
            Update::SubagentSpawned {
                subagent_id,
                child_session_id,
                parent_session_id,
                ..
            } => {
                coverage.spawned.insert(subagent_id.clone());
                if parent_session_id == session.0.as_ref() {
                    coverage
                        .replayed_children
                        .insert(child_session_id.clone(), subagent_id.clone());
                }
            }
            Update::SubagentProgress {
                subagent_id,
                child_session_id,
                parent_session_id,
                ..
            } => {
                if parent_session_id == session.0.as_ref() {
                    coverage
                        .replayed_children
                        .insert(child_session_id.clone(), subagent_id.clone());
                }
            }
            Update::SubagentFinished {
                subagent_id,
                child_session_id,
                ..
            } => {
                coverage.finished.insert(subagent_id.clone());
                coverage
                    .replayed_children
                    .insert(child_session_id.clone(), subagent_id.clone());
            }
            _ => {}
        }
    }
    coverage
}

fn history_query_error(error: &io::Error) -> Option<acp::Error> {
    if error.kind() != io::ErrorKind::InvalidData {
        return None;
    }
    match error.to_string().as_str() {
        "history around target is not visible"
        | "history timeline ordinal is outside the published cut" => {
            Some(acp::Error::invalid_params().data(error.to_string()))
        }
        _ => None,
    }
}

impl MvpAgent {
    fn installed_cut(
        &self,
        session_id: &acp::SessionId,
        token: &str,
    ) -> Result<HistoryPageCut, acp::Error> {
        self.resident_resources
            .borrow()
            .get(session_id)
            .and_then(|resources| resources.history_page_cut.clone())
            .filter(|cut| cut.token() == token)
            .ok_or_else(|| acp::Error::invalid_params().data("history cut is stale or tampered"))
    }

    /// Cursor-tail reload keeps the installed cut when this actor and branch
    /// still own it. A full page load passes the replacement cut instead.
    pub(super) async fn apply_loaded_history_cut(
        &self,
        session_id: &acp::SessionId,
        loaded: Option<HistoryPageCut>,
        cursor_tail: bool,
    ) {
        if cursor_tail && loaded.is_none() && self.cursor_tail_keeps_installed_cut(session_id).await
        {
            return;
        }
        self.resident_resources
            .borrow_mut()
            .entry(session_id.clone())
            .or_default()
            .history_page_cut = loaded;
    }

    async fn cursor_tail_keeps_installed_cut(&self, session_id: &acp::SessionId) -> bool {
        let (actor, chat) = {
            let sessions = self.sessions.borrow();
            let Some(handle) = sessions.get(session_id) else {
                return false;
            };
            if handle.cmd_tx.is_closed()
                || handle.chat_state_handle.is_closed()
                || handle.chat_state_handle.pending_timeline_commit().is_some()
            {
                return false;
            }
            (handle.cmd_tx.clone(), handle.chat_state_handle.clone())
        };
        let Some(branch) = chat.history_branch().await else {
            return false;
        };
        let sessions = self.sessions.borrow();
        let Some(handle) = sessions.get(session_id) else {
            return false;
        };
        if handle.cmd_tx.is_closed()
            || !handle.cmd_tx.same_channel(&actor)
            || handle.chat_state_handle.is_closed()
            || handle.chat_state_handle.pending_timeline_commit().is_some()
        {
            return false;
        }
        drop(sessions);
        self.resident_resources
            .borrow()
            .get(session_id)
            .and_then(|resources| resources.history_page_cut.as_ref())
            .is_some_and(|cut| cut.branch_matches(branch))
    }

    async fn authorized_read<T: Send + 'static>(
        &self,
        session_id: &acp::SessionId,
        token: &str,
        branch: Option<chat_state::EventSeq>,
        read: impl FnOnce() -> io::Result<T> + Send + 'static,
    ) -> Result<T, acp::Error> {
        let stale_cut = || acp::Error::invalid_params().data("history cut is stale or tampered");
        let branch_changed = || acp::Error::invalid_params().data("history branch changed");
        let actor_changed = || acp::Error::invalid_params().data("history session actor changed");
        let actor = self
            .sessions
            .borrow()
            .get(session_id)
            .ok_or_else(actor_changed)?
            .cmd_tx
            .clone();
        let session_id = session_id.clone();
        let live_chat = || {
            let sessions = self.sessions.borrow();
            let handle = sessions.get(&session_id).ok_or_else(actor_changed)?;
            if handle.cmd_tx.is_closed()
                || !handle.cmd_tx.same_channel(&actor)
                || handle.chat_state_handle.is_closed()
                || handle.chat_state_handle.pending_timeline_commit().is_some()
            {
                return Err(actor_changed());
            }
            Ok(handle.chat_state_handle.clone())
        };
        let confirm_branch = |current: Option<Option<chat_state::EventSeq>>| match current {
            Some(current) if current == branch => Ok(()),
            Some(_) => Err(branch_changed()),
            None => Err(actor_changed()),
        };
        let chat = live_chat()?;
        let before = chat.history_branch().await;
        live_chat()?;
        confirm_branch(before)?;
        let value = tokio::task::spawn_blocking(read)
            .await
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))?
            .map_err(|error| {
                history_query_error(&error)
                    .unwrap_or_else(|| crate::session::persistence::io_error_to_acp(&error))
            })?;
        let chat = live_chat()?;
        let after = chat.history_branch().await;
        live_chat()?;
        confirm_branch(after)?;
        let still_installed = self
            .resident_resources
            .borrow()
            .get(&session_id)
            .and_then(|resources| resources.history_page_cut.as_ref())
            .is_some_and(|current| current.token() == token);
        if !still_installed {
            return Err(stale_cut());
        }
        Ok(value)
    }

    pub(super) async fn read_history_page(
        &self,
        args: &acp::ExtRequest,
    ) -> crate::extensions::ExtResult {
        let mut timer = crate::instrumentation_timer!("session.history_page.read");
        let request: ReadHistoryPage = crate::extensions::parse_params(args)?;
        let tampered =
            || acp::Error::invalid_params().data("history page cursor is stale or tampered");
        let wrong_direction =
            || acp::Error::invalid_params().data("history page cursor has the wrong direction");
        let wrong_role = || acp::Error::invalid_params().data("history cursor has the wrong role");
        let cut = self.installed_cut(&request.session_id, &request.cut)?;
        let query = match &request.query {
            HistoryPageQuery::Latest => ResolvedQuery::Latest,
            HistoryPageQuery::Before { cursor } => match cut.decode_token(cursor) {
                Ok(SignedToken::Before(position)) => ResolvedQuery::Before(position),
                Ok(SignedToken::After(_)) => return Err(wrong_direction()),
                Ok(_) => return Err(wrong_role()),
                Err(()) => return Err(tampered()),
            },
            HistoryPageQuery::After { cursor } => match cut.decode_token(cursor) {
                Ok(SignedToken::After(position)) => ResolvedQuery::After(position),
                Ok(SignedToken::Before(_)) => return Err(wrong_direction()),
                Ok(_) => return Err(wrong_role()),
                Err(()) => return Err(tampered()),
            },
            HistoryPageQuery::Around { target } => match cut.decode_token(target) {
                Ok(SignedToken::Target { source, .. }) => ResolvedQuery::Around(source),
                Ok(_) => return Err(wrong_role()),
                Err(()) => return Err(tampered()),
            },
        };
        let session = request.session_id.clone();
        let branch = cut.branch;
        let read_cut = cut.clone();
        let response = self
            .authorized_read(&request.session_id, &request.cut, branch, move || {
                let window = read_cut.read(&session, query)?;
                read_cut.response(session, window)
            })
            .await?;
        timer.with_field("rows", response.rows.len());
        crate::extensions::to_raw_response(&response)
    }

    pub(super) async fn read_history_timeline(
        &self,
        args: &acp::ExtRequest,
    ) -> crate::extensions::ExtResult {
        let mut timer = crate::instrumentation_timer!("session.history_timeline.read");
        let request: ReadHistoryTimeline = crate::extensions::parse_params(args)?;
        let tampered =
            || acp::Error::invalid_params().data("history timeline cursor is stale or tampered");
        let wrong_role = || acp::Error::invalid_params().data("history cursor has the wrong role");
        let cut = self.installed_cut(&request.session_id, &request.cut)?;
        let query = match &request.query {
            HistoryTimelineQuery::Newest => crate::session::history::TimelineQuery::Newest,
            HistoryTimelineQuery::Before { cursor } => match cut.decode_token(cursor) {
                Ok(SignedToken::TimelineBefore(ordinal)) => {
                    crate::session::history::TimelineQuery::Before(ordinal)
                }
                Ok(_) => return Err(wrong_role()),
                Err(()) => return Err(tampered()),
            },
            HistoryTimelineQuery::AroundOrdinal { ordinal } => {
                crate::session::history::TimelineQuery::Around(*ordinal)
            }
        };
        let branch = cut.branch;
        let read_cut = cut.clone();
        let limit = request.limit;
        let page = self
            .authorized_read(&request.session_id, &request.cut, branch, move || {
                read_cut.read_timeline(query, limit)
            })
            .await?;
        let response = HistoryTimelineResponse {
            session_id: request.session_id,
            cut: cut.token(),
            total_entries: page.total_entries,
            entries: page
                .entries
                .iter()
                .map(|entry| HistoryTimelineTarget {
                    target: cut.target_token(entry),
                    entry: entry.clone(),
                })
                .collect(),
            older_cursor: page
                .older_ordinal
                .map(|ordinal| cut.timeline_cursor(ordinal)),
        };
        timer.with_field("entries", response.entries.len());
        crate::extensions::to_raw_response(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::history::{HistoryContent, HistoryFact};
    use crate::session::storage::{ContainedDirectory, TIMELINE_FILE, jsonl::JsonlStorageAdapter};
    use std::io::Write;
    use std::path::Path;

    fn texts(rows: &[HistoryRow]) -> Vec<String> {
        rows.iter()
            .filter_map(|row| {
                let SessionUpdate::Acp(notification) = &row.update else {
                    return None;
                };
                let acp::SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
                    return None;
                };
                let acp::ContentBlock::Text(text) = &chunk.content else {
                    return None;
                };
                Some(text.text.clone())
            })
            .collect()
    }

    fn journal(
        count: usize,
    ) -> (
        tempfile::TempDir,
        acp::SessionId,
        chat_state::Timeline,
        crate::session::history::HistorySnapshot,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let directory =
            ContainedDirectory::open(temp.path(), Path::new(""), "fixture", false).unwrap();
        let session = acp::SessionId::new("paged");
        let mut timeline = chat_state::Timeline::default();
        timeline
            .append(
                sampling_types::ConversationItem::system("fixed context"),
                chat_state::MessageCause::Seed,
            )
            .unwrap();
        let facts: Vec<_> = (0..count)
            .map(|n| HistoryFact {
                meta: None,
                content: HistoryContent::Assistant(acp::ContentChunk::new(
                    acp::ContentBlock::Text(acp::TextContent::new(format!("history-{n}"))),
                )),
            })
            .collect();
        let original = JsonlStorageAdapter::encode_journal(timeline.events(), &facts).unwrap();
        std::fs::write(temp.path().join(TIMELINE_FILE), &original).unwrap();
        let history = JsonlStorageAdapter::read_journal_from_directory(&directory)
            .unwrap()
            .history;
        assert!(
            history.indexed.is_none(),
            "fixture must exercise the prepared projection"
        );
        history.validate(&timeline).unwrap();
        (temp, session, timeline, history)
    }

    fn chat_state() -> chat_state::ChatStateHandle {
        let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
        chat_state::ChatStateActor::spawn(
            vec![],
            sampling_types::SamplingConfig {
                base_url: "http://localhost".into(),
                model: "model".into(),
                output_limit: None,
                temperature: None,
                top_p: None,
                api_backend: Default::default(),
                extra_headers: Default::default(),
                env_http_headers: Default::default(),
                query_params: Default::default(),
                context_window: std::num::NonZeroU64::new(128_000).unwrap(),
                reasoning_effort: None,
                stream_tool_calls: None,
            },
            Box::new(chat_state::NullTimelinePersistence),
            event_tx,
            tokio_util::sync::CancellationToken::new(),
        )
    }

    struct Installed {
        agent: super::super::MvpAgent,
        session: acp::SessionId,
        cut: String,
        _commands: tokio::sync::mpsc::UnboundedReceiver<crate::session::SessionCommand>,
    }

    fn install(cut: HistoryPageCut) -> Installed {
        let session = cut.session.clone();
        let token = cut.token();
        let mut agent = super::super::tests::build_minimal_agent_for_tests();
        let (mut handle, commands) =
            super::super::tests::make_test_handle_with_receiver("test-model", None);
        handle.chat_state_handle = chat_state();
        handle.info.id = session.clone();
        agent.sessions.borrow_mut().insert(session.clone(), handle);
        agent
            .resident_resources
            .borrow_mut()
            .entry(session.clone())
            .or_default()
            .history_page_cut = Some(cut);
        Installed {
            agent,
            session,
            cut: token,
            _commands: commands,
        }
    }

    async fn page(
        agent: &super::super::MvpAgent,
        session: &acp::SessionId,
        cut: &str,
        query: serde_json::Value,
    ) -> Result<serde_json::Value, acp::Error> {
        let request = acp::ExtRequest::new(
            "grow/read_history_page",
            serde_json::value::to_raw_value(&serde_json::json!({
                "sessionId": session,
                "cut": cut,
                "query": query,
            }))
            .unwrap()
            .into(),
        );
        agent
            .read_history_page(&request)
            .await
            .map(|response| serde_json::from_str(response.0.get()).unwrap())
    }

    #[test]
    fn extension_pages_one_cut_without_preparing_again() {
        let _gate = PREPARE_GATE.lock().unwrap();
        PREPARE_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (temp, session, timeline, history) = journal(270);
                let captured = HistoryPageCut::capture(&session, &history, &timeline).unwrap();
                assert_eq!(PREPARE_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
                assert!(captured.coverage.spawned.is_empty());
                let path = temp.path().join(TIMELINE_FILE);
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap();
                file.write_all(b"appended-after-cut\n").unwrap();
                let mut installed = install(captured.cut);
                let latest = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                let again = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                assert_eq!(again, latest);
                assert_eq!(latest["cut"], installed.cut);
                assert_eq!(latest["sessionId"], "paged");
                assert!(latest.get("olderCursor").is_some());
                assert!(latest.get("newerCursor").is_none());
                assert!(latest.get("targetSource").is_none());
                let older = latest["olderCursor"].as_str().unwrap();
                let before = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "before", "cursor": older}),
                )
                .await
                .unwrap();
                assert_eq!(before["cut"], installed.cut);
                let before_rows: Vec<HistoryRow> =
                    serde_json::from_value(before["rows"].clone()).unwrap();
                let latest_rows: Vec<HistoryRow> =
                    serde_json::from_value(latest["rows"].clone()).unwrap();
                assert!(!before_rows.is_empty() && !latest_rows.is_empty());
                assert!(before_rows.last().unwrap().source < latest_rows.first().unwrap().source);
                let newer = before["newerCursor"]
                    .as_str()
                    .expect("older page can read forward");
                let after = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "after", "cursor": newer}),
                )
                .await
                .unwrap();
                let after_rows: Vec<HistoryRow> =
                    serde_json::from_value(after["rows"].clone()).unwrap();
                assert_eq!(
                    after_rows.first().unwrap().source,
                    latest_rows.first().unwrap().source
                );
                let seen = texts(&latest_rows)
                    .into_iter()
                    .chain(texts(&before_rows))
                    .chain(texts(&after_rows));
                assert!(
                    seen.into_iter()
                        .all(|text| text != "history-appended"
                            && !text.contains("appended-after-cut"))
                );
                assert_eq!(PREPARE_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
                assert!(installed._commands.try_recv().is_err());
            });
    }

    #[test]
    fn empty_history_keeps_a_cut_with_no_older_cursor() {
        let _gate = PREPARE_GATE.lock().unwrap();
        PREPARE_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let session = acp::SessionId::new("empty");
                let timeline = chat_state::Timeline::default();
                let history = HistorySnapshot::default();
                let captured = HistoryPageCut::capture(&session, &history, &timeline).unwrap();
                let installed = install(captured.cut);
                let latest = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                assert_eq!(latest["rows"], serde_json::json!([]));
                assert!(latest["range"].is_null());
                assert!(latest.get("olderCursor").is_none());
                assert_eq!(latest["cut"], installed.cut);
                let retry = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                assert_eq!(retry, latest);
                assert_eq!(PREPARE_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
            });
    }

    #[test]
    fn rejects_tampered_cursor_wrong_direction_and_replaced_branch() {
        let _gate = PREPARE_GATE.lock().unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let session = acp::SessionId::new("guard");
                let timeline = chat_state::Timeline::default();
                let mut captured =
                    HistoryPageCut::capture(&session, &HistorySnapshot::default(), &timeline)
                        .unwrap();
                let installed = install(captured.cut.clone());
                let missing = page(
                    &installed.agent,
                    &installed.session,
                    "not-the-cut",
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    missing.data,
                    Some(serde_json::json!("history cut is stale or tampered"))
                );

                captured
                    .cut
                    .set_branch_for_test(Some(chat_state::EventSeq::new(7)));
                let token = captured.cut.token();
                installed
                    .agent
                    .resident_resources
                    .borrow_mut()
                    .get_mut(&session)
                    .unwrap()
                    .history_page_cut = Some(captured.cut);
                let replaced = page(
                    &installed.agent,
                    &session,
                    &token,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    replaced.data,
                    Some(serde_json::json!("history branch changed"))
                );

                let (_temp, session, timeline, history) = journal(270);
                let captured = HistoryPageCut::capture(&session, &history, &timeline).unwrap();
                let mut installed = install(captured.cut);
                let latest = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                let older = latest["olderCursor"].as_str().unwrap().to_owned();
                let wrong = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "after", "cursor": older}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    wrong.data,
                    Some(serde_json::json!(
                        "history page cursor has the wrong direction"
                    ))
                );
                let mut forged: serde_json::Value =
                    serde_json::from_str(latest["olderCursor"].as_str().unwrap()).unwrap();
                forged["proof"] = serde_json::json!("00");
                let tampered = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "before", "cursor": forged.to_string()}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    tampered.data,
                    Some(serde_json::json!(
                        "history page cursor is stale or tampered"
                    ))
                );

                drop(installed._commands);
                let closed = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    closed.data,
                    Some(serde_json::json!("history session actor changed"))
                );
            });
    }

    fn user_fact(text: &str, message_id: &str, segments: Option<&[&str]>) -> HistoryFact {
        let mut body = acp::TextContent::new(text);
        if let Some(segments) = segments {
            let mut meta = serde_json::Map::new();
            meta.insert(
                "combinedDisplayTexts".into(),
                serde_json::Value::Array(
                    segments
                        .iter()
                        .copied()
                        .map(serde_json::Value::from)
                        .collect(),
                ),
            );
            body.meta = Some(meta);
        }
        let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(body)).meta(
            serde_json::json!({"messageId": message_id})
                .as_object()
                .cloned(),
        );
        HistoryFact {
            meta: None,
            content: HistoryContent::User(chunk),
        }
    }

    fn journal_facts(
        session_name: &str,
        facts: Vec<HistoryFact>,
    ) -> (
        tempfile::TempDir,
        acp::SessionId,
        chat_state::Timeline,
        crate::session::history::HistorySnapshot,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let directory =
            ContainedDirectory::open(temp.path(), Path::new(""), "fixture", false).unwrap();
        let session = acp::SessionId::new(session_name);
        let mut timeline = chat_state::Timeline::default();
        timeline
            .append(
                sampling_types::ConversationItem::system("fixed context"),
                chat_state::MessageCause::Seed,
            )
            .unwrap();
        let original = JsonlStorageAdapter::encode_journal(timeline.events(), &facts).unwrap();
        std::fs::write(temp.path().join(TIMELINE_FILE), &original).unwrap();
        let history = JsonlStorageAdapter::read_journal_from_directory(&directory)
            .unwrap()
            .history;
        history.validate(&timeline).unwrap();
        (temp, session, timeline, history)
    }

    async fn timeline(
        agent: &super::super::MvpAgent,
        session: &acp::SessionId,
        cut: &str,
        limit: usize,
        query: serde_json::Value,
    ) -> Result<serde_json::Value, acp::Error> {
        let request = acp::ExtRequest::new(
            "grow/read_history_timeline",
            serde_json::value::to_raw_value(&serde_json::json!({
                "sessionId": session,
                "cut": cut,
                "limit": limit,
                "query": query,
            }))
            .unwrap()
            .into(),
        );
        agent
            .read_history_timeline(&request)
            .await
            .map(|response| serde_json::from_str(response.0.get()).unwrap())
    }

    fn user_text(row: &HistoryRow) -> Option<String> {
        let SessionUpdate::Acp(notification) = &row.update else {
            return None;
        };
        let acp::SessionUpdate::UserMessageChunk(chunk) = &notification.update else {
            return None;
        };
        let acp::ContentBlock::Text(text) = &chunk.content else {
            return None;
        };
        Some(text.text.clone())
    }

    fn rows_of(value: &serde_json::Value) -> Vec<HistoryRow> {
        serde_json::from_value(value["rows"].clone()).unwrap()
    }

    #[test]
    fn timeline_directory_keeps_distinct_targets_for_one_cut() {
        let _gate = PREPARE_GATE.lock().unwrap();
        PREPARE_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut facts: Vec<_> = (0..20)
                    .map(|n| user_fact(&format!("prompt-{n:02}"), &format!("id-{n}"), None))
                    .collect();
                facts[1] = user_fact("prompt-00", "id-dup", None);
                facts[2] = user_fact("joined", "id-combo", Some(&["alpha", "beta"]));
                let (temp, session, timeline_state, history) = journal_facts("catalog", facts);
                let captured =
                    HistoryPageCut::capture(&session, &history, &timeline_state).unwrap();
                let mut file = std::fs::OpenOptions::new()
                    .append(true)
                    .open(temp.path().join(TIMELINE_FILE))
                    .unwrap();
                file.write_all(b"appended-after-cut\n").unwrap();
                let mut installed = install(captured.cut);
                let newest = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    10_000,
                    serde_json::json!({"kind": "newest"}),
                )
                .await
                .unwrap();
                assert_eq!(newest["cut"], installed.cut);
                assert_eq!(newest["totalEntries"], 21);
                let entries = newest["entries"].as_array().unwrap();
                assert_eq!(entries.len(), 21);
                assert!(
                    entries
                        .iter()
                        .all(|entry| entry["preview"].as_str().unwrap().len() <= 160)
                );
                let same: Vec<_> = entries
                    .iter()
                    .filter(|entry| entry["preview"].as_str() == Some("prompt-00"))
                    .collect();
                assert_eq!(same.len(), 2);
                assert_ne!(same[0]["target"], same[1]["target"]);
                assert_ne!(same[0]["messageId"], same[1]["messageId"]);
                assert_ne!(same[0]["source"], same[1]["source"]);
                let combo: Vec<_> = entries
                    .iter()
                    .filter(|entry| entry["messageId"].as_str() == Some("id-combo"))
                    .cloned()
                    .collect();
                assert_eq!(combo.len(), 2);
                assert_eq!(combo[0]["source"], combo[1]["source"]);
                assert_eq!(combo[0]["displaySegment"], serde_json::json!(0));
                assert_eq!(combo[1]["displaySegment"], serde_json::json!(1));
                assert_ne!(combo[0]["target"], combo[1]["target"]);
                assert!(combo[0]["preview"] != combo[1]["preview"]);
                assert!(newest.get("olderCursor").is_none());
                let newest_page = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    4,
                    serde_json::json!({"kind": "newest"}),
                )
                .await
                .unwrap();
                let newest_ordinals: Vec<_> = newest_page["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["ordinal"].as_u64().unwrap())
                    .collect();
                assert_eq!(newest_ordinals, vec![17, 18, 19, 20]);
                let older = newest_page["olderCursor"].as_str().unwrap();
                let before = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    4,
                    serde_json::json!({"kind": "before", "cursor": older}),
                )
                .await
                .unwrap();
                let before_ordinals: Vec<_> = before["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| entry["ordinal"].as_u64().unwrap())
                    .collect();
                assert_eq!(before_ordinals, vec![13, 14, 15, 16]);
                let around = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    3,
                    serde_json::json!({"kind": "around_ordinal", "ordinal": 0}),
                )
                .await
                .unwrap();
                assert_eq!(around["entries"][0]["ordinal"], 0);
                assert!(around.get("olderCursor").is_none());
                let last = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    1,
                    serde_json::json!({"kind": "around_ordinal", "ordinal": 20}),
                )
                .await
                .unwrap();
                assert_eq!(last["entries"][0]["ordinal"], 20);
                let illegal = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    1,
                    serde_json::json!({"kind": "around_ordinal", "ordinal": 21}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    illegal.data,
                    Some(serde_json::json!(
                        "history timeline ordinal is outside the published cut"
                    ))
                );
                for jumped in [&combo[0], &combo[1]] {
                    let page = page(
                        &installed.agent,
                        &installed.session,
                        &installed.cut,
                        serde_json::json!({"kind": "around", "target": jumped["target"]}),
                    )
                    .await
                    .unwrap();
                    assert_eq!(page["targetSource"], jumped["source"]);
                    assert!(page.get("displaySegment").is_none());
                }
                assert_eq!(PREPARE_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
                let page_cursor = {
                    let resources = installed.agent.resident_resources.borrow();
                    let cut = resources
                        .get(&installed.session)
                        .unwrap()
                        .history_page_cut
                        .as_ref()
                        .unwrap();
                    cut.cursor(
                        CursorRole::Before,
                        PagePosition {
                            record: 1,
                            part: 0,
                            offset: None,
                        },
                    )
                };
                let wrong_role = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    4,
                    serde_json::json!({"kind": "before", "cursor": page_cursor}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    wrong_role.data,
                    Some(serde_json::json!("history cursor has the wrong role"))
                );
                let timeline_cursor = newest_page["olderCursor"].as_str().unwrap();
                let page_role = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "before", "cursor": timeline_cursor}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    page_role.data,
                    Some(serde_json::json!("history cursor has the wrong role"))
                );
                let mut forged: serde_json::Value =
                    serde_json::from_str(combo[0]["target"].as_str().unwrap()).unwrap();
                forged["ordinal"] = serde_json::json!(0);
                let tampered = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "around", "target": forged.to_string()}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    tampered.data,
                    Some(serde_json::json!(
                        "history page cursor is stale or tampered"
                    ))
                );
                let stale = timeline(
                    &installed.agent,
                    &installed.session,
                    "not-the-cut",
                    4,
                    serde_json::json!({"kind": "newest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    stale.data,
                    Some(serde_json::json!("history cut is stale or tampered"))
                );
                let replaced_token = {
                    let mut resources = installed.agent.resident_resources.borrow_mut();
                    let cut = resources
                        .get_mut(&installed.session)
                        .unwrap()
                        .history_page_cut
                        .as_mut()
                        .unwrap();
                    cut.set_branch_for_test(Some(chat_state::EventSeq::new(4)));
                    cut.token()
                };
                let replaced = timeline(
                    &installed.agent,
                    &installed.session,
                    &replaced_token,
                    4,
                    serde_json::json!({"kind": "newest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    replaced.data,
                    Some(serde_json::json!("history branch changed"))
                );
                assert!(installed._commands.try_recv().is_err());
            });
    }

    #[test]
    fn around_seeks_the_target_and_keeps_fragment_boundaries() {
        let _gate = PREPARE_GATE.lock().unwrap();
        PREPARE_CALLS.store(0, std::sync::atomic::Ordering::Relaxed);
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let huge = "H".repeat(crate::session::history::HISTORY_PAGE_BYTES);
                let mut facts = Vec::new();
                for n in 0..300 {
                    if n == 150 {
                        facts.push(user_fact(&huge, "big", None));
                    } else {
                        facts.push(user_fact(&format!("prompt-{n:03}"), &format!("id-{n}"), None));
                    }
                }
                let (_temp, session, timeline_state, history) = journal_facts("around", facts);
                let captured =
                    HistoryPageCut::capture(&session, &history, &timeline_state).unwrap();
                let missing = captured.cut.sign_target(
                    crate::session::history::HistorySourceKey { record: 9_999, part: 3 },
                    None,
                    0,
                );
                let installed = install(captured.cut);
                let catalog = timeline(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    10_000,
                    serde_json::json!({"kind": "newest"}),
                )
                .await
                .unwrap();
                assert_eq!(catalog["entries"].as_array().unwrap().len(), 128);
                assert_eq!(catalog["totalEntries"], 300);
                assert_eq!(PREPARE_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
                let entry_at = |ordinal: u64| {
                    let page = timeline(
                        &installed.agent,
                        &installed.session,
                        &installed.cut,
                        1,
                        serde_json::json!({"kind": "around_ordinal", "ordinal": ordinal}),
                    );
                    page
                };
                let first = entry_at(0).await.unwrap();
                let middle = entry_at(200).await.unwrap();
                let big = entry_at(150).await.unwrap();
                let last = entry_at(299).await.unwrap();
                assert_eq!(big["entries"][0]["preview"].as_str().unwrap().chars().count(), 160);
                assert!(big["entries"][0]["preview"].as_str().unwrap().chars().all(|ch| ch == 'H'));
                assert_ne!(big["entries"][0]["target"], first["entries"][0]["target"]);
                let jump = |target: &serde_json::Value| {
                    page(
                        &installed.agent,
                        &installed.session,
                        &installed.cut,
                        serde_json::json!({"kind": "around", "target": target}),
                    )
                };
                let first_page = jump(&first["entries"][0]["target"]).await.unwrap();
                let first_rows = rows_of(&first_page);
                assert_eq!(
                    first_rows[0].source,
                    serde_json::from_value(first["entries"][0]["source"].clone()).unwrap()
                );
                assert!(first_page.get("olderCursor").is_none());
                assert!(user_text(&first_rows[0]).unwrap().starts_with("prompt-000"));
                let middle_page = jump(&middle["entries"][0]["target"]).await.unwrap();
                let middle_rows = rows_of(&middle_page);
                assert!(middle_rows.iter().any(|row| user_text(row).unwrap() == "prompt-200"));
                assert!(middle_rows.iter().all(|row| {
                    let text = user_text(row).unwrap();
                    text != "prompt-000" && text != "prompt-299"
                }));
                assert!(middle_page.get("olderCursor").is_some());
                assert!(middle_page.get("newerCursor").is_some());
                assert!(middle_rows.len() <= 128);
                assert!(serde_json::to_vec(&middle_rows).unwrap().len() <= crate::session::history::HISTORY_PAGE_BYTES);
                let last_page = jump(&last["entries"][0]["target"]).await.unwrap();
                let last_rows = rows_of(&last_page);
                assert_eq!(user_text(last_rows.last().unwrap()).unwrap(), "prompt-299");
                assert!(last_page.get("newerCursor").is_none());
                let big_page = jump(&big["entries"][0]["target"]).await.unwrap();
                let big_rows = rows_of(&big_page);
                assert!(big_rows.len() <= 128);
                assert!(serde_json::to_vec(&big_rows).unwrap().len() <= crate::session::history::HISTORY_PAGE_BYTES);
                let big_row = big_rows
                    .iter()
                    .find(|row| {
                        row.source
                            == serde_json::from_value(big["entries"][0]["source"].clone()).unwrap()
                    })
                    .unwrap();
                let fragment = big_row.fragment.unwrap();
                assert_eq!(
                    fragment.coordinate,
                    crate::session::history::HistoryFragmentCoordinate::Utf8Bytes
                );
                assert_eq!(fragment.start, 0);
                let body = user_text(big_row).unwrap();
                assert!(body.chars().all(|ch| ch == 'H'));
                assert!(body.len() < huge.len());
                assert_eq!(big_page["targetSource"], big["entries"][0]["source"]);
                let older = big_page["olderCursor"].as_str().unwrap();
                let before_big = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "before", "cursor": older}),
                )
                .await
                .unwrap();
                let before_rows = rows_of(&before_big);
                let resumed = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "after", "cursor": before_big["newerCursor"]}),
                )
                .await
                .unwrap();
                let resumed_rows = rows_of(&resumed);
                assert_eq!(resumed_rows[0].source, big_rows[0].source);
                assert_eq!(resumed_rows[0].fragment, big_rows[0].fragment);
                assert!(!before_rows.is_empty());
                let middle_older = middle_page["olderCursor"].as_str().unwrap();
                let before_middle = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "before", "cursor": middle_older}),
                )
                .await
                .unwrap();
                let back = page(
                    &installed.agent,
                    &installed.session,
                    &installed.cut,
                    serde_json::json!({"kind": "after", "cursor": before_middle["newerCursor"]}),
                )
                .await
                .unwrap();
                assert_eq!(rows_of(&back)[0].source, middle_rows[0].source);
                assert_eq!(rows_of(&back)[0].fragment, middle_rows[0].fragment);
                if let Some(newer) = middle_page["newerCursor"].as_str() {
                    let after_middle = page(
                        &installed.agent,
                        &installed.session,
                        &installed.cut,
                        serde_json::json!({"kind": "after", "cursor": newer}),
                    )
                    .await
                    .unwrap();
                    let rewind = page(
                        &installed.agent,
                        &installed.session,
                        &installed.cut,
                        serde_json::json!({"kind": "before", "cursor": after_middle["olderCursor"]}),
                    )
                    .await
                    .unwrap();
                    assert_eq!(
                        rows_of(&rewind).last().unwrap().source,
                        middle_rows.last().unwrap().source
                    );
                    assert_eq!(
                        rows_of(&rewind).last().unwrap().fragment,
                        middle_rows.last().unwrap().fragment
                    );
                }
                let absent = jump(&serde_json::Value::String(missing)).await.unwrap_err();
                assert_eq!(
                    absent.data,
                    Some(serde_json::json!("history around target is not visible"))
                );
                assert_eq!(PREPARE_CALLS.load(std::sync::atomic::Ordering::Relaxed), 1);
            });
    }

    #[test]
    fn cursor_tail_load_keeps_the_installed_cut_readable() {
        let _gate = PREPARE_GATE.lock().unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (_temp, session, timeline_state, history) = journal(4);
                let captured =
                    HistoryPageCut::capture(&session, &history, &timeline_state).unwrap();
                let token = captured.cut.token();
                let installed = install(captured.cut);
                let latest = page(
                    &installed.agent,
                    &installed.session,
                    &token,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                installed
                    .agent
                    .apply_loaded_history_cut(&installed.session, None, true)
                    .await;
                let again = page(
                    &installed.agent,
                    &installed.session,
                    &token,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap();
                assert_eq!(again, latest);
                assert_eq!(again["cut"], token);
                let foreign = {
                    let mut resources = installed.agent.resident_resources.borrow_mut();
                    let cut = resources
                        .get_mut(&installed.session)
                        .unwrap()
                        .history_page_cut
                        .as_mut()
                        .unwrap();
                    cut.set_branch_for_test(Some(chat_state::EventSeq::new(9)));
                    cut.token()
                };
                let mismatched = page(
                    &installed.agent,
                    &installed.session,
                    &foreign,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    mismatched.data,
                    Some(serde_json::json!("history branch changed"))
                );
                installed
                    .agent
                    .apply_loaded_history_cut(&installed.session, None, true)
                    .await;
                let retired = page(
                    &installed.agent,
                    &installed.session,
                    &foreign,
                    serde_json::json!({"kind": "latest"}),
                )
                .await
                .unwrap_err();
                assert_eq!(
                    retired.data,
                    Some(serde_json::json!("history cut is stale or tampered"))
                );
            });
    }
}
