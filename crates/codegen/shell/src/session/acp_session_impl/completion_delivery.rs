//! Delivery state for results whose blocking wait was displaced by user steering.
//!
//! A model tool call must receive exactly one `tool_result`.  When an active
//! Goal is waiting for a background task and the user interjects, the wait call
//! is therefore closed with a "moved to background" result.  The eventual task
//! result is delivered later as a system reminder, never as a second result for
//! the original call id.

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct DeferredCompletion {
    pub(crate) task_id: String,
    pub(crate) body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryState {
    Awaiting,
    DeferredBySteering,
}

#[derive(Debug)]
struct DeliveryEntry {
    state: DeliveryState,
    ready: Option<(u64, String)>,
}

#[derive(Debug, Default)]
struct DeliveryInner {
    entries: std::collections::HashMap<String, DeliveryEntry>,
    next_sequence: u64,
    generation: u64,
}

/// Session-local, race-safe handoff between wait tools and completion sources.
///
/// Completion sources may win the race with the wait wrapper.  Keeping the
/// payload beside the wait state lets `defer_wait` expose an already-ready
/// result, while `finish_wait` discards it when the original tool result won.
#[derive(Debug, Default, Clone)]
pub(crate) struct CompletionDeliveryTracker {
    inner: std::sync::Arc<parking_lot::Mutex<DeliveryInner>>,
    changed: std::sync::Arc<tokio::sync::Notify>,
}

impl CompletionDeliveryTracker {
    pub(crate) fn begin_wait(&self, ids: &[String]) {
        let mut inner = self.inner.lock();
        for id in ids.iter().filter(|id| !id.trim().is_empty()) {
            inner.entries.entry(id.clone()).or_insert(DeliveryEntry {
                state: DeliveryState::Awaiting,
                ready: None,
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
                state: DeliveryState::Awaiting,
                ready: None,
            });
            entry.state = DeliveryState::DeferredBySteering;
            newly_ready |= entry.ready.is_some();
        }
        if newly_ready {
            inner.generation = inner.generation.wrapping_add(1);
        }
        drop(inner);
        if newly_ready {
            self.changed.notify_waiters();
        }
    }

    /// Record a completion. Returns true only when it belongs to a wait that
    /// was explicitly deferred by steering and should wake the active turn.
    pub(crate) fn complete(&self, task_id: String, body: String) -> bool {
        let mut inner = self.inner.lock();
        let Some(entry) = inner.entries.get(&task_id) else {
            return false;
        };
        if entry.ready.is_some() {
            return false;
        }
        let state = entry.state;
        let sequence = inner.next_sequence;
        inner.next_sequence = inner.next_sequence.wrapping_add(1);
        let entry = inner
            .entries
            .get_mut(&task_id)
            .expect("entry was resolved above");
        entry.ready = Some((sequence, body));
        if state == DeliveryState::DeferredBySteering {
            inner.generation = inner.generation.wrapping_add(1);
            drop(inner);
            self.changed.notify_waiters();
            true
        } else {
            false
        }
    }

    /// Queue an internal Goal-stage completion that already owns asynchronous
    /// delivery (there is no model tool call to pair). This shares ordering
    /// and evaluator invalidation with steering-deferred task completions.
    pub(crate) fn queue_ready(&self, task_id: String, body: String) {
        let mut inner = self.inner.lock();
        let sequence = inner.next_sequence;
        inner.next_sequence = inner.next_sequence.wrapping_add(1);
        inner.entries.insert(
            task_id,
            DeliveryEntry {
                state: DeliveryState::DeferredBySteering,
                ready: Some((sequence, body)),
            },
        );
        inner.generation = inner.generation.wrapping_add(1);
        drop(inner);
        self.changed.notify_waiters();
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
        self.inner
            .lock()
            .entries
            .values()
            .any(|entry| entry.state == DeliveryState::DeferredBySteering && entry.ready.is_some())
    }

    /// Drain ready deferred results in completion order. Draining is the
    /// delivery commit, so later output polling cannot surface them twice.
    pub(crate) fn drain_ready(&self) -> Vec<DeferredCompletion> {
        let mut inner = self.inner.lock();
        let mut ready = Vec::new();
        let ids = inner
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (entry.state == DeliveryState::DeferredBySteering)
                    .then(|| entry.ready.as_ref().map(|(seq, _)| (*seq, id.clone())))
                    .flatten()
            })
            .collect::<Vec<_>>();
        let mut ids = ids;
        ids.sort_by_key(|(sequence, _)| *sequence);
        for (_, id) in ids {
            if let Some(entry) = inner.entries.remove(&id)
                && let Some((_, body)) = entry.ready
            {
                ready.push(DeferredCompletion { task_id: id, body });
            }
        }
        ready
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
    /// Inject deferred completions as hidden system reminders. Returns true so
    /// callers can force another model iteration before Goal evaluation.
    pub(super) async fn drain_deferred_completions(&self) -> bool {
        let completions = self.completion_delivery.drain_ready();
        if completions.is_empty() {
            return false;
        }
        let ids = completions
            .iter()
            .map(|completion| completion.task_id.clone())
            .collect::<Vec<_>>();
        for completion in completions {
            self.push_system_reminder(&completion.body);
        }
        self.mark_completions_reported(&ids.iter().map(String::as_str).collect::<Vec<_>>())
            .await;
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
        tracker.begin_wait(&["a".into()]);
        assert!(!tracker.complete("a".into(), "done".into()));
        tracker.defer_wait(&["a".into()]);
        assert!(tracker.has_ready());
        assert_eq!(tracker.drain_ready()[0].body, "done");
        assert!(!tracker.contains("a"));
    }

    #[test]
    fn normal_wait_completion_is_not_delivered_twice() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(&["a".into()]);
        assert!(!tracker.complete("a".into(), "done".into()));
        tracker.finish_wait(&["a".into()]);
        assert!(tracker.drain_ready().is_empty());
    }

    #[test]
    fn deferred_completions_keep_completion_order() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(&["a".into(), "b".into()]);
        tracker.defer_wait(&["a".into(), "b".into()]);
        assert!(tracker.complete("b".into(), "second task first".into()));
        assert!(tracker.complete("a".into(), "first task second".into()));
        let drained = tracker.drain_ready();
        assert_eq!(drained[0].task_id, "b");
        assert_eq!(drained[1].task_id, "a");
    }

    #[test]
    fn explicit_output_consumes_queued_completion() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(&["a".into()]);
        tracker.defer_wait(&["a".into()]);
        assert!(tracker.complete("a".into(), "done".into()));
        tracker.consume(&["a"]);
        assert!(tracker.drain_ready().is_empty());
    }

    #[test]
    fn duplicate_completion_does_not_reorder_or_wake_twice() {
        let tracker = CompletionDeliveryTracker::default();
        tracker.begin_wait(&["a".into()]);
        tracker.defer_wait(&["a".into()]);
        assert!(tracker.complete("a".into(), "first".into()));
        let generation = tracker.generation();
        assert!(!tracker.complete("a".into(), "duplicate".into()));
        assert_eq!(tracker.generation(), generation);
        assert_eq!(tracker.drain_ready()[0].body, "first");
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
        tracker.queue_ready("goal-stage".into(), "done".into());
        waiter.await.unwrap();
    }
}
