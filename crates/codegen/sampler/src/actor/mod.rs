//! Sampler actor: owns global state, spawns per-request tasks.
//!
//! The actor task itself is single-threaded -- it processes one
//! command at a time -- but it spawns `tokio::spawn` per-request
//! tasks for the actual streaming work, so multiple requests can be
//! in flight concurrently.

pub(crate) mod request_task;
pub(crate) mod state;

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::commands::SamplerCommand;
use crate::config::{RetryPolicy, SamplerConfig};
use crate::events::{PreviewEventBudget, SamplingEvent};
use crate::handle::SamplerHandle;
use state::{ActiveRequest, ActorState};

use crate::types::RequestId;

/// Sampler actor.
///
/// Construct via [`SamplerActor::spawn`]; the returned
/// [`SamplerHandle`] is the only supported way to interact with it.
pub struct SamplerActor {
    cmd_rx: mpsc::UnboundedReceiver<SamplerCommand>,
    event_tx: mpsc::UnboundedSender<SamplingEvent>,
    preview_budget: Option<PreviewEventBudget>,
    state: ActorState,
    /// Per-request tasks, including replaced submissions that are still
    /// settling. JoinSet task ids identify the exact registration to clean up.
    tasks: JoinSet<RequestId>,
}

/// Sole join owner for a spawned sampler actor.
pub struct SamplerOwner {
    handle: SamplerHandle,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl SamplerOwner {
    pub fn handle(&self) -> SamplerHandle {
        self.handle.clone()
    }

    pub async fn shutdown(&mut self) -> Result<(), tokio::task::JoinError> {
        self.handle.close();
        match self.task.take() {
            Some(task) => task.await,
            None => Ok(()),
        }
    }

    pub async fn abort_and_join(&mut self) -> Result<(), tokio::task::JoinError> {
        let Some(task) = self.task.take() else {
            return Ok(());
        };
        task.abort();
        match task.await {
            Ok(()) => Ok(()),
            Err(error) if error.is_cancelled() => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Close admission and join within `timeout`; on timeout, abort and still
    /// observe the actor terminal so no detached owner survives teardown.
    pub async fn shutdown_bounded(&mut self, timeout: std::time::Duration) -> Result<(), String> {
        self.handle.close();
        let Some(mut task) = self.task.take() else {
            return Ok(());
        };
        match tokio::time::timeout(timeout, &mut task).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(format!("sampler actor failed: {error}")),
            Err(_) => {
                task.abort();
                match task.await {
                    Ok(()) => Err("sampler actor shutdown timed out".into()),
                    Err(error) if error.is_cancelled() => {
                        Err("sampler actor shutdown timed out".into())
                    }
                    Err(error) => Err(format!(
                        "sampler actor shutdown timed out and abort failed: {error}"
                    )),
                }
            }
        }
    }
}

impl SamplerActor {
    /// Spawn the actor on the current tokio runtime and return a
    /// handle. The actor stops when the returned handle (and all its
    /// clones) are dropped.
    pub fn spawn(
        config: SamplerConfig,
        retry_policy: RetryPolicy,
        event_tx: mpsc::UnboundedSender<SamplingEvent>,
    ) -> SamplerHandle {
        let mut owner = Self::spawn_owned(config, retry_policy, event_tx);
        let handle = owner.handle();
        drop(owner.task.take());
        handle
    }

    /// Spawn an actor with an explicit shutdown and join owner.
    pub fn spawn_owned(
        config: SamplerConfig,
        retry_policy: RetryPolicy,
        event_tx: mpsc::UnboundedSender<SamplingEvent>,
    ) -> SamplerOwner {
        Self::spawn_owned_inner(config, retry_policy, event_tx, None)
    }

    /// Spawn a session sampler whose fine-grained preview events apply
    /// backpressure when its client/persistence drainer falls behind.
    pub fn spawn_owned_with_preview_budget(
        config: SamplerConfig,
        retry_policy: RetryPolicy,
        event_tx: mpsc::UnboundedSender<SamplingEvent>,
        preview_budget: PreviewEventBudget,
    ) -> SamplerOwner {
        Self::spawn_owned_inner(config, retry_policy, event_tx, Some(preview_budget))
    }

    fn spawn_owned_inner(
        config: SamplerConfig,
        retry_policy: RetryPolicy,
        event_tx: mpsc::UnboundedSender<SamplingEvent>,
        preview_budget: Option<PreviewEventBudget>,
    ) -> SamplerOwner {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let handle = SamplerHandle::new(cmd_tx);
        let actor = Self {
            cmd_rx,
            event_tx,
            preview_budget,
            state: ActorState::new(config, retry_policy),
            tasks: JoinSet::new(),
        };
        let task = tokio::spawn(actor.run());
        SamplerOwner {
            handle,
            task: Some(task),
        }
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                biased;
                // Prefer cleaning up finished tasks before processing
                // new commands -- prevents `active_requests` from
                // staying stale longer than necessary.
                Some(joined) = self.tasks.join_next_with_id(), if !self.tasks.is_empty() => {
                    match joined {
                        Ok((task_id, _)) => self.state.remove_task(task_id),
                        Err(join_err) => {
                            self.state.remove_task(join_err.id());
                            tracing::warn!(
                                error = %join_err,
                                "request task panicked or was aborted"
                            );
                        }
                    }
                }
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(SamplerCommand::Shutdown) => break,
                        Some(cmd) => self.handle_command(cmd),
                        None => break, // all handles dropped
                    }
                }
            }
        }

        // Stop provider work cooperatively, then keep the JoinSet alive until
        // every admitted request has completed its evidence and usage
        // settlement. `JoinSet::shutdown` aborts tasks, which would cut off
        // those independent acknowledgment barriers; the owner deadline is
        // the only forced-abort boundary.
        for (_, active) in self.state.active_requests.drain() {
            active.cancel_token.cancel();
        }
        while let Some(joined) = self.tasks.join_next_with_id().await {
            match joined {
                Ok((task_id, _)) => self.state.remove_task(task_id),
                Err(join_err) => {
                    self.state.remove_task(join_err.id());
                    tracing::warn!(
                        error = %join_err,
                        "request task panicked or was aborted during sampler shutdown"
                    );
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: SamplerCommand) {
        match cmd {
            SamplerCommand::Shutdown => unreachable!("shutdown is handled by the run loop"),
            SamplerCommand::Submit {
                request_id,
                cancel_token,
                request,
                config,
                completion_tx,
                scope_capture,
                usage_sink,
                evidence_sink,
                recovery,
            } => {
                let effective_config = config
                    .map(|b| *b)
                    .unwrap_or_else(|| self.state.config.clone());
                let event_tx = self.event_tx.clone();
                let preview_budget = self.preview_budget.clone();
                let retry_policy = self.state.retry_policy.clone();
                let request_inner = *request;
                let task_id = self
                    .tasks
                    .spawn(request_task::run_request_task(
                        request_id.clone(),
                        request_inner,
                        effective_config,
                        retry_policy,
                        event_tx,
                        cancel_token.clone(),
                        completion_tx,
                        scope_capture,
                        usage_sink,
                        evidence_sink,
                        recovery,
                        preview_budget,
                    ))
                    .id();
                let active = ActiveRequest {
                    task_id,
                    cancel_token,
                };
                if let Some(prev) = self.state.register(request_id, active) {
                    // The replaced task stays tracked through its settlement.
                    prev.cancel_token.cancel();
                }
            }
            SamplerCommand::Cancel { request_id } => {
                self.state.cancel(&request_id);
            }
            SamplerCommand::UpdateConfig { config } => {
                self.state.update_config(*config);
            }
            SamplerCommand::IsActive { request_id, reply } => {
                let _ = reply.send(self.state.active_requests.contains_key(&request_id));
            }
            SamplerCommand::ActiveCount { reply } => {
                let _ = reply.send(self.state.active_requests.len());
            }
        }
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;

    fn owner_for(task: tokio::task::JoinHandle<()>) -> SamplerOwner {
        let (cmd_tx, _cmd_rx) = mpsc::unbounded_channel();
        SamplerOwner {
            handle: SamplerHandle::new(cmd_tx),
            task: Some(task),
        }
    }

    #[tokio::test]
    async fn bounded_shutdown_aborts_and_observes_hung_owner() {
        let mut owner = owner_for(tokio::spawn(std::future::pending()));

        let error = owner
            .shutdown_bounded(std::time::Duration::from_millis(10))
            .await
            .expect_err("hung owner must fail the shutdown barrier");

        assert!(error.contains("timed out"));
        assert!(owner.task.is_none(), "timed-out task must not be detached");
    }

    #[tokio::test]
    async fn bounded_shutdown_observes_panicked_owner() {
        let mut owner = owner_for(tokio::spawn(async { panic!("sampler owner panic") }));

        let error = owner
            .shutdown_bounded(std::time::Duration::from_secs(1))
            .await
            .expect_err("panicked owner must fail the shutdown barrier");

        assert!(error.contains("failed"));
        assert!(owner.task.is_none(), "panicked task must be observed");
    }
}
