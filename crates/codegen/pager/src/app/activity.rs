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
        let root_session_id = agent.session.session_id.as_ref().map(|id| id.0.as_ref());
        let root_permission_pending = root_session_id.is_some_and(|root| {
            agent
                .permission_queue
                .iter()
                .any(|permission| permission.request.request.session_id.0.as_ref() == root)
        });
        let root_question_pending = agent.question_view.as_ref().is_some_and(|question| {
            question.source_session_id.is_none()
                || question.source_session_id.as_deref() == root_session_id
        });
        // A question parked on a non-finished child view (its own
        // `question_view`, not hoisted onto the parent) still demands user
        // input, so the dashboard NeedsInput rows light up while the child
        // waits in the background.
        let child_question_pending = agent.subagent_views.iter().any(|(child_sid, child)| {
            child.question_view.as_ref().is_some_and(|question| {
                question.source_session_id.as_deref() == Some(child_sid.as_str())
            }) && agent
                .session
                .subagent_sessions
                .get(child_sid)
                .is_some_and(|info| !info.finished)
        });
        Self {
            foreground_busy: !agent.session.state.is_idle()
                || agent.session.turn_activity().is_some(),
            queued_prompts: !agent.session.pending_prompts.is_empty(),
            needs_input: root_permission_pending || root_question_pending || child_question_pending,
            replaying: agent.session.loading_replay,
            background_tasks: agent
                .session
                .bg_tasks
                .values()
                .any(|task| task.status == crate::app::agent::BgTaskStatus::Running),
            scheduled_work: !agent.session.scheduled_tasks.is_empty(),
            subagents: agent
                .session
                .subagent_sessions
                .values()
                .any(|info| !info.finished && info.workflow_run_id.is_none()),
            // Active private runs (deep research) share the `workflows` flag:
            // this projection only drives motion/working chrome, never any
            // management surface (those iterate `workflow_runs` directly).
            workflows: agent
                .session
                .workflow_runs
                .iter()
                .any(|run| run.is_active())
                || agent
                    .session
                    .private_workflow_runs
                    .iter()
                    .any(|run| run.is_active()),
            goal_active: agent
                .session
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
        agent.session.goal_state = Some(crate::app::agent::GoalDisplayState::test_stub());
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

    #[test]
    fn active_private_workflow_projects_work_and_motion() {
        let mut agent = test_agent_view(Some("private-wf-session"), "/tmp".into());
        agent
            .session
            .private_workflow_runs
            .push(crate::app::agent::WorkflowRunSnapshot {
                run_id: "wf_private".into(),
                definition_id: None,
                definition_scope: None,
                definition_hash: None,
                name: "deep-research".into(),
                objective: "investigate".into(),
                status: "active".into(),
                management_available: false,
                builtin: false,
                phases: vec![("Research".into(), "active".into())],
                current_phase: Some("Research".into()),
                agents: vec![crate::app::agent::WorkflowAgentRowView {
                    agent_id: "a1".into(),
                    label: "researcher-0".into(),
                    phase: Some("Research".into()),
                    model: None,
                    state: "running".into(),
                    tokens_used: 0,
                    duration_ms: 0,
                }],
                agent_budget: None,
                agents_used: 0,
                agents_remaining: None,
                agent_usage_incomplete: false,
                active_agents: 1,
                elapsed_ms: 1_000,
                received_at: std::time::Instant::now(),
                pause_message: None,
                result_summary: None,
            });
        let projection = AgentActivityProjection::from_agent(&agent);
        assert!(
            projection.workflows,
            "an active private run must set the workflows projection flag"
        );
        assert!(projection.working());
        assert!(projection.animates());
        assert!(
            agent.session.workflow_runs.is_empty(),
            "the private run must stay out of the public workflow_runs list"
        );
    }

    #[test]
    fn settled_private_workflow_does_not_project_work() {
        let mut agent = test_agent_view(Some("settled-wf-session"), "/tmp".into());
        agent
            .session
            .private_workflow_runs
            .push(crate::app::agent::WorkflowRunSnapshot {
                run_id: "wf_private".into(),
                definition_id: None,
                definition_scope: None,
                definition_hash: None,
                name: "deep-research".into(),
                objective: "investigate".into(),
                status: "complete".into(),
                management_available: false,
                builtin: false,
                phases: Vec::new(),
                current_phase: None,
                agents: Vec::new(),
                agent_budget: None,
                agents_used: 0,
                agents_remaining: None,
                agent_usage_incomplete: false,
                active_agents: 0,
                elapsed_ms: 1_000,
                received_at: std::time::Instant::now(),
                pause_message: None,
                result_summary: None,
            });
        let projection = AgentActivityProjection::from_agent(&agent);
        assert!(!projection.workflows);
        assert!(!projection.working());
        assert!(!projection.animates());
    }

    fn make_subagent_info(finished: bool) -> crate::app::subagent::SubagentInfo {
        crate::app::subagent::SubagentInfo {
            subagent_id: std::sync::Arc::from("sa-child-1"),
            child_session_id: std::sync::Arc::from("child-1"),
            description: std::sync::Arc::from("test child"),
            subagent_type: std::sync::Arc::from("general-purpose"),
            model: None,
            context_source: None,
            resumed_from: None,
            capability_mode: None,
            permission_mode: None,
            effective_permission_mode: None,
            workflow_run_id: None,
            context_normalized: false,
            parent_prompt_id: None,
            started_at: std::time::Instant::now(),
            last_progress_at: std::time::Instant::now(),
            finished,
            status: None,
            error: None,
            duration_ms: None,
            tool_calls: None,
            turns: None,
            turn_count: None,
            tool_call_count: None,
            tokens_used: None,
            context_window_tokens: None,
            context_usage_pct: None,
            tools_used: Vec::new(),
            error_count: None,
            activity_label: None,
            is_background: false,
            pending_kill: false,
            kill_requested_at: None,
            scrollback_entry_id: None,
            prompt: None,
            child_cwd: None,
            worktree_path: None,
            child_updates_replayed: false,
        }
    }

    /// Parent with one child view keyed `child-1`; optionally parks an
    /// ACP-style question on the child's own `question_view`.
    fn parent_with_child_question(finished: bool, park_question: bool) -> AgentView {
        let mut parent = test_agent_view(Some("parent-sess"), "/tmp".into());
        let mut child = test_agent_view(Some("child-1"), "/tmp".into());
        if park_question {
            let stashed = child.prompt.stash();
            child.question_view = Some(crate::views::question_view::QuestionViewState::with_response_tx(
                Some("child-1".into()),
                "call-q".into(),
                vec![],
                stashed,
                None,
                tools::implementations::grow_build::ask_user_question::AskUserQuestionMode::Default,
            ));
        }
        parent
            .subagent_views
            .insert("child-1".into(), Box::new(child));
        parent
            .session
            .subagent_sessions
            .insert("child-1".into(), make_subagent_info(finished));
        parent
    }

    #[test]
    fn child_pending_question_projects_needs_input() {
        let parent = parent_with_child_question(false, true);
        let projection = AgentActivityProjection::from_agent(&parent);
        assert!(
            projection.needs_input,
            "a non-finished child with a parked question must set needs_input"
        );
        assert!(
            parent.question_view.is_none(),
            "the child question must not hoist onto the parent view"
        );
    }

    #[test]
    fn finished_child_question_does_not_project_needs_input() {
        let parent = parent_with_child_question(true, true);
        let projection = AgentActivityProjection::from_agent(&parent);
        assert!(
            !projection.needs_input,
            "a finished child must not contribute needs_input"
        );
    }

    #[test]
    fn child_without_question_does_not_project_needs_input() {
        let parent = parent_with_child_question(false, false);
        let projection = AgentActivityProjection::from_agent(&parent);
        assert!(
            !projection.needs_input,
            "a non-finished child with no question must not set needs_input"
        );
    }
}
