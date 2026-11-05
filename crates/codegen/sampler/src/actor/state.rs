//! Actor-internal state.
//!
//! All fields are touched only from the actor task, so no mutex /
//! atomic synchronization is needed -- the actor's command-loop
//! serialization gives us a "single-threaded with shared state"
//! discipline matching the hunk-tracker pattern.

use std::collections::HashMap;

use tokio::task::Id;
use tokio_util::sync::CancellationToken;

use crate::config::{RetryPolicy, SamplerConfig};
use crate::types::RequestId;

/// In-flight request bookkeeping.
///
/// `task_id` identifies this submission while it remains tracked by the
/// actor's JoinSet, even after its public request id has been reused.
/// `cancel_token` is shared with the submitting handle and per-request task.
/// The completion oneshot is moved into the task at spawn time.
pub(crate) struct ActiveRequest {
    pub(crate) task_id: Id,
    pub(crate) cancel_token: CancellationToken,
}

/// Actor-owned state.
pub(crate) struct ActorState {
    pub(crate) active_requests: HashMap<RequestId, ActiveRequest>,
    pub(crate) config: SamplerConfig,
    pub(crate) retry_policy: RetryPolicy,
}

impl ActorState {
    pub(crate) fn new(config: SamplerConfig, retry_policy: RetryPolicy) -> Self {
        Self {
            active_requests: HashMap::new(),
            config,
            retry_policy,
        }
    }

    /// Register a newly-spawned request. Returns the previous entry if
    /// the same `request_id` was already in flight; callers cancel that
    /// submission while retaining its task in the JoinSet.
    pub(crate) fn register(
        &mut self,
        request_id: RequestId,
        active: ActiveRequest,
    ) -> Option<ActiveRequest> {
        self.active_requests.insert(request_id, active)
    }

    /// Remove only the registration owned by this completed task. A replaced
    /// task remains in the JoinSet but no longer owns its public request id.
    pub(crate) fn remove_task(&mut self, task_id: Id) {
        self.active_requests
            .retain(|_, active| active.task_id != task_id);
    }

    /// Cancel and remove an in-flight request.
    pub(crate) fn cancel(&mut self, request_id: &RequestId) -> bool {
        if let Some(active) = self.active_requests.remove(request_id) {
            active.cancel_token.cancel();
            true
        } else {
            false
        }
    }

    /// Replace the default config. The next request submitted without
    /// an override will use this.
    pub(crate) fn update_config(&mut self, config: SamplerConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ApiBackend;
    use indexmap::IndexMap;

    /// Minimal config builder for tests in this module.
    fn cfg() -> SamplerConfig {
        SamplerConfig {
            api_key: None,
            base_url: "https://example.test".into(),
            model: "test-model".into(),
            output_limit: None,
            temperature: None,
            top_p: None,
            api_backend: ApiBackend::ChatCompletions,
            auth_scheme: Default::default(),
            extra_headers: IndexMap::new(),
            query_params: IndexMap::new(),
            env_http_headers: IndexMap::new(),
            context_window: 8192,
            force_http1: false,
            max_retries: None,
            stream_tool_calls: false,
            idle_timeout_secs: None,
            reasoning_effort: None,
            origin_client: None,
            bearer_resolver: None,
            compactions_remaining: None,
            compaction_at_tokens: None,
            doom_loop_recovery: None,
        }
    }

    #[test]
    fn cancel_unknown_request_returns_false() {
        let mut state = ActorState::new(cfg(), RetryPolicy::default());
        assert!(!state.cancel(&RequestId::from("unknown")));
    }

    #[tokio::test]
    async fn register_then_cancel_removes() {
        let mut state = ActorState::new(cfg(), RetryPolicy::default());
        let mut tasks = tokio::task::JoinSet::new();
        let id = RequestId::from("req-1");
        state.register(
            id.clone(),
            ActiveRequest {
                task_id: tasks.spawn(async {}).id(),
                cancel_token: CancellationToken::new(),
            },
        );
        assert_eq!(state.active_requests.len(), 1);
        assert!(state.cancel(&id));
        assert_eq!(state.active_requests.len(), 0);
    }

    #[tokio::test]
    async fn register_returns_previous_when_same_id() {
        let mut state = ActorState::new(cfg(), RetryPolicy::default());
        let mut tasks = tokio::task::JoinSet::new();
        let id = RequestId::from("req-1");
        let first = ActiveRequest {
            task_id: tasks.spawn(async {}).id(),
            cancel_token: CancellationToken::new(),
        };
        let second = ActiveRequest {
            task_id: tasks.spawn(async {}).id(),
            cancel_token: CancellationToken::new(),
        };
        assert!(state.register(id.clone(), first).is_none());
        assert!(state.register(id.clone(), second).is_some());
    }
}
