//! Session teardown helpers.
use super::*;

/// Best-effort removal of this session's per-session scratch staging on
/// teardown. A no-op in builds without a scratch producer.
pub(super) fn cleanup_session_scratch(_session: &SessionActor) {}

pub(super) async fn shutdown_workflows(session: &SessionActor) {
    session.checkpoint_goal_before_shutdown().await;
    if let Err(run_ids) = session
        .workflow_manager
        .lock()
        .await
        .cancel_all_and_drain(std::time::Duration::from_secs(7))
        .await
    {
        tracing::warn!(
            ?run_ids,
            "workflow shutdown completed with interrupted runs"
        );
    }
    let (respond_to, ack) = tokio::sync::oneshot::channel();
    if session
        .notifications
        .persistence_tx
        .send(PersistenceMsg::FlushAndAck { respond_to })
        .is_err()
    {
        tracing::warn!("workflow shutdown persistence channel closed before flush");
        return;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), ack).await {
        Ok(Ok(())) => {}
        Ok(Err(_)) => {
            tracing::warn!("workflow shutdown persistence actor dropped flush ack")
        }
        Err(_) => tracing::warn!("workflow shutdown persistence flush timed out"),
    }
}

/// Close the primary-owned permission authority at teardown entry and wait
/// until its final audit event has crossed the bridge. End hooks and memory
/// work may take time; no child permission request remains live while they run.
pub(super) async fn stop_permission_manager_and_drain_audit(session: &SessionActor) {
    if session.owns_permission_manager {
        session.permissions.shutdown_and_drain().await;
        let bridge = session.permission_audit_bridge.lock().take();
        if let Some(bridge) = bridge
            && let Err(error) = bridge.await
        {
            tracing::warn!(%error, "permission audit bridge failed during session shutdown");
        }
    }
}

/// Cross the final persistence barrier after every teardown producer,
/// including the drained permission bridge and session-end hooks, has stopped.
pub(super) async fn final_session_persistence_flush(session: &SessionActor) {
    let (respond_to, ack) = tokio::sync::oneshot::channel();
    if session
        .notifications
        .persistence_tx
        .send(PersistenceMsg::FlushAndAck { respond_to })
        .is_err()
    {
        tracing::warn!("permission audit persistence channel closed before final flush");
        return;
    }
    if ack.await.is_err() {
        tracing::warn!("permission audit persistence actor dropped flush ack");
    }
}
