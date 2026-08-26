use super::AgentView;
use crate::app::session::{GoalDisplayState, GoalDisplayStatus};
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::SessionEvent;

impl AgentView {
    /// Apply one parsed Goal projection and its durable transition atomically.
    ///
    /// The ACP layer owns wire parsing; this method owns the previous-state
    /// comparison, elapsed-time floor, history event, and detail visibility.
    pub(crate) fn apply_goal_update(&mut self, mut next: GoalDisplayState) -> bool {
        if self.session.last_cleared_goal_id.as_deref() == Some(next.goal_id.as_str()) {
            return false;
        }

        let elapsed_floor_ms = self
            .session
            .goal_state
            .as_ref()
            .filter(|goal| goal.goal_id == next.goal_id)
            .map(|goal| goal.live_elapsed_ms())
            .unwrap_or(0)
            .max(next.elapsed_ms);

        if let Some(event) = goal_transition_event(
            self.session.goal_state.as_ref(),
            &next.goal_id,
            &next.objective,
            next.status,
            elapsed_floor_ms,
        ) {
            self.scrollback
                .push_block(RenderBlock::session_event(event));
        }

        next.received_at = std::time::Instant::now();
        next.elapsed_floor_ms = elapsed_floor_ms;
        self.session.goal_state = Some(next);
        true
    }

    /// Clear the active Goal, record its durable transition, and close detail.
    pub(crate) fn clear_goal(&mut self) -> bool {
        if let Some(goal) = self.session.goal_state.take() {
            self.session.last_cleared_goal_id = Some(goal.goal_id);
            self.scrollback
                .push_block(RenderBlock::session_event(SessionEvent::GoalCleared));
        }
        self.set_goal_detail_visible(false);
        true
    }
}

fn goal_transition_event(
    previous: Option<&GoalDisplayState>,
    goal_id: &str,
    objective: &str,
    status: GoalDisplayStatus,
    elapsed_ms: u64,
) -> Option<SessionEvent> {
    let previous = previous.filter(|goal| goal.goal_id == goal_id);
    let Some(previous) = previous else {
        return match status {
            GoalDisplayStatus::Active => Some(SessionEvent::GoalCreated),
            GoalDisplayStatus::Complete => Some(SessionEvent::GoalCompleted {
                elapsed: std::time::Duration::from_millis(elapsed_ms),
            }),
            _ => None,
        };
    };

    if status == GoalDisplayStatus::Complete && previous.status != GoalDisplayStatus::Complete {
        return Some(SessionEvent::GoalCompleted {
            elapsed: std::time::Duration::from_millis(elapsed_ms),
        });
    }
    if objective != previous.objective {
        return Some(SessionEvent::GoalObjectiveUpdated);
    }
    if status != previous.status {
        return match status {
            GoalDisplayStatus::Active => Some(SessionEvent::GoalRestarted),
            GoalDisplayStatus::Paused => Some(SessionEvent::GoalPaused),
            GoalDisplayStatus::Blocked => Some(SessionEvent::GoalBlocked),
            GoalDisplayStatus::BudgetLimited => Some(SessionEvent::GoalBudgetLimited),
            GoalDisplayStatus::Complete => None,
        };
    }
    None
}
