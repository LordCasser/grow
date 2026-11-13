use std::{ffi::OsStr, io, sync::Arc};

use acp_transport::protocol as acp;
use serde::{Deserialize, Serialize};

use super::MvpAgent;
use crate::session::{
    history::{HistorySnapshot, IndexedPages},
    storage::{ContainedDirectory, SessionUpdate, TIMELINE_FILE, jsonl::JsonlStorageAdapter},
};

#[derive(Clone)]
pub(super) struct HistoryPageCut {
    id: uuid::Uuid,
    bytes: u64,
    hash: String,
    branch: Option<chat_state::EventSeq>,
    older_rows: usize,
    pages: Option<Arc<IndexedPages>>,
}

#[derive(Deserialize, Serialize)]
struct Position {
    end: usize,
    proof: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadPage {
    session_id: acp::SessionId,
    cursor: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    session_id: acp::SessionId,
    updates: Vec<SessionUpdate>,
    older_cursor: Option<String>,
}

impl HistoryPageCut {
    pub(super) fn new(
        history: &HistorySnapshot,
        timeline: &chat_state::Timeline,
        older_rows: usize,
        pages: Option<Arc<IndexedPages>>,
    ) -> Option<Self> {
        (older_rows != 0).then(|| Self {
            id: uuid::Uuid::new_v4(),
            bytes: history.bytes,
            hash: history.prefix_hash.clone(),
            branch: timeline.history_branch(),
            older_rows,
            pages,
        })
    }

    pub(super) fn first_cursor(&self) -> String {
        self.cursor(self.older_rows)
    }

    fn cursor(&self, end: usize) -> String {
        serde_json::to_string(&Position {
            end,
            proof: self.position_proof(end),
        })
        .expect("serialize history position")
    }

    fn position_proof(&self, end: usize) -> String {
        let key = blake3::derive_key("Grow session history page position", self.id.as_bytes());
        blake3::keyed_hash(&key, &(end as u64).to_le_bytes())
            .to_hex()
            .to_string()
    }

    fn read(
        &self,
        session_id: acp::SessionId,
        directory: &ContainedDirectory,
        end: usize,
    ) -> io::Result<Page> {
        if let Some(pages) = &self.pages {
            let (start, updates) = pages.page(&session_id, end)?;
            return Ok(Page {
                session_id,
                updates,
                older_cursor: (start != 0).then(|| self.cursor(start)),
            });
        }
        let name = OsStr::new(TIMELINE_FILE);
        let file = directory.open_regular(name, "Timeline journal")?;
        if file.metadata()?.len() < self.bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "history cut was truncated",
            ));
        }
        let mut budget = usize::MAX;
        let journal = JsonlStorageAdapter::read_journal_at_frontier(
            file,
            directory.display_path().join(name),
            self.bytes,
            &mut budget,
        )?;
        let mut history = journal.history;
        if history.bytes != self.bytes || history.prefix_hash != self.hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "history cut changed",
            ));
        }
        let timeline = chat_state::Timeline::from_events(journal.events)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        history.validate(&timeline)?;
        history.load_notices(directory, &timeline, session_id.0.as_ref())?;
        let mut projected = history.into_prepare(&session_id, &timeline, None)?;
        let (start, updates) = projected.take_page(end)?;
        Ok(Page {
            session_id,
            updates,
            older_cursor: (start != 0).then(|| self.cursor(start)),
        })
    }
}

impl MvpAgent {
    pub(super) async fn read_history_page(
        &self,
        args: &acp::ExtRequest,
    ) -> crate::extensions::ExtResult {
        let ReadPage { session_id, cursor } = crate::extensions::parse_params(args)?;
        let stale = || acp::Error::invalid_params().data("history page cursor is stale");
        let position: Position = serde_json::from_str(&cursor).map_err(|_| stale())?;
        let current_cut = || {
            self.resident_resources
                .borrow()
                .get(&session_id)
                .and_then(|resources| resources.history_page_cut.clone())
                .filter(|cut| cut.position_proof(position.end) == position.proof)
        };
        let cut = current_cut().ok_or_else(stale)?;
        if position.end == 0 || position.end > cut.older_rows {
            return Err(stale());
        }
        let handle = self
            .sessions
            .borrow()
            .get(&session_id)
            .cloned()
            .ok_or_else(stale)?;
        let branch = cut.branch;
        if handle.chat_state_handle.history_branch().await != Some(branch) {
            return Err(stale());
        }
        let directory = handle.session_directory.clone();
        let page_session = session_id.clone();
        let end = position.end;
        let page = tokio::task::spawn_blocking(move || cut.read(page_session, &directory, end))
            .await
            .map_err(|error| acp::Error::internal_error().data(error.to_string()))?
            .map_err(|error| crate::session::persistence::io_error_to_acp(&error))?;
        if handle.chat_state_handle.history_branch().await != Some(branch)
            || handle.cmd_tx.is_closed()
            || current_cut().is_none()
        {
            return Err(stale());
        }
        crate::extensions::to_raw_response(&page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::history::{HistoryContent, HistoryFact};
    use std::{io::Write, path::Path};

    #[test]
    fn page_cut_excludes_later_appends_and_rejects_replaced_prefix() {
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
        let facts: Vec<_> = (0..270)
            .map(|n| HistoryFact {
                meta: None,
                content: HistoryContent::Assistant(acp::ContentChunk::new(
                    acp::ContentBlock::Text(acp::TextContent::new(format!("history-{n}"))),
                )),
            })
            .collect();
        let original = JsonlStorageAdapter::encode_journal(timeline.events(), &facts).unwrap();
        let path = temp.path().join(TIMELINE_FILE);
        std::fs::write(&path, &original).unwrap();
        let history = JsonlStorageAdapter::read_journal_from_directory(&directory)
            .unwrap()
            .history;
        let cut = HistoryPageCut::new(&history, &timeline, 142, None).unwrap();
        let before = cut.read(session.clone(), &directory, 142).unwrap();
        assert_eq!(before.updates.len(), 128);

        // Reuse the exact committed prefix because the encoder stamps time.
        let extension = crate::session::history::encode_presentation(
            271,
            0,
            &serde_json::value::to_raw_value(&facts[0]).unwrap(),
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(&extension)
            .unwrap();
        let after = cut.read(session.clone(), &directory, 142).unwrap();
        assert_eq!(
            serde_json::to_value(after).unwrap(),
            serde_json::to_value(before).unwrap()
        );

        let mut changed = facts;
        changed[0] = changed[1].clone();
        let replaced = JsonlStorageAdapter::encode_journal(timeline.events(), &changed).unwrap();
        std::fs::write(&path, replaced).unwrap();
        assert!(cut.read(session, &directory, 142).is_err());
    }
}
