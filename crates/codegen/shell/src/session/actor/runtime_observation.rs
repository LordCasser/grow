//! Read-only, bounded runtime facts for session lifecycle queries.
//!
//! The actor owns writes. Readers take a short synchronous snapshot and never
//! enqueue a command behind model execution or persistence work.

use std::collections::VecDeque;
use std::sync::Arc;

use acp_transport::protocol as acp;
use parking_lot::RwLock;

use crate::session::prompt_queue::PromptStatus;

pub(crate) const MAX_RUNTIME_OBSERVATION_ENTRIES: usize = 128;

pub(crate) type SharedRuntimeObservation = Arc<RwLock<RuntimeObservationSnapshot>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeTerminal {
    pub(crate) prompt_id: String,
    pub(crate) stop_reason: String,
    pub(crate) agent_result: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeObservationSnapshot {
    pub(crate) session_id: acp::SessionId,
    pub(crate) control_epoch: String,
    pub(crate) queue: VecDeque<(String, usize)>,
    pub(crate) running: Option<(String, u64)>,
    pub(crate) settling: Option<String>,
    pub(crate) terminals: VecDeque<RuntimeTerminal>,
    pub(crate) unavailable: Option<String>,
}

impl RuntimeObservationSnapshot {
    pub(crate) fn new(session_id: acp::SessionId, control_epoch: String) -> Self {
        Self {
            session_id,
            control_epoch,
            queue: VecDeque::new(),
            running: None,
            settling: None,
            terminals: VecDeque::new(),
            unavailable: Some("runtime observation is initializing".to_string()),
        }
    }

    pub(crate) fn replace_queue_running(
        &mut self,
        queue: Vec<(String, usize)>,
        running: Option<(String, u64)>,
    ) {
        self.queue = queue
            .into_iter()
            .take(MAX_RUNTIME_OBSERVATION_ENTRIES)
            .collect();
        self.running = running;
        self.settling = None;
        self.unavailable = None;
    }

    pub(crate) fn settling(&mut self, prompt_id: &str) {
        self.running = None;
        self.settling = Some(prompt_id.to_string());
        self.unavailable = None;
    }

    pub(crate) fn terminal(
        &mut self,
        prompt_id: &str,
        stop_reason: String,
        agent_result: Option<String>,
    ) {
        self.running = None;
        self.settling = None;
        self.queue.retain(|(id, _)| id != prompt_id);
        if let Some(index) = self
            .terminals
            .iter()
            .position(|terminal| terminal.prompt_id == prompt_id)
        {
            self.terminals.remove(index);
        }
        self.terminals.push_back(RuntimeTerminal {
            prompt_id: prompt_id.to_string(),
            stop_reason,
            agent_result,
        });
        while self.terminals.len() > MAX_RUNTIME_OBSERVATION_ENTRIES {
            self.terminals.pop_front();
        }
        self.unavailable = None;
    }

    pub(crate) fn unavailable(&mut self, reason: impl Into<String>) {
        self.unavailable = Some(reason.into());
        self.running = None;
        self.settling = None;
    }

    pub(crate) fn prompt_status(&self, prompt_id: &str) -> PromptStatus {
        if let Some(terminal) = self
            .terminals
            .iter()
            .rev()
            .find(|terminal| terminal.prompt_id == prompt_id)
        {
            return PromptStatus::Terminal {
                stop_reason: terminal.stop_reason.clone(),
                agent_result: terminal.agent_result.clone(),
            };
        }
        if let Some(reason) = &self.unavailable {
            return PromptStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        if self.settling.as_deref() == Some(prompt_id) {
            return PromptStatus::Settling;
        }
        if let Some((running_id, turn_start_ms)) = &self.running
            && running_id == prompt_id
        {
            return PromptStatus::Running {
                turn_start_ms: *turn_start_ms,
            };
        }
        if let Some((_, position)) = self
            .queue
            .iter()
            .find(|(queued_id, _)| queued_id == prompt_id)
        {
            return PromptStatus::Queued {
                position: *position,
                queue_version: 0,
            };
        }
        PromptStatus::Unknown
    }

    pub(crate) fn lifecycle_status(&self) -> PromptStatus {
        if let Some(reason) = &self.unavailable {
            return PromptStatus::Unavailable {
                reason: reason.clone(),
            };
        }
        if let Some((_, turn_start_ms)) = &self.running {
            return PromptStatus::Running {
                turn_start_ms: *turn_start_ms,
            };
        }
        if self.settling.is_some() {
            return PromptStatus::Settling;
        }
        self.terminals
            .back()
            .map_or(PromptStatus::Unknown, |terminal| PromptStatus::Terminal {
                stop_reason: terminal.stop_reason.clone(),
                agent_result: terminal.agent_result.clone(),
            })
    }
}

pub(crate) struct RuntimeObservationOwnerGuard(SharedRuntimeObservation);

impl RuntimeObservationOwnerGuard {
    pub(crate) fn new(observation: SharedRuntimeObservation) -> Self {
        Self(observation)
    }
}

impl Drop for RuntimeObservationOwnerGuard {
    fn drop(&mut self) {
        self.0.write().unavailable("session runtime owner exited");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_lookup_preserves_bounded_runtime_facts() {
        let mut snapshot = RuntimeObservationSnapshot::new(
            acp::SessionId::new("session-1"),
            "epoch-1".to_string(),
        );
        snapshot.replace_queue_running(
            vec![("queued-1".to_string(), 4)],
            Some(("running-1".to_string(), 123)),
        );

        assert!(matches!(
            snapshot.prompt_status("running-1"),
            PromptStatus::Running { turn_start_ms: 123 }
        ));
        assert!(matches!(
            snapshot.prompt_status("queued-1"),
            PromptStatus::Queued { position: 4, .. }
        ));

        snapshot.terminal(
            "running-1",
            "end_turn".to_string(),
            Some("done".to_string()),
        );
        assert!(matches!(
            snapshot.prompt_status("running-1"),
            PromptStatus::Terminal { agent_result: Some(result), .. } if result == "done"
        ));
        assert_eq!(snapshot.terminals.len(), 1);
    }

    #[test]
    fn owner_drop_marks_snapshot_unavailable() {
        let snapshot = Arc::new(RwLock::new(RuntimeObservationSnapshot::new(
            acp::SessionId::new("session-1"),
            "epoch-1".to_string(),
        )));
        let owner = RuntimeObservationOwnerGuard::new(snapshot.clone());
        drop(owner);
        assert!(matches!(
            snapshot.read().lifecycle_status(),
            PromptStatus::Unavailable { .. }
        ));
    }
}
