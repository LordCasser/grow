use super::AgentView;
use crate::app::session::GoalDisplayState;

impl AgentView {
    /// State projection only. Command results and autonomous lifecycle facts
    /// have their own durable notices; hydration must not invent either.
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

        next.received_at = std::time::Instant::now();
        next.elapsed_floor_ms = elapsed_floor_ms;
        self.session.goal_state = Some(next);
        true
    }

    /// Clear the Goal projection and close detail. The durable command notice
    /// owns the visible confirmation; a state snapshot must not echo it again.
    pub(crate) fn clear_goal(&mut self) -> bool {
        if let Some(goal) = self.session.goal_state.take() {
            self.session.last_cleared_goal_id = Some(goal.goal_id);
        }
        self.set_goal_detail_visible(false);
        true
    }
}
