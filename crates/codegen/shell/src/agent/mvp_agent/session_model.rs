//! Admit coordinator-authorized sampling changes for one exact child actor.

use super::*;
use tools::implementations::grow_build::set_subagent_model::SubagentModelControlRequest;

#[cfg(test)]
thread_local! {
    static CATALOG_WAIT_FOR_TEST: RefCell<Option<oneshot::Sender<()>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn observe_next_catalog_wait() -> oneshot::Receiver<()> {
    let (sender, receiver) = oneshot::channel();
    CATALOG_WAIT_FOR_TEST.with(|hook| {
        assert!(hook.borrow_mut().replace(sender).is_none());
    });
    receiver
}

impl MvpAgent {
    pub(super) fn set_subagent_model(
        &self,
        mut request: SubagentModelControlRequest,
        target: SessionHandle,
        target_cancellation: CancellationToken,
    ) {
        let agent = LocalRef::new(self);
        self.spawn_owned_local(async move {
            let result = tokio::select! {
                biased;
                _ = request.respond_to.closed() => return,
                _ = request.cancellation.cancelled() => Err("subagent model control was cancelled".into()),
                _ = target_cancellation.cancelled() => Err("the target subagent is no longer running".into()),
                result = agent.get().admit_subagent_model_tool(
                    &target,
                    request.model,
                    request.effort,
                    &request.cancellation,
                    &target_cancellation,
                ) => result,
            };
            let _ = request.respond_to.send(result);
        });
    }

    fn check_subagent_model_target(
        &self,
        target: &SessionHandle,
        cancellation: &CancellationToken,
        target_cancellation: &CancellationToken,
    ) -> Result<(), String> {
        if cancellation.is_cancelled() {
            return Err("subagent model control was cancelled".into());
        }
        if target_cancellation.is_cancelled() {
            return Err("the target subagent is no longer running".into());
        }
        let children = self.active_child_sessions.borrow();
        let current = children
            .get(&target.info.id)
            .ok_or_else(|| "the target subagent is no longer running".to_string())?;
        if !current.cmd_tx.same_channel(&target.cmd_tx) {
            return Err("the target subagent incarnation has been replaced".into());
        }
        // Completion publishes Settling before delivering the child Turn
        // result. The control route can outlive execution during finalization.
        if matches!(
            target.runtime_observation.read().lifecycle_status(),
            crate::session::prompt_queue::PromptStatus::Terminal { .. }
                | crate::session::prompt_queue::PromptStatus::Settling
                | crate::session::prompt_queue::PromptStatus::Unavailable { .. }
        ) {
            return Err("the target subagent execution is no longer open".into());
        }
        Ok(())
    }

    async fn admit_subagent_model_tool(
        &self,
        target: &SessionHandle,
        model: Option<String>,
        effort: Option<sampling_types::ReasoningEffort>,
        cancellation: &CancellationToken,
        target_cancellation: &CancellationToken,
    ) -> Result<(), String> {
        // The coordinator grants this exact runtime, never a later actor
        // discovered by session ID. Recheck after asynchronous catalog waiting.
        self.check_subagent_model_target(target, cancellation, target_cancellation)?;
        #[cfg(test)]
        CATALOG_WAIT_FOR_TEST.with(|hook| {
            if let Some(waiting) = hook.borrow_mut().take() {
                let _ = waiting.send(());
            }
        });
        let catalog_transaction = self.model_reload_lock.lock().await;
        self.check_subagent_model_target(target, cancellation, target_cancellation)?;
        let (admitted, admission) = oneshot::channel();
        let enqueued = crate::agent::handlers::model_switch::enqueue_tool(
            self,
            &catalog_transaction,
            target.clone(),
            model,
            effort,
            admitted,
        )
        .map_err(|error| error.to_string())?;
        drop(catalog_transaction);
        let agent = LocalRef::new(self);
        let session_id = target.info.id.clone();
        // Application keeps the existing finish owner. The parent tool waits
        // only for admission, so the child's current Step can finish normally.
        self.spawn_owned_local(async move {
            match crate::agent::handlers::model_switch::finish(agent.get(), enqueued).await {
                Ok(_) => {
                    agent.get().model_unavailable_sessions.borrow_mut().remove(session_id.0.as_ref());
                }
                Err(error) => {
                    tracing::warn!(session_id = %session_id.0, %error, "queued child model control did not apply");
                }
            }
        });
        admission.await.map_err(|_| {
            "subagent actor closed before confirming model-control admission".to_string()
        })?
    }
}
