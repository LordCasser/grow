//! Backend trait abstracting how subagent operations are dispatched.
//!
//! `SubagentBackend` decouples the tool implementations (`TaskTool`,
//! `WaitSubagentTool`, `KillTaskTool`) from the transport mechanism used to
//! communicate with the subagent coordinator.
//!
//! All hosts use [`ChannelBackend`].
//! The receiver is owned by the shared single-writer coordinator actor; only
//! the child runner plugged into that actor differs by host.

use std::sync::Arc;
use std::sync::OnceLock;
use std::future::Future;

use tokio::sync::{Semaphore, mpsc, oneshot};
use tool_types::planning::PlanningReadSnapshot;

use super::parent_planning::{ParentPlanningReadError, ParentPlanningRequest};
use super::types::{
    SpawnedSubagentRef, SubagentCancelOutcome, SubagentCancelRequest, SubagentCancelTarget,
    SubagentEvent, SubagentInspectRequest, SubagentInspection, SubagentListRunningRequest,
    SubagentObservationError, SubagentQueryRequest, SubagentRegistryCounts,
    SubagentRegistryCountsRequest, SubagentRequest, SubagentResult, SubagentSnapshot,
    SubagentSpawnRequest, SubagentSpawnedRefsRequest, SubagentValidateTypeOutcome,
    SubagentValidateTypeRequest,
};
use crate::register_resource;
use tool_runtime::ToolError;
use tool_types::{SubagentDetails, SubagentListScope, SubagentStatus, SubagentSummary};

/// Abstraction over the mechanism used to spawn, query, and cancel subagents.
///
/// Injected into `Resources` as [`SubagentBackendResource`] so that
/// `TaskTool`, `WaitSubagentTool`, and `KillTaskTool` can operate
/// identically regardless of the underlying transport.
#[async_trait::async_trait]
pub trait SubagentBackend: Send + Sync + 'static {
    /// Read the session-bound caller's immediate parent's plan without changing
    /// either session. `Ok(None)` means the available parent has no local plan.
    async fn read_parent_plan(
        &self,
        _cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<Option<PlanningReadSnapshot>, ParentPlanningReadError> {
        Err(ParentPlanningReadError::Unavailable)
    }

    async fn interact(
        &self,
        _id: String,
        _target: Option<String>,
        _action: super::interaction::AgentInteraction,
        _cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<super::interaction::AgentInteractionOutput, String> {
        Err("This host does not support parent-child communication.".into())
    }

    /// Queue a sampling selection for an active directly owned child. The host
    /// owns the actor-admission receipt and must not wait for application.
    async fn set_model(
        &self,
        _subagent_id: String,
        _model: Option<String>,
        _effort: Option<sampling_types::ReasoningEffort>,
        _cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<(), String> {
        Err("This host does not support subagent model control.".into())
    }

    /// Spawn a subagent and await its result.
    ///
    /// For blocking mode the caller awaits the returned future directly.
    /// For background mode the caller spawns a tokio task around this call
    /// and drops the receiver immediately.
    async fn spawn(&self, request: SubagentRequest) -> Result<SubagentResult, ToolError>;

    /// Query the current state of a subagent by ID.
    ///
    /// When `block` is true the backend waits (up to `timeout_ms`) for the
    /// subagent to reach a terminal state before responding. A per-query
    /// transport cap can return a nonterminal snapshot sooner; callers owning
    /// a longer wait budget must continue within their original deadline.
    async fn query(
        &self,
        id: &str,
        block: bool,
        timeout_ms: Option<u64>,
    ) -> Result<Option<SubagentSnapshot>, SubagentObservationError>;

    /// List active children owned by this backend's bound source session.
    async fn list_running(&self) -> Result<Vec<SubagentInspection>, SubagentObservationError> {
        Err(SubagentObservationError::Unavailable)
    }

    /// List basic lifecycle state from the session bound to this backend.
    async fn list_subagents(
        &self,
        _scope: SubagentListScope,
        _deadline: tokio::time::Instant,
    ) -> Result<Vec<SubagentSummary>, SubagentObservationError> {
        Err(SubagentObservationError::Unavailable)
    }

    /// Get structured metadata and status for multiple children without loading answers.
    async fn get_statuses(
        &self,
        _ids: &[String],
        _deadline: tokio::time::Instant,
    ) -> Result<Vec<SubagentDetails>, SubagentObservationError> {
        Err(SubagentObservationError::Unavailable)
    }

    /// Request cancellation of a subagent by ID.
    async fn cancel(&self, id: &str) -> Result<SubagentCancelOutcome, SubagentObservationError>;

    /// Validate a subagent type synchronously before spawning.
    /// Returns `ValidationUnavailable` on channel close / responder drop / timeout.
    async fn validate_type(
        &self,
        subagent_type: &str,
        parent_session_id: &str,
    ) -> SubagentValidateTypeOutcome;
}

const OBSERVATION_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
const QUERY_DEADLINE_ALLOWANCE: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_QUERY_CALLER_DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);
const MAX_CONCURRENT_SUBAGENT_QUERIES: usize = 32;

fn query_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Semaphore::new(MAX_CONCURRENT_SUBAGENT_QUERIES))
}

fn query_deadlines(block: bool, timeout_ms: Option<u64>) -> (std::time::Duration, Option<u64>) {
    if !block {
        return (OBSERVATION_DEADLINE, timeout_ms);
    }
    let coordinator_wait_ms = timeout_ms
        .unwrap_or(OBSERVATION_DEADLINE.as_millis() as u64)
        .min(
            MAX_QUERY_CALLER_DEADLINE
                .saturating_sub(QUERY_DEADLINE_ALLOWANCE)
                .as_millis() as u64,
        );
    let total = std::time::Duration::from_millis(coordinator_wait_ms)
        .saturating_add(QUERY_DEADLINE_ALLOWANCE)
        .min(MAX_QUERY_CALLER_DEADLINE);
    (total, Some(coordinator_wait_ms))
}

async fn race_observation_with_history<T, Runtime, History, IsTerminal>(
    runtime: Runtime,
    history: History,
    history_enabled: bool,
    is_terminal: IsTerminal,
) -> Result<Option<T>, SubagentObservationError>
where
    Runtime: Future<Output = Result<Option<T>, SubagentObservationError>>,
    History: Future<Output = Result<Option<T>, SubagentObservationError>>,
    IsTerminal: Fn(&T) -> bool,
{
    tokio::pin!(runtime, history);
    let first = tokio::select! {
        biased;
        result = &mut runtime => futures::future::Either::Left(result),
        result = &mut history => futures::future::Either::Right(result),
    };

    match first {
        futures::future::Either::Left(Ok(Some(snapshot))) => Ok(Some(snapshot)),
        futures::future::Either::Left(result @ Err(SubagentObservationError::InvalidEvidence)) => {
            result
        }
        futures::future::Either::Left(runtime_result) => {
            resolve_observation(runtime_result, history.await, history_enabled, &is_terminal)
        }
        futures::future::Either::Right(Ok(Some(snapshot))) if is_terminal(&snapshot) => {
            Ok(Some(snapshot))
        }
        futures::future::Either::Right(history_result) => {
            resolve_observation(runtime.await, history_result, history_enabled, &is_terminal)
        }
    }
}

fn resolve_observation<T, IsTerminal>(
    runtime_result: Result<Option<T>, SubagentObservationError>,
    history_result: Result<Option<T>, SubagentObservationError>,
    history_enabled: bool,
    is_terminal: &IsTerminal,
) -> Result<Option<T>, SubagentObservationError>
where
    IsTerminal: Fn(&T) -> bool,
{
    match runtime_result {
        Ok(Some(snapshot)) => Ok(Some(snapshot)),
        Ok(None) if history_enabled => history_result,
        Err(SubagentObservationError::Unauthorized | SubagentObservationError::Unavailable)
            if history_enabled =>
        {
            history_result
        }
        Err(SubagentObservationError::TimedOut) if history_enabled => match history_result {
            Ok(Some(snapshot)) if is_terminal(&snapshot) => Ok(Some(snapshot)),
            _ => Err(SubagentObservationError::TimedOut),
        },
        runtime_result => runtime_result,
    }
}

/// Resource wrapper injected into every session's `Resources`.
///
/// Wraps an `Arc<dyn SubagentBackend>` so the backend can be shared across
/// concurrent tool invocations within the same session.
#[derive(Clone)]
pub struct SubagentBackendResource(pub Arc<dyn SubagentBackend>);

impl SubagentBackendResource {
    pub fn backend(&self) -> &dyn SubagentBackend {
        self.0.as_ref()
    }
}

impl std::fmt::Debug for SubagentBackendResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubagentBackendResource").finish()
    }
}

register_resource!(
    "grow_build",
    "SubagentBackendResource",
    SubagentBackendResource
);

/// In-process channel-based backend for the local host shell.
///
/// Wraps a single `mpsc::UnboundedSender<SubagentEvent>` that carries
/// spawn, query, and cancel messages to the coordinator. The oneshot for
/// `spawn` is created inside the backend so callers never manage it.
#[derive(Clone)]
pub struct ChannelBackend {
    tx: mpsc::UnboundedSender<SubagentEvent>,
    parent_session_id: Option<Arc<str>>,
    history_query: Option<Arc<HistoryQuery>>,
    history_list: Option<Arc<HistoryList>>,
    history_status: Option<Arc<HistoryStatus>>,
}

type HistoryQuery = dyn Fn(
        String,
    ) -> super::coordinator::SendBoxFuture<
        Result<Option<SubagentInspection>, SubagentObservationError>,
    > + Send
    + Sync;
type HistoryList = dyn Fn(SubagentListScope)
        -> super::coordinator::SendBoxFuture<Result<Vec<SubagentSummary>, SubagentObservationError>>
    + Send
    + Sync;
type HistoryStatus = dyn Fn(
        Vec<String>,
    )
        -> super::coordinator::SendBoxFuture<Result<Vec<SubagentDetails>, SubagentObservationError>>
    + Send
    + Sync;

impl ChannelBackend {
    /// Host-only observer after source access has been established from the
    /// owning session's durable facts. Never inject this into model resources.
    pub fn for_authorized_observer(tx: mpsc::UnboundedSender<SubagentEvent>) -> Self {
        Self {
            tx,
            parent_session_id: None,
            history_query: None,
            history_list: None,
            history_status: None,
        }
    }

    #[cfg(test)]
    pub fn new(tx: mpsc::UnboundedSender<SubagentEvent>) -> Self {
        Self::for_authorized_observer(tx)
    }

    /// Bind model-facing operations to one parent session.
    pub fn for_session(
        tx: mpsc::UnboundedSender<SubagentEvent>,
        parent_session_id: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            tx,
            parent_session_id: Some(parent_session_id.into()),
            history_query: None,
            history_list: None,
            history_status: None,
        }
    }

    /// Read the owning session's durable facts when the runtime registry has
    /// no authorized entry. The reader must independently authorize its source.
    pub fn with_history_query<F, Fut>(mut self, query: F) -> Self
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<
                Output = Result<Option<SubagentInspection>, SubagentObservationError>,
            > + Send
            + 'static,
    {
        self.history_query = Some(Arc::new(move |id| Box::pin(query(id))));
        self
    }

    /// Read the bound source's complete durable subagent inventory.
    pub fn with_history_list<F, Fut>(mut self, list: F) -> Self
    where
        F: Fn(SubagentListScope) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Vec<SubagentSummary>, SubagentObservationError>>
            + Send
            + 'static,
    {
        self.history_list = Some(Arc::new(move |scope| Box::pin(list(scope))));
        self
    }

    /// Read requested statuses from the bound source's durable history in one operation.
    pub fn with_history_status<F, Fut>(mut self, status: F) -> Self
    where
        F: Fn(Vec<String>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Vec<SubagentDetails>, SubagentObservationError>>
            + Send
            + 'static,
    {
        self.history_status = Some(Arc::new(move |ids| Box::pin(status(ids))));
        self
    }

    fn parent_session_id(&self) -> Option<String> {
        self.parent_session_id.as_deref().map(str::to_owned)
    }

    pub fn sender(&self) -> mpsc::UnboundedSender<SubagentEvent> {
        self.tx.clone()
    }

    pub fn into_resource(self) -> SubagentBackendResource {
        SubagentBackendResource(Arc::new(self))
    }

    pub async fn cancel_parent_prompt(&self, parent_prompt_id: &str) -> SubagentCancelOutcome {
        let (respond_to, response_rx) = oneshot::channel();
        if self
            .tx
            .send(SubagentEvent::Cancel(SubagentCancelRequest {
                parent_session_id: self.parent_session_id(),
                target: SubagentCancelTarget::ParentPromptId(parent_prompt_id.to_owned()),
                respond_to,
            }))
            .is_err()
        {
            return SubagentCancelOutcome::NotFound;
        }
        response_rx.await.unwrap_or(SubagentCancelOutcome::NotFound)
    }

    /// User Stop: cancel all non-workflow children for this parent session.
    ///
    /// Requires [`Self::for_session`]; unbound backends return `NotFound` and
    /// do not broadcast a wildcard cancel.
    pub async fn cancel_parent_session(&self) -> SubagentCancelOutcome {
        let (respond_to, response_rx) = oneshot::channel();
        if !self.request_cancel_parent_session(respond_to) {
            return SubagentCancelOutcome::NotFound;
        }
        response_rx.await.unwrap_or(SubagentCancelOutcome::NotFound)
    }

    /// Fire-and-forget ParentSession cancel used by the shell Stop path.
    /// Returns false when the backend is unbound or the channel is closed.
    pub fn request_cancel_parent_session(
        &self,
        respond_to: oneshot::Sender<SubagentCancelOutcome>,
    ) -> bool {
        let Some(parent_session_id) = self.parent_session_id() else {
            return false;
        };
        self.tx
            .send(SubagentEvent::Cancel(SubagentCancelRequest {
                parent_session_id: Some(parent_session_id),
                target: SubagentCancelTarget::ParentSession,
                respond_to,
            }))
            .is_ok()
    }

    /// Re-open Task spawns after a prior ParentSession stop (start of next turn).
    pub fn open_spawn_admission(&self) -> bool {
        let Some(parent_session_id) = self.parent_session_id() else {
            return false;
        };
        self.tx
            .send(SubagentEvent::OpenSpawnAdmission { parent_session_id })
            .is_ok()
    }

    pub async fn inspect(
        &self,
        id: &str,
    ) -> Result<Option<SubagentInspection>, SubagentObservationError> {
        let deadline = tokio::time::Instant::now() + OBSERVATION_DEADLINE;
        let runtime = async {
            let _permit = tokio::time::timeout_at(deadline, query_semaphore().acquire())
                .await
                .map_err(|_| SubagentObservationError::TimedOut)?
                .map_err(|_| SubagentObservationError::Unavailable)?;
            let (respond_to, response_rx) = oneshot::channel();
            self.tx
                .send(SubagentEvent::Inspect(SubagentInspectRequest {
                    subagent_id: id.to_owned(),
                    parent_session_id: self.parent_session_id(),
                    respond_to,
                }))
                .map_err(|_| SubagentObservationError::Unavailable)?;
            match tokio::time::timeout_at(deadline, response_rx).await {
                Err(_) => Err(SubagentObservationError::TimedOut),
                Ok(Err(_)) => Err(SubagentObservationError::Unavailable),
                Ok(Ok(result)) => result,
            }
        };
        let history = self.read_history_inspection(id, deadline);
        race_observation_with_history(
            runtime,
            history,
            self.history_query.is_some(),
            |inspection| inspection.snapshot.status.is_terminal(),
        )
        .await
    }

    async fn read_history_inspection(
        &self,
        id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<Option<SubagentInspection>, SubagentObservationError> {
        let Some(query) = &self.history_query else {
            return Err(SubagentObservationError::Unavailable);
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(SubagentObservationError::TimedOut);
        }
        tokio::time::timeout_at(deadline, query(id.to_owned()))
            .await
            .map_err(|_| SubagentObservationError::TimedOut)?
    }

    pub async fn list_running(
        &self,
        parent_session_id: &str,
    ) -> Result<Vec<SubagentInspection>, SubagentObservationError> {
        let deadline = tokio::time::Instant::now() + OBSERVATION_DEADLINE;
        let _permit = tokio::time::timeout_at(deadline, query_semaphore().acquire())
            .await
            .map_err(|_| SubagentObservationError::TimedOut)?
            .map_err(|_| SubagentObservationError::Unavailable)?;
        let (respond_to, response_rx) = oneshot::channel();
        if self
            .tx
            .send(SubagentEvent::ListRunning(SubagentListRunningRequest {
                parent_session_id: parent_session_id.to_owned(),
                respond_to,
            }))
            .is_err()
        {
            return Err(SubagentObservationError::Unavailable);
        }
        match tokio::time::timeout_at(deadline, response_rx).await {
            Err(_) => Err(SubagentObservationError::TimedOut),
            Ok(Err(_)) => Err(SubagentObservationError::Unavailable),
            Ok(Ok(result)) => result,
        }
    }

    pub async fn spawned_refs_for_prompt(
        &self,
        parent_session_id: &str,
        prompt_id: &str,
    ) -> Vec<SpawnedSubagentRef> {
        let (respond_to, response_rx) = oneshot::channel();
        if self
            .tx
            .send(SubagentEvent::SpawnedRefs(SubagentSpawnedRefsRequest {
                parent_session_id: self
                    .parent_session_id
                    .as_deref()
                    .unwrap_or(parent_session_id)
                    .to_owned(),
                prompt_id: prompt_id.to_owned(),
                respond_to,
            }))
            .is_err()
        {
            return Vec::new();
        }
        response_rx.await.unwrap_or_default()
    }

    pub async fn registry_counts(&self) -> SubagentRegistryCounts {
        let (respond_to, response_rx) = oneshot::channel();
        if self
            .tx
            .send(SubagentEvent::RegistryCounts(
                SubagentRegistryCountsRequest { respond_to },
            ))
            .is_err()
        {
            return SubagentRegistryCounts::default();
        }
        response_rx.await.unwrap_or_default()
    }

    /// Spawn while holding the host's interruptible foreground-wait token.
    pub async fn spawn_with_foreground_wait(
        &self,
        request: SubagentRequest,
        wait: Option<&super::types::SubagentForegroundWait>,
    ) -> Result<SubagentResult, ToolError> {
        let _wait = wait.map(super::types::SubagentForegroundWait::enter);
        self.spawn(request).await
    }
}

struct CancelResultReceiverOnDrop {
    cancel_token: tokio_util::sync::CancellationToken,
    armed: bool,
}

impl Drop for CancelResultReceiverOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.cancel_token.cancel();
        }
    }
}

#[async_trait::async_trait]
impl SubagentBackend for ChannelBackend {
    async fn read_parent_plan(
        &self,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<Option<PlanningReadSnapshot>, ParentPlanningReadError> {
        let source_session_id = self
            .parent_session_id()
            .ok_or(ParentPlanningReadError::Unavailable)?;
        if cancellation.is_cancelled() {
            return Err(ParentPlanningReadError::Unavailable);
        }
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(SubagentEvent::ReadParentPlanning(ParentPlanningRequest {
                source_session_id,
                cancellation: cancellation.clone(),
                respond_to,
            }))
            .map_err(|_| ParentPlanningReadError::Unavailable)?;
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(ParentPlanningReadError::Unavailable),
            result = tokio::time::timeout(OBSERVATION_DEADLINE, response) => {
                result
                    .map_err(|_| ParentPlanningReadError::TimedOut)?
                    .map_err(|_| ParentPlanningReadError::Unavailable)?
            }
        }
    }

    async fn set_model(
        &self,
        subagent_id: String,
        model: Option<String>,
        effort: Option<sampling_types::ReasoningEffort>,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<(), String> {
        let source_session_id = self
            .parent_session_id()
            .ok_or("Unbound subagent model-control source")?;
        if cancellation.is_cancelled() {
            return Err("Subagent model control cancelled".into());
        }
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(SubagentEvent::SetModel(
                super::super::set_subagent_model::SubagentModelControlRequest {
                    source_session_id,
                    subagent_id,
                    model,
                    effort,
                    cancellation,
                    respond_to,
                },
            ))
            .map_err(|_| "Subagent coordinator closed before confirming admission")?;
        // Once forwarded, the host owns the admission/cancellation race. Keeping
        // this receiver open lets it report the actual target actor receipt.
        response
            .await
            .map_err(|_| "Subagent model-control endpoint closed before confirming admission")?
    }

    async fn list_running(&self) -> Result<Vec<SubagentInspection>, SubagentObservationError> {
        let Some(parent_session_id) = self.parent_session_id.as_deref() else {
            return Err(SubagentObservationError::Unauthorized);
        };
        ChannelBackend::list_running(self, parent_session_id).await
    }

    async fn list_subagents(
        &self,
        scope: SubagentListScope,
        deadline: tokio::time::Instant,
    ) -> Result<Vec<SubagentSummary>, SubagentObservationError> {
        let Some(parent_session_id) = self.parent_session_id.as_deref() else {
            return Err(SubagentObservationError::Unauthorized);
        };
        let runtime = async {
            let _permit = tokio::time::timeout_at(deadline, query_semaphore().acquire())
                .await
                .map_err(|_| SubagentObservationError::TimedOut)?
                .map_err(|_| SubagentObservationError::Unavailable)?;
            let (respond_to, response_rx) = oneshot::channel();
            self.tx
                .send(SubagentEvent::ListSubagents(
                    super::types::SubagentListRequest {
                        parent_session_id: parent_session_id.to_owned(),
                        scope,
                        respond_to,
                    },
                ))
                .map_err(|_| SubagentObservationError::Unavailable)?;
            tokio::time::timeout_at(deadline, response_rx)
                .await
                .map_err(|_| SubagentObservationError::TimedOut)?
                .map_err(|_| SubagentObservationError::Unavailable)
        };

        let Some(history_list) = &self.history_list else {
            if scope == SubagentListScope::All {
                return Err(SubagentObservationError::Unavailable);
            }
            let mut summaries = runtime.await?;
            summaries.retain(|summary| !summary.status.is_terminal());
            summaries.sort_by(|left, right| left.subagent_id.cmp(&right.subagent_id));
            return Ok(summaries);
        };

        let (runtime, history) = tokio::join!(
            tokio::time::timeout_at(deadline, runtime),
            tokio::time::timeout_at(deadline, history_list(scope)),
        );
        let runtime = runtime.unwrap_or(Err(SubagentObservationError::TimedOut));
        let history = history.unwrap_or(Err(SubagentObservationError::TimedOut));
        let mut summaries = match (runtime, history) {
            (runtime, Ok(history)) => merge_subagent_lists(runtime, history),
            (_, Err(error)) => return Err(error),
        }?;
        if scope == SubagentListScope::Active {
            summaries.retain(|summary| !summary.status.is_terminal());
        }
        summaries.sort_by(|left, right| left.subagent_id.cmp(&right.subagent_id));
        Ok(summaries)
    }

    async fn get_statuses(
        &self,
        ids: &[String],
        deadline: tokio::time::Instant,
    ) -> Result<Vec<SubagentDetails>, SubagentObservationError> {
        let Some(parent_session_id) = self.parent_session_id() else {
            return Err(SubagentObservationError::Unauthorized);
        };
        let _permit = match tokio::time::timeout_at(deadline, query_semaphore().acquire()).await {
            Err(_) => {
                return Ok(ids
                    .iter()
                    .map(|id| unknown_status(id, SubagentObservationError::TimedOut))
                    .collect());
            }
            Ok(Err(_)) => {
                return Ok(ids
                    .iter()
                    .map(|id| unknown_status(id, SubagentObservationError::Unavailable))
                    .collect());
            }
            Ok(Ok(permit)) => permit,
        };
        let mut details = std::collections::HashMap::new();
        let mut pending = Vec::new();
        let mut runtime_errors = std::collections::HashMap::new();

        let (respond_to, response_rx) = oneshot::channel();
        let runtime = self.tx.send(SubagentEvent::GetStatuses(
            super::types::SubagentGetStatusesRequest {
                subagent_ids: ids.to_vec(),
                parent_session_id: Some(parent_session_id),
                respond_to,
            },
        ));
        let runtime = if runtime.is_err() {
            Err(SubagentObservationError::Unavailable)
        } else {
            match tokio::time::timeout_at(deadline, response_rx).await {
                Err(_) => Err(SubagentObservationError::TimedOut),
                Ok(Err(_)) => Err(SubagentObservationError::Unavailable),
                Ok(Ok(results)) => {
                    for (index, id) in ids.iter().enumerate() {
                        match results.get(index) {
                            Some(Ok(Some(detail))) => {
                                details.insert(id.clone(), detail.clone());
                            }
                            Some(Err(SubagentObservationError::InvalidEvidence)) => {
                                details.insert(
                                    id.clone(),
                                    unknown_status(id, SubagentObservationError::InvalidEvidence),
                                );
                            }
                            Some(Err(error)) => {
                                pending.push(id.clone());
                                runtime_errors.insert(id.clone(), *error);
                            }
                            Some(Ok(None)) | None => {
                                pending.push(id.clone());
                                runtime_errors
                                    .insert(id.clone(), SubagentObservationError::Unavailable);
                            }
                        }
                    }
                    Ok(())
                }
            }
        };

        if let Err(error) = runtime {
            if error == SubagentObservationError::InvalidEvidence {
                for id in ids {
                    details.insert(id.clone(), unknown_status(id, error));
                }
            } else {
                pending.extend(ids.iter().cloned());
                for id in ids {
                    runtime_errors.insert(id.clone(), error);
                }
            }
        }

        if !pending.is_empty() {
            if let Some(history_status) = &self.history_status
                && tokio::time::Instant::now() < deadline
            {
                match tokio::time::timeout_at(deadline, history_status(pending.clone())).await {
                    Ok(Ok(history)) => {
                        let pending_ids = pending
                            .iter()
                            .map(String::as_str)
                            .collect::<std::collections::HashSet<_>>();
                        let mut returned = std::collections::HashSet::new();
                        for detail in history {
                            if !pending_ids.contains(detail.subagent_id.as_str()) {
                                continue;
                            }
                            returned.insert(detail.subagent_id.clone());
                            details.insert(detail.subagent_id.clone(), detail);
                        }
                        for id in &pending {
                            if !returned.contains(id) {
                                details.insert(
                                    id.clone(),
                                    unknown_status(id, SubagentObservationError::Unavailable),
                                );
                            }
                        }
                    }
                    Ok(Err(error)) => {
                        for id in &pending {
                            details.insert(id.clone(), unknown_status(id, error));
                        }
                    }
                    Err(_) => {
                        for id in &pending {
                            details.insert(
                                id.clone(),
                                unknown_status(id, SubagentObservationError::TimedOut),
                            );
                        }
                    }
                }
            } else {
                for id in &pending {
                    details.insert(
                        id.clone(),
                        unknown_status(
                            id,
                            runtime_errors
                                .get(id)
                                .copied()
                                .unwrap_or(SubagentObservationError::Unavailable),
                        ),
                    );
                }
            }
        }
        Ok(ids
            .iter()
            .map(|id| {
                details
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| unknown_status(id, SubagentObservationError::Unavailable))
            })
            .collect())
    }

    async fn interact(
        &self,
        id: String,
        target_child_id: Option<String>,
        action: super::interaction::AgentInteraction,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Result<super::interaction::AgentInteractionOutput, String> {
        let source_session_id = self
            .parent_session_id()
            .ok_or("Unbound agent communication source")?;
        let wait_for_host = matches!(
            &action,
            super::interaction::AgentInteraction::Send { .. }
                | super::interaction::AgentInteraction::Ask { .. }
        );
        let (respond_to, response) = oneshot::channel();
        self.tx
            .send(SubagentEvent::Interact(
                super::interaction::AgentInteractionRequest {
                    source_session_id,
                    target_child_id,
                    id,
                    action,
                    receipt_only: false,
                    cancellation: cancellation.clone(),
                    respond_to,
                },
            ))
            .map_err(|_| "Subagent coordinator closed")?;

        // Send and Ask own their cancellation/admission race in the host. They must
        // keep the response receiver alive so a durable ACK that is ready at
        // the same time as cancellation can win, and so the runner can do its
        // bounded read-only receipt check after a lost ACK. Ask must report
        // its actual target outcome after success admission wins cancellation.
        if wait_for_host {
            return response
                .await
                .map_err(|_| "Agent interaction endpoint closed")?;
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err("Agent interaction cancelled".into()),
            result = response => result.map_err(|_| "Agent interaction endpoint closed")?,
        }
    }

    async fn spawn(&self, mut request: SubagentRequest) -> Result<SubagentResult, ToolError> {
        if let Some(parent_session_id) = self.parent_session_id.as_deref() {
            request.parent_session_id = parent_session_id.to_owned();
        }
        let (respond_to, response_rx) = oneshot::channel();
        let cancel_on_receiver_drop = request.owner.is_workflow();
        let cancel_token = request.cancel_token.clone();
        self.tx
            .send(SubagentEvent::Spawn(SubagentSpawnRequest {
                request: Box::new(request),
                result_tx: respond_to,
            }))
            .map_err(|_| {
                ToolError::custom(
                    "channel_closed",
                    "Subagent coordinator channel closed — cannot spawn subagent",
                )
            })?;

        let mut receiver_guard = cancel_on_receiver_drop.then(|| CancelResultReceiverOnDrop {
            cancel_token: cancel_token.clone(),
            armed: true,
        });
        let result = response_rx.await;
        if result.is_ok() {
            if let Some(guard) = receiver_guard.as_mut() {
                guard.armed = false;
            }
        } else if cancel_on_receiver_drop {
            cancel_token.cancel();
        }
        result.map_err(|_| {
            ToolError::custom(
                "channel_closed",
                "Subagent result channel dropped — child session may have crashed",
            )
        })
    }

    async fn query(
        &self,
        id: &str,
        block: bool,
        timeout_ms: Option<u64>,
    ) -> Result<Option<SubagentSnapshot>, SubagentObservationError> {
        let (budget, coordinator_timeout_ms) = query_deadlines(block, timeout_ms);
        let deadline = tokio::time::Instant::now() + budget;
        let runtime = async {
            let _permit = tokio::time::timeout_at(deadline, query_semaphore().acquire())
                .await
                .map_err(|_| SubagentObservationError::TimedOut)?
                .map_err(|_| SubagentObservationError::Unavailable)?;
            let (respond_to, response_rx) = oneshot::channel();
            self.tx
                .send(SubagentEvent::Query(SubagentQueryRequest {
                    subagent_id: id.to_string(),
                    parent_session_id: self.parent_session_id(),
                    block,
                    timeout_ms: coordinator_timeout_ms,
                    respond_to,
                }))
                .map_err(|_| SubagentObservationError::Unavailable)?;
            match tokio::time::timeout_at(deadline, response_rx).await {
                Err(_) => Err(SubagentObservationError::TimedOut),
                Ok(Err(_)) => Err(SubagentObservationError::Unavailable),
                Ok(Ok(result)) => result,
            }
        };
        let history = async {
            self.read_history_inspection(id, deadline)
                .await
                .map(|inspection| inspection.map(|inspection| inspection.snapshot))
        };
        race_observation_with_history(runtime, history, self.history_query.is_some(), |snapshot| {
            snapshot.status.is_terminal()
        })
        .await
    }

    async fn cancel(&self, id: &str) -> Result<SubagentCancelOutcome, SubagentObservationError> {
        let deadline = tokio::time::Instant::now() + OBSERVATION_DEADLINE;
        let (respond_to, response_rx) = oneshot::channel();
        let sent = self.tx.send(SubagentEvent::Cancel(SubagentCancelRequest {
            parent_session_id: self.parent_session_id(),
            target: SubagentCancelTarget::SubagentId(id.to_string()),
            respond_to,
        }));
        if sent.is_err() {
            return Err(SubagentObservationError::Unavailable);
        }
        match tokio::time::timeout_at(deadline, response_rx).await {
            Err(_) => Err(SubagentObservationError::TimedOut),
            Ok(Err(_)) => Err(SubagentObservationError::Unavailable),
            Ok(Ok(SubagentCancelOutcome::Unauthorized)) => {
                Err(SubagentObservationError::Unauthorized)
            }
            Ok(Ok(outcome)) => Ok(outcome),
        }
    }

    async fn validate_type(
        &self,
        subagent_type: &str,
        parent_session_id: &str,
    ) -> SubagentValidateTypeOutcome {
        let parent_session_id = self
            .parent_session_id
            .as_deref()
            .unwrap_or(parent_session_id);
        let (respond_to, response_rx) = oneshot::channel();
        if self
            .tx
            .send(SubagentEvent::ValidateType(SubagentValidateTypeRequest {
                subagent_type: subagent_type.to_string(),
                parent_session_id: parent_session_id.to_string(),
                respond_to,
            }))
            .is_err()
        {
            tracing::warn!(
                subagent_type,
                "coordinator validation channel closed, treating as ValidationUnavailable",
            );
            return SubagentValidateTypeOutcome::ValidationUnavailable;
        }
        let timeout = validate_type_timeout();
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_)) => {
                tracing::warn!(
                    subagent_type,
                    "coordinator validation responder dropped, treating as ValidationUnavailable",
                );
                SubagentValidateTypeOutcome::ValidationUnavailable
            }
            Err(_) => {
                tracing::warn!(
                    subagent_type,
                    timeout_ms = timeout.as_millis() as u64,
                    "coordinator validation timed out, treating as ValidationUnavailable",
                );
                SubagentValidateTypeOutcome::ValidationUnavailable
            }
        }
    }
}

fn merge_subagent_lists(
    runtime: Result<Vec<SubagentSummary>, SubagentObservationError>,
    history: Vec<SubagentSummary>,
) -> Result<Vec<SubagentSummary>, SubagentObservationError> {
    use std::collections::BTreeMap;

    let mut merged = BTreeMap::new();
    for summary in history {
        merged.insert(summary.subagent_id.clone(), summary);
    }
    for summary in runtime.unwrap_or_default() {
        match merged.get(&summary.subagent_id) {
            Some(durable)
                if (durable.status.is_terminal() && !summary.status.is_terminal())
                    || matches!(
                        durable.status,
                        SubagentStatus::ObservationInvalidEvidence | SubagentStatus::Unauthorized
                    ) => {}
            _ => {
                merged.insert(summary.subagent_id.clone(), summary);
            }
        }
    }
    Ok(merged.into_values().collect())
}

fn unknown_status(id: &str, error: SubagentObservationError) -> SubagentDetails {
    let status = match error {
        SubagentObservationError::Unavailable => SubagentStatus::ObservationUnavailable,
        SubagentObservationError::TimedOut => SubagentStatus::ObservationTimedOut,
        SubagentObservationError::InvalidEvidence => SubagentStatus::ObservationInvalidEvidence,
        SubagentObservationError::Unauthorized => SubagentStatus::Unauthorized,
    };
    SubagentDetails::unknown(id.to_owned(), status, error.to_string())
}

/// Default `validate_type` timeout. Override via [`VALIDATE_TYPE_TIMEOUT_ENV_VAR`].
pub const VALIDATE_TYPE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Env-var override for [`VALIDATE_TYPE_TIMEOUT`] (positive milliseconds).
pub const VALIDATE_TYPE_TIMEOUT_ENV_VAR: &str = "GROW_VALIDATE_TYPE_TIMEOUT_MS";

/// Validation timeout, honoring the env-var override.
pub fn validate_type_timeout() -> std::time::Duration {
    let raw = std::env::var(VALIDATE_TYPE_TIMEOUT_ENV_VAR).ok();
    parse_timeout_ms(raw.as_deref())
        .map(std::time::Duration::from_millis)
        .unwrap_or(VALIDATE_TYPE_TIMEOUT)
}

/// Parse a positive `u64` millisecond value; `None` for unset, invalid, or zero.
pub(crate) fn parse_timeout_ms(value: Option<&str>) -> Option<u64> {
    value?.parse::<u64>().ok().filter(|&ms| ms > 0)
}

/// Resolve a `Duration` from a positive-millisecond env override, falling back
/// to `default` when the var is unset / non-numeric / zero.
pub fn env_duration_or(env_var: &str, default: std::time::Duration) -> std::time::Duration {
    parse_timeout_ms(std::env::var(env_var).ok().as_deref())
        .map(std::time::Duration::from_millis)
        .unwrap_or(default)
}

#[cfg(test)]
#[path = "backend_tests.rs"]
mod tests;
