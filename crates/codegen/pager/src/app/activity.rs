//! Read-only projection of session and Goal runtime state for presentation.
//!
//! This module is the one-way seam between lifecycle state and pager chrome.
//! It owns no state, is never serialized, and must never feed commands back
//! into the session or Goal schedulers.

use super::agent_view::AgentView;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentActivityProjection {
    pub foreground_busy: bool,
    /// User work admitted to the local FIFO but not yet owning foreground.
    /// This makes the dashboard row non-idle without inventing motion for a
    /// static queue.
    pub queued_prompts: bool,
    pub needs_input: bool,
    pub replaying: bool,
    pub background_tasks: bool,
    pub scheduled_work: bool,
    pub subagents: bool,
    pub workflows: bool,
    pub goal_active: bool,
}

impl AgentActivityProjection {
    pub fn from_agent(agent: &AgentView) -> Self {
        Self {
            foreground_busy: !agent.session.state.is_idle()
                || agent.session.turn_activity().is_some(),
            queued_prompts: !agent.session.pending_prompts.is_empty(),
            needs_input: !agent.permission_queue.is_empty() || agent.question_view.is_some(),
            replaying: agent.session.loading_replay,
            background_tasks: agent
                .session
                .bg_tasks
                .values()
                .any(|task| task.status == crate::app::agent::BgTaskStatus::Running),
            scheduled_work: !agent.session.scheduled_tasks.is_empty(),
            subagents: agent
                .subagent_sessions
                .values()
                .any(|info| !info.finished && info.workflow_run_id.is_none()),
            workflows: agent.workflow_runs.iter().any(|run| run.is_active()),
            goal_active: agent
                .goal_state
                .as_ref()
                .is_some_and(|goal| goal.status == crate::app::agent::GoalDisplayStatus::Active),
        }
    }

    pub fn working(self) -> bool {
        self.foreground_busy
            || self.queued_prompts
            || self.replaying
            || self.background_tasks
            || self.scheduled_work
            || self.subagents
            || self.workflows
            || self.goal_active
    }

    pub fn animates(self) -> bool {
        self.foreground_busy
            || self.needs_input
            || self.replaying
            || self.background_tasks
            || self.scheduled_work
            || self.subagents
            || self.workflows
            || self.goal_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::agent_view::test_agent_view;

    #[test]
    fn active_goal_projects_work_while_foreground_is_idle() {
        let mut agent = test_agent_view(Some("goal-session"), "/tmp".into());
        agent.goal_state = Some(crate::app::agent::GoalDisplayState::test_stub());
        let projection = AgentActivityProjection::from_agent(&agent);
        assert!(projection.goal_active);
        assert!(projection.working());
    }

    #[test]
    fn queued_prompt_is_work_without_motion() {
        let mut agent = test_agent_view(Some("queued-session"), "/tmp".into());
        agent.session.enqueue_prompt("later".into());
        let projection = AgentActivityProjection::from_agent(&agent);
        assert!(projection.queued_prompts);
        assert!(projection.working());
        assert!(!projection.animates());
    }
}
