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
use tokio_util::sync::CancellationToken;

use crate::commands::SamplerCommand;
use crate::config::{RetryPolicy, SamplerConfig};
use crate::events::SamplingEvent;
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
    state: ActorState,
    /// Per-request tasks. The actor's run loop selects on
    /// `cmd_rx.recv()` and `tasks.join_next()`; when a task finishes
    /// it returns its `RequestId` so the actor can clean up
    /// `active_requests`.
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
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let handle = SamplerHandle::new(cmd_tx);
        let actor = Self {
            cmd_rx,
            event_tx,
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
                Some(joined) = self.tasks.join_next(), if !self.tasks.is_empty() => {
                    match joined {
                        Ok(request_id) => {
                            // Task finished normally; remove from
                            // active set unless the user has already
                            // cancelled it (Cancel removes it too).
                            self.state.remove(&request_id);
                        }
                        Err(join_err) => {
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

        // Cancel any still-running tasks before exiting so they don't
        // leak. The cancellation token shutdown is best-effort.
        for (_, active) in self.state.active_requests.drain() {
            active.cancel_token.cancel();
        }
        self.tasks.shutdown().await;
    }

    fn handle_command(&mut self, cmd: SamplerCommand) {
        match cmd {
            SamplerCommand::Shutdown => unreachable!("shutdown is handled by the run loop"),
            SamplerCommand::Submit {
                request_id,
                request,
                config,
                completion_tx,
                scope_capture,
                usage_sink,
            } => {
                let cancel_token = CancellationToken::new();
                let active = ActiveRequest {
                    cancel_token: cancel_token.clone(),
                };
                if let Some(prev) = self.state.register(request_id.clone(), active) {
                    // Caller submitted a duplicate id; cancel the
                    // previous one so we don't leak its task.
                    prev.cancel_token.cancel();
                }
                let effective_config = config
                    .map(|b| *b)
                    .unwrap_or_else(|| self.state.config.clone());
                let event_tx = self.event_tx.clone();
                let retry_policy = self.state.retry_policy.clone();
                let request_inner = *request;
                self.tasks.spawn(request_task::run_request_task(
                    request_id,
                    request_inner,
                    effective_config,
                    retry_policy,
                    event_tx,
                    cancel_token,
                    completion_tx,
                    scope_capture,
                    usage_sink,
                ));
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
