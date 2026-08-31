//! Delivery state for results whose blocking wait was displaced by user steering.
//!
//! A model tool call must receive exactly one `tool_result`.  When an active
//! Goal is waiting for a background task and the user interjects, the wait call
//! is therefore closed with a "moved to background" result.  The eventual task
//! result is delivered later as a system reminder, never as a second result for
//! the original call id.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryState {
    Awaiting,
    DeferredBySteering,
}

#[derive(Debug)]
struct DeliveryEntry {
    owner_turn: Option<String>,
    state: DeliveryState,
    ready_sequence: Option<u64>,
}

#[derive(Debug, Default)]
struct DeliveryInner {
    entries: std::collections::HashMap<String, DeliveryEntry>,
    next_sequence: u64,
    generation: u64,
}

/// Session-local, race-safe handoff between wait tools and completion sources.
///
/// Completion sources may win the race with the wait wrapper. Payloads stay
/// solely in the durable notification inbox; this tracker owns only the
/// ephemeral wait handoff.
#[derive(Debug, Default, Clone)]
pub(crate) struct CompletionDeliveryTracker {
    inner: std::sync::Arc<parking_lot::Mutex<DeliveryInner>>,
    changed: std::sync::Arc<tokio::sync::Notify>,
}

impl CompletionDeliveryTracker {
    pub(crate) fn begin_wait(&self, owner_turn: Option<&str>, ids: &[String]) {
        let mut inner = self.inner.lock();
        for id in ids.iter().filter(|id| !id.trim().is_empty()) {
            inner.entries.entry(id.clone()).or_insert(DeliveryEntry {
                owner_turn: owner_turn.map(str::to_owned),
                state: DeliveryState::Awaiting,
                ready_sequence: None,
            });
        }
    }

    /// The original wait returned normally. Any completion it raced is owned
    /// by that tool result and must not be surfaced again.
    pub(crate) fn finish_wait(&self, ids: &[String]) {
        let mut inner = self.inner.lock();
        for id in ids {
            inner.entries.remove(id);
        }
    }

    /// A later output/kill tool explicitly returned the terminal payload.
    /// Remove any queued reminder in the same commit path so the model cannot
    /// observe the completion twice.
    pub(crate) fn consume(&self, ids: &[&str]) {
        let mut inner = self.inner.lock();
        for id in ids {
            inner.entries.remove(*id);
        }
    }

    /// Transfer a wait to asynchronous delivery before its receiver is
    /// dropped. This ordering is what makes completion-vs-interjection races
    /// lossless.
    pub(crate) fn defer_wait(&self, ids: &[String]) {
        let mut inner = self.inner.lock();
        let mut newly_ready = false;
        for id in ids.iter().filter(|id| !id.trim().is_empty()) {
            let entry = inner.entries.entry(id.clone()).or_insert(DeliveryEntry {
                owner_turn: None,
                state: DeliveryState::Awaiting,
                ready_sequence: None,
            });
            entry.state = DeliveryState::DeferredBySteering;
            newly_ready |= entry.ready_sequence.is_some();
        }
        if newly_ready {
            inner.generation = inner.generation.wrapping_add(1);
        }
        drop(inner);
        if newly_ready {
            self.changed.notify_waiters();
        }
    }

    /// Transfer every still-blocking wait owned by an aborted turn to the
    /// asynchronous completion rail. This must run before the turn future is
    /// dropped; otherwise its select branch cannot perform `defer_wait` and an
    /// `Awaiting` reservation would suppress the eventual completion forever.
    pub(crate) fn defer_turn_waits(&self, owner_turn: &str) {
        let mut inner = self.inner.lock();
        let mut newly_ready = false;
        for entry in inner.entries.values_mut().filter(|entry| {
            entry.owner_turn.as_deref() == Some(owner_turn)
                && entry.state == DeliveryState::Awaiting
        }) {
            entry.state = DeliveryState::DeferredBySteering;
            newly_ready |= entry.ready_sequence.is_some();
        }
        if newly_ready {
            inner.generation = inner.generation.wrapping_add(1);
        }
        drop(inner);
        if newly_ready {
            self.changed.notify_waiters();
        }
    }

    /// Retire every reservation owned by a turn whose background work is being
    /// killed. There can be no later result to deliver for these tasks.
    pub(crate) fn consume_turn_waits(&self, owner_turn: &str) {
        self.inner
            .lock()
            .entries
            .retain(|_, entry| entry.owner_turn.as_deref() != Some(owner_turn));
    }

    /// Record a completion. Returns true only when it belongs to a wait that
    /// was explicitly deferred by steering and should wake the active turn.
    pub(crate) fn complete(&self, task_id: String) -> bool {
        let mut inner = self.inner.lock();
        let Some(entry) = inner.entries.get(&task_id) else {
            return false;
        };
        if entry.ready_sequence.is_some() {
            return false;
        }
        let state = entry.state;
        let sequence = inner.next_sequence;
        inner.next_sequence = inner.next_sequence.wrapping_add(1);
        let entry = inner
            .entries
            .get_mut(&task_id)
            .expect("entry was resolved above");
        entry.ready_sequence = Some(sequence);
        if state == DeliveryState::DeferredBySteering {
            inner.generation = inner.generation.wrapping_add(1);
            drop(inner);
            self.changed.notify_waiters();
            true
        } else {
            false
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.inner.lock().generation
    }

    /// Wait for a new model-visible completion generation without polling.
    /// Registering before the second generation read closes the producer race.
    pub(crate) async fn wait_generation_change(&self, generation: u64) {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.has_ready() || self.generation() != generation {
                return;
            }
            notified.await;
        }
    }

    pub(crate) fn has_ready(&self) -> bool {
        self.inner.lock().entries.values().any(|entry| {
            entry.state == DeliveryState::DeferredBySteering && entry.ready_sequence.is_some()
        })
    }

    /// Snapshot ready ids in completion order. The caller removes them only
    /// after the Timeline consumption event commits.
    pub(crate) fn ready_ids(&self) -> Vec<String> {
        let inner = self.inner.lock();
        let mut ids = inner
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (entry.state == DeliveryState::DeferredBySteering)
                    .then(|| entry.ready_sequence.map(|sequence| (sequence, id.clone())))
                    .flatten()
            })
            .collect::<Vec<_>>();
        ids.sort_by_key(|(sequence, _)| *sequence);
        ids.into_iter().map(|(_, id)| id).collect()
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, id: &str) -> bool {
        self.inner.lock().entries.contains_key(id)
    }
}

/// Extract the task identities carried by all interruptible wait-tool shapes.
pub(super) fn wait_task_ids(args: &serde_json::Value) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(id) = args.get("task_id").and_then(serde_json::Value::as_str) {
        ids.push(id.to_owned());
    }
    if let Some(values) = args.get("task_ids").and_then(serde_json::Value::as_array) {
        ids.extend(
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned),
        );
    }
    ids.retain(|id| !id.trim().is_empty());
    ids.sort();
    ids.dedup();
    ids
}

impl SessionActor {
    /// Consume steering-deferred completion receipts into the current turn.
    /// The Timeline fact both acknowledges the inbox and materializes the
    /// exact synthetic input, so a crash cannot split those operations.
    pub(super) async fn drain_deferred_completions(&self) -> bool {
        let ids = self.completion_delivery.ready_ids();
        if ids.is_empty() {
            return false;
        }
        let Some(turn) = self.events.current_turn() else {
            tracing::error!(task_ids = ?ids, "deferred completions became ready outside a turn");
            return false;
        };
        let pending = self
            .chat_state_handle
            .pending_notifications()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|notification| match &notification.source {
                chat_state::NotificationSource::TaskCompleted { task_id, .. } => {
                    ids.contains(&task_id)
                }
                chat_state::NotificationSource::SubagentCompleted { subagent_id, .. } => {
                    ids.contains(&subagent_id)
                }
                chat_state::NotificationSource::MonitorProgress { .. }
                | chat_state::NotificationSource::TaskStillRunning { .. }
                | chat_state::NotificationSource::PlanHandoff { .. }
                | chat_state::NotificationSource::WorkflowHandoff { .. } => false,
            })
            .collect::<Vec<_>>();
        if pending.len() != ids.len() {
            tracing::warn!(task_ids = ?ids, "deferred completion receipt is not yet visible");
            return false;
        }
        let directory = match self.session_directory.try_clone() {
            Ok(directory) => directory,
            Err(error) => {
                tracing::error!(task_ids = ?ids, %error, "cannot open deferred completion inbox");
                return false;
            }
        };
        let payload_refs = ids
            .iter()
            .filter_map(|id| {
                pending
                    .iter()
                    .find(|notification| match &notification.source {
                        chat_state::NotificationSource::TaskCompleted { task_id, .. } => {
                            task_id == id
                        }
                        chat_state::NotificationSource::SubagentCompleted {
                            subagent_id, ..
                        } => subagent_id == id,
                        _ => false,
                    })
                    .map(|notification| notification.payload_ref.clone())
            })
            .collect::<Vec<_>>();
        let bodies = match tokio::task::spawn_blocking(move || {
            payload_refs
                .iter()
                .map(|payload| {
                    crate::session::notification_inbox::read_payload(&directory, payload)
                })
                .collect::<std::io::Result<Vec<_>>>()
        })
        .await
        {
            Ok(Ok(bodies)) => bodies,
            Ok(Err(error)) => {
                tracing::error!(task_ids = ?ids, %error, "deferred completion payload is missing or corrupt");
                return false;
            }
            Err(error) => {
                tracing::error!(task_ids = ?ids, %error, "deferred completion payload reader failed");
                return false;
            }
        };
        let notification_ids = pending
            .iter()
            .map(|notification| notification.id.clone())
            .collect::<Vec<_>>();
        let body = bodies.join("\n\n---\n\n");
        let mut input = sampling_types::ConversationItem::notification_drain(body);
        input.set_prompt_index(self.chat_state_handle.get_prompt_index().await);
        if let Err(error) = self
            .consume_notifications_durably(notification_ids, turn, Some(input))
            .await
        {
            tracing::error!(task_ids = ?ids, %error, "failed to consume deferred completion receipts");
            return false;
        }
        let consumed_ids = ids.iter().map(String::as_str).collect::<Vec<_>>();
        self.completion_delivery.consume(&consumed_ids);
        tracing::info!(task_ids = ?ids, "delivered steering-deferred completion(s) to main agent");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_before_defer_is_not_lost() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into()]);
        assert!(!tracker.complete("a".into()));
        tracker.defer_wait(&["a".into()]);
        assert!(tracker.has_ready());
        assert_eq!(tracker.ready_ids(), vec!["a"]);
        tracker.consume(&["a"]);
        assert!(!tracker.contains("a"));
    }

    #[test]
    fn normal_wait_completion_is_not_delivered_twice() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into()]);
        assert!(!tracker.complete("a".into()));
        tracker.finish_wait(&["a".into()]);
        assert!(tracker.ready_ids().is_empty());
    }

    #[test]
    fn deferred_completions_keep_completion_order() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into(), "b".into()]);
        tracker.defer_wait(&["a".into(), "b".into()]);
        assert!(tracker.complete("b".into()));
        assert!(tracker.complete("a".into()));
        assert_eq!(tracker.ready_ids(), vec!["b", "a"]);
    }

    #[test]
    fn explicit_output_consumes_queued_completion() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into()]);
        tracker.defer_wait(&["a".into()]);
        assert!(tracker.complete("a".into()));
        tracker.consume(&["a"]);
        assert!(tracker.ready_ids().is_empty());
    }

    #[test]
    fn duplicate_completion_does_not_reorder_or_wake_twice() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into()]);
        tracker.defer_wait(&["a".into()]);
        assert!(tracker.complete("a".into()));
        let generation = tracker.generation();
        assert!(!tracker.complete("a".into()));
        assert_eq!(tracker.generation(), generation);
        assert_eq!(tracker.ready_ids(), vec!["a"]);
    }

    #[tokio::test]
    async fn visible_generation_change_wakes_registered_observer() {
        let tracker = CompletionDeliveryTracker::default();
        let generation = tracker.generation();
        let observer = tracker.clone();
        let waiter = tokio::spawn(async move {
            observer.wait_generation_change(generation).await;
        });
        tokio::task::yield_now().await;
        tracker.begin_wait(None, &["goal-stage".into()]);
        tracker.defer_wait(&["goal-stage".into()]);
        tracker.complete("goal-stage".into());
        waiter.await.unwrap();
    }

    #[test]
    fn aborting_a_turn_defers_only_its_waits() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into()]);
        tracker.begin_wait(Some("turn-b"), &["b".into()]);

        assert!(!tracker.complete("a".into()));
        assert!(!tracker.complete("b".into()));
        tracker.defer_turn_waits("turn-a");

        assert_eq!(tracker.ready_ids(), vec!["a"]);
        assert!(tracker.contains("b"));
    }

    #[test]
    fn killing_a_turn_consumes_its_waits() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(Some("turn-a"), &["a".into()]);
        tracker.consume_turn_waits("turn-a");
        assert!(!tracker.complete("a".into()));
        assert!(!tracker.contains("a"));
    }
}
