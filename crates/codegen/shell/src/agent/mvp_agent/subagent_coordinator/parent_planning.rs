//! Read the already resolved direct parent's canonical planning snapshot.

use crate::session::SessionHandle;
use tool_types::planning::{PlanDelegation, PlanningReadSnapshot};
use tools::implementations::grow_build::task::parent_planning::{
    ParentPlanningReadError, ParentPlanningRequest,
};

pub(super) async fn run(
    request: ParentPlanningRequest,
    source: Option<SessionHandle>,
    parent: Option<SessionHandle>,
    parent_session_id: String,
    task_id: String,
    parent_step_id: Option<String>,
    deadline: tokio::time::Instant,
) {
    let ParentPlanningRequest {
        source_session_id,
        cancellation,
        mut respond_to,
    } = request;
    if respond_to.is_closed() {
        return;
    }
    let (Some(source), Some(parent)) = (source, parent) else {
        let _ = respond_to.send(Err(ParentPlanningReadError::Unavailable));
        return;
    };
    if !available(&source) || !available(&parent) {
        let _ = respond_to.send(Err(ParentPlanningReadError::Unavailable));
        return;
    }

    // This deadline includes time queued on the LocalSet. Dropping this read
    // never cancels either session or the child's execution token.
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(ParentPlanningReadError::Unavailable),
        _ = tokio::time::sleep_until(deadline) => Err(ParentPlanningReadError::TimedOut),
        _ = respond_to.closed() => return,
        _ = source.cmd_tx.closed() => Err(ParentPlanningReadError::Unavailable),
        _ = parent.cmd_tx.closed() => Err(ParentPlanningReadError::Unavailable),
        snapshot = parent.chat_state_handle.planning_state() => {
            snapshot.map_err(|_| ParentPlanningReadError::Unavailable)
        }
    };
    let result = result.and_then(|snapshot| {
        if !available(&source) || !available(&parent) {
            return Err(ParentPlanningReadError::Unavailable);
        }
        let Some(snapshot) = snapshot else {
            return if parent_step_id.is_some() {
                Err(ParentPlanningReadError::InvalidEvidence)
            } else {
                Ok(None)
            };
        };
        let plan = snapshot.plan;
        if plan.session_id != parent_session_id {
            return Err(ParentPlanningReadError::InvalidEvidence);
        }
        let delegation = if let Some(parent_step_id) = parent_step_id {
            if !plan.links.iter().any(|link| {
                link.id == task_id
                    && link.child_session_id == source_session_id
                    && link.parent_step_id == parent_step_id
            }) {
                return Err(ParentPlanningReadError::InvalidEvidence);
            }
            Some(PlanDelegation {
                parent_session_id,
                parent_scope_id: plan.scope_id.clone(),
                parent_step_id,
                task_id,
                child_session_id: source_session_id,
            })
        } else {
            None
        };
        Ok(Some(PlanningReadSnapshot {
            timeline_seq: snapshot.seq.get(),
            plan,
            delegation,
        }))
    });
    let _ = respond_to.send(result);
}

fn available(handle: &SessionHandle) -> bool {
    !handle.cmd_tx.is_closed() && !handle.chat_state_handle.is_closed()
}

#[cfg(test)]
#[path = "parent_planning_tests.rs"]
mod tests;
