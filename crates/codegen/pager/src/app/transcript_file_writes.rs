//! Ordered, application-owned explicit transcript file writes.
use crate::app::actions::{Effect, TaskResult};
use crate::app::session::AgentId;
use acp_transport::protocol as acp;
use std::collections::VecDeque;
use std::path::PathBuf;

const MAX_PENDING: usize = 8;

#[derive(Debug, Clone, Copy)]
pub enum FileWriteKind {
    Copy,
    Export,
}

#[derive(Debug)]
pub struct TranscriptFileWrite {
    pub agent_id: AgentId,
    pub session_id: Option<acp::SessionId>,
    pub path: PathBuf,
    pub content: String,
    pub kind: FileWriteKind,
}

impl TranscriptFileWrite {
    fn write(self) -> std::io::Result<String> {
        match self.kind {
            FileWriteKind::Copy => {
                let path = crate::clipboard::write_text_to_copy_file(&self.content, &self.path)?;
                Ok(format!(
                    "Copied to {}{}",
                    path.display(),
                    crate::clipboard::clipboard_stats_suffix(&self.content)
                ))
            }
            FileWriteKind::Export => {
                crate::export_cmd::write_export_file(&self.path, &self.content)?;
                Ok(format!("Conversation exported to {}", self.path.display()))
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct TranscriptFileWrites {
    pending: VecDeque<TranscriptFileWrite>,
    active: Option<u64>,
    next_id: u64,
}

impl TranscriptFileWrites {
    pub fn enqueue(&mut self, request: TranscriptFileWrite) -> Result<Option<Effect>, ()> {
        if self.pending.len() >= MAX_PENDING {
            return Err(());
        }
        self.pending.push_back(request);
        Ok(self.start_next())
    }

    pub fn finish(&mut self, id: u64) -> bool {
        if self.active != Some(id) {
            return false;
        }
        self.active = None;
        true
    }

    pub fn start_next(&mut self) -> Option<Effect> {
        if self.active.is_some() {
            return None;
        }
        let request = self.pending.pop_front()?;
        self.next_id += 1;
        self.active = Some(self.next_id);
        Some(Effect::WriteTranscriptFile {
            id: self.next_id,
            request,
        })
    }
}

pub(crate) async fn execute(id: u64, request: TranscriptFileWrite) -> TaskResult {
    let agent_id = request.agent_id;
    let session_id = request.session_id.clone();
    execute_write(id, agent_id, session_id, move || request.write()).await
}

async fn execute_write(
    id: u64,
    agent_id: AgentId,
    session_id: Option<acp::SessionId>,
    write: impl FnOnce() -> std::io::Result<String> + Send + 'static,
) -> TaskResult {
    let result = match tokio::task::spawn_blocking(write).await {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(error) => Err(format!("File writer failed: {error}")),
    };
    TaskResult::TranscriptFileWritten {
        id,
        agent_id,
        session_id,
        result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(text: &str) -> TranscriptFileWrite {
        TranscriptFileWrite {
            agent_id: AgentId(0),
            session_id: None,
            path: "unused".into(),
            content: text.into(),
            kind: FileWriteKind::Export,
        }
    }
    #[test]
    fn queue_is_bounded_serial_and_ignores_stale_completion() {
        let mut queue = TranscriptFileWrites::default();
        let Some(Effect::WriteTranscriptFile { id, .. }) = queue.enqueue(request("first")).unwrap()
        else {
            panic!()
        };
        for i in 0..MAX_PENDING {
            assert!(queue.enqueue(request(&i.to_string())).unwrap().is_none());
        }
        assert!(queue.enqueue(request("overflow")).is_err());
        assert!(!queue.finish(id + 1));
        assert!(queue.start_next().is_none());
        assert!(queue.finish(id));
        let Some(Effect::WriteTranscriptFile {
            id: next_id,
            request,
        }) = queue.start_next()
        else {
            panic!()
        };
        assert_eq!(request.content, "0");
        assert!(!queue.finish(id));
        assert!(queue.finish(next_id));
        let Some(Effect::WriteTranscriptFile { request, .. }) = queue.start_next() else {
            panic!()
        };
        assert_eq!(request.content, "1");
    }
    #[tokio::test]
    async fn blocked_file_io_does_not_block_async_runtime() {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = tokio::spawn(execute_write(1, AgentId(0), None, move || {
            let _ = started_tx.send(());
            release_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(std::io::Error::other)?;
            Ok("saved".to_string())
        }));
        started_rx.await.unwrap();
        tokio::task::yield_now().await;
        assert!(
            !worker.is_finished(),
            "runtime must remain responsive while I/O is blocked"
        );
        release_tx.send(()).unwrap();
        assert!(matches!(
            worker.await.unwrap(),
            TaskResult::TranscriptFileWritten { result: Ok(_), .. }
        ));
    }
    #[tokio::test]
    async fn worker_panic_returns_failure() {
        let result = execute_write(7, AgentId(0), None, || panic!("injected writer panic")).await;
        assert!(matches!(
            result,
            TaskResult::TranscriptFileWritten {
                id: 7,
                result: Err(_),
                ..
            }
        ));
    }
}
