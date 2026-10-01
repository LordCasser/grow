//! Pure display state for the normal status/task widgets. Only delivered
//! transcript facts enter these maps; no AgentSession or execution handles.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::app::session::{
    BgTaskState, BgTaskStatus, GoalDisplayState, GoalDisplayStatus, ScheduledTaskInfo,
    WorkflowAgentRowView, WorkflowRunSnapshot,
};
use crate::app::subagent::SubagentInfo;
use crate::motion::FrameStamp;
use shell::extensions::notification::SessionUpdate as GrowUpdate;

pub(crate) struct RecordedSurfaces {
    pub bg_tasks: BTreeMap<String, BgTaskState>,
    pub subagents: HashMap<String, SubagentInfo>,
    pub scheduled: HashMap<String, ScheduledTaskInfo>,
    pub workflows: Vec<WorkflowRunSnapshot>,
    pub goal: Option<GoalDisplayState>,
    pub total_tokens: Option<u64>,
    pub turn_started_ms: Option<u64>,
    pub turn_active: bool,
    origin: Instant,
    first_ms: Option<u64>,
    last_ms: Option<u64>,
}

impl Default for RecordedSurfaces {
    fn default() -> Self {
        Self {
            bg_tasks: BTreeMap::new(),
            subagents: HashMap::new(),
            scheduled: HashMap::new(),
            workflows: Vec::new(),
            goal: None,
            total_tokens: None,
            turn_started_ms: None,
            turn_active: false,
            origin: Instant::now(),
            first_ms: None,
            last_ms: None,
        }
    }
}

impl RecordedSurfaces {
    pub fn observe_time(&mut self, at: Option<u64>) {
        if let Some(at) = at {
            self.first_ms.get_or_insert(at);
            self.last_ms = Some(self.last_ms.map_or(at, |last| last.max(at)));
        }
    }

    pub fn frame(&self, source_ms: Option<u64>, finished: bool) -> FrameStamp {
        let at = if finished {
            self.last_ms
        } else {
            source_ms.or(self.last_ms)
        };
        let elapsed = Duration::from_millis(
            at.zip(self.first_ms)
                .map_or(0, |(at, first)| at.saturating_sub(first)),
        );
        let wall = at
            .and_then(|at| UNIX_EPOCH.checked_add(Duration::from_millis(at)))
            .unwrap_or(UNIX_EPOCH);
        FrameStamp::at_virtual(self.origin, elapsed, wall)
    }

    pub fn apply(&mut self, update: &GrowUpdate) {
        let frame = self.frame(self.last_ms, false);
        let now = frame.now();
        match update {
            GrowUpdate::SubagentSpawned {
                subagent_id,
                child_session_id,
                description,
                subagent_type,
                model,
                reasoning_effort,
                effective_context_source,
                resumed_from,
                capability_mode,
                permission_mode,
                effective_permission_mode,
                workflow_run_id,
                context_normalized,
                parent_prompt_id,
                ..
            } => {
                self.subagents
                    .entry(child_session_id.clone())
                    .or_insert_with(|| SubagentInfo {
                        subagent_id: Arc::from(subagent_id.as_str()),
                        child_session_id: Arc::from(child_session_id.as_str()),
                        description: Arc::from(description.as_str()),
                        subagent_type: Arc::from(subagent_type.as_str()),
                        model: model.as_deref().map(Arc::from),
                        reasoning_effort: reasoning_effort
                            .as_deref()
                            .and_then(|effort| effort.parse().ok()),
                        context_source: effective_context_source.as_deref().map(Arc::from),
                        resumed_from: resumed_from.as_deref().map(Arc::from),
                        capability_mode: capability_mode.as_deref().map(Arc::from),
                        permission_mode: permission_mode.as_deref().map(Arc::from),
                        effective_permission_mode: effective_permission_mode
                            .as_deref()
                            .map(Arc::from),
                        workflow_run_id: workflow_run_id.as_deref().map(Arc::from),
                        context_normalized: *context_normalized,
                        parent_prompt_id: parent_prompt_id.as_deref().map(Arc::from),
                        started_at: now,
                        last_progress_at: now,
                        finished: false,
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
                    });
            }
            GrowUpdate::SubagentProgress {
                child_session_id,
                duration_ms,
                turn_count,
                tool_call_count,
                tokens_used,
                context_window_tokens,
                context_usage_pct,
                tools_used,
                error_count,
                ..
            } => {
                if let Some(info) = self
                    .subagents
                    .get_mut(child_session_id)
                    .filter(|info| !info.finished)
                {
                    info.last_progress_at = now;
                    info.duration_ms = Some(*duration_ms);
                    info.turn_count = Some(*turn_count);
                    info.tool_call_count = Some(*tool_call_count);
                    info.tokens_used = Some(*tokens_used);
                    info.context_window_tokens = Some(*context_window_tokens);
                    info.context_usage_pct = Some(*context_usage_pct);
                    info.tools_used = tools_used.iter().map(|s| Arc::from(s.as_str())).collect();
                    info.error_count = Some(*error_count);
                }
            }
            GrowUpdate::SubagentFinished {
                child_session_id,
                status,
                duration_ms,
                error,
                tool_calls,
                turns,
                tokens_used,
                ..
            } => {
                if let Some(info) = self.subagents.get_mut(child_session_id) {
                    info.finished = true;
                    info.status = Some(Arc::from(status.as_str()));
                    info.error = error.as_deref().map(Arc::from);
                    info.duration_ms = Some(*duration_ms);
                    info.tool_calls = Some(*tool_calls);
                    info.turns = Some(*turns);
                    info.tokens_used = Some(*tokens_used);
                    info.last_progress_at = now;
                }
            }
            GrowUpdate::TaskBackgrounded {
                task_id,
                tool_call_id,
                command,
                cwd,
                output_file,
                description,
                monitor_description,
            } => {
                self.bg_tasks
                    .entry(task_id.clone())
                    .or_insert_with(|| BgTaskState {
                        task_id: task_id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        command: command.clone(),
                        description: monitor_description.clone().or_else(|| description.clone()),
                        cwd: cwd.clone(),
                        output_file: output_file.clone(),
                        status: BgTaskStatus::Running,
                        start_time: frame.wall_now(),
                        end_time: None,
                        exit_code: None,
                        signal: None,
                        stdout: String::new(),
                        stdout_line_count: 0,
                        truncated: false,
                        pending_kill: false,
                        kill_requested_at: None,
                        scrollback_entry_id: None,
                        is_monitor: monitor_description.is_some(),
                        restored_from_replay: false,
                    });
            }
            GrowUpdate::TaskCompleted { task_snapshot: s } => {
                let success =
                    s.exit_code == Some(0) || (s.exit_code.is_none() && s.signal.is_none());
                let state = self
                    .bg_tasks
                    .entry(s.task_id.clone())
                    .or_insert_with(|| BgTaskState {
                        task_id: s.task_id.clone(),
                        tool_call_id: String::new(),
                        command: s.command.clone(),
                        description: s.display_command.clone(),
                        cwd: String::new(),
                        output_file: String::new(),
                        status: BgTaskStatus::Running,
                        start_time: s.start_time,
                        end_time: None,
                        exit_code: None,
                        signal: None,
                        stdout: String::new(),
                        stdout_line_count: 0,
                        truncated: false,
                        pending_kill: false,
                        kill_requested_at: None,
                        scrollback_entry_id: None,
                        is_monitor: false,
                        restored_from_replay: false,
                    });
                state.status = if success {
                    BgTaskStatus::Done
                } else {
                    BgTaskStatus::Failed
                };
                state.start_time = s.start_time;
                state.end_time = s.end_time.or(Some(frame.wall_now()));
                state.exit_code = s.exit_code;
                state.signal = s.signal.clone();
                state.cwd = s.cwd.clone();
                state.output_file = s.output_file.to_string_lossy().into_owned();
                state.set_stdout(s.output.clone());
                state.truncated |= s.truncated;
            }
            GrowUpdate::MonitorEvent {
                task_id,
                event_text,
                ..
            } => {
                if let Some(task) = self.bg_tasks.get_mut(task_id) {
                    task.append_stdout(event_text);
                }
            }
            GrowUpdate::ScheduledTaskCreated {
                task_id,
                prompt,
                human_schedule,
                next_fire_at,
            }
            | GrowUpdate::ScheduledTaskFired {
                task_id,
                prompt,
                human_schedule,
                next_fire_at,
                ..
            } => {
                let task =
                    self.scheduled
                        .entry(task_id.clone())
                        .or_insert_with(|| ScheduledTaskInfo {
                            task_id: task_id.clone(),
                            prompt: prompt.clone(),
                            human_schedule: human_schedule.clone(),
                            created_at: now,
                            next_fire_at: next_fire_at.clone(),
                            tag: "loop".into(),
                            last_subagent_id: None,
                        });
                task.next_fire_at = next_fire_at.clone();
                if let GrowUpdate::ScheduledTaskFired { subagent_id, .. } = update {
                    task.last_subagent_id = Some(subagent_id.clone());
                }
            }
            GrowUpdate::ScheduledTaskDeleted { task_id } => {
                self.scheduled.remove(task_id);
            }
            GrowUpdate::WorkflowUpdated {
                run_id,
                definition_id,
                definition_scope,
                definition_hash,
                name,
                objective,
                status,
                phases,
                current_phase,
                agents,
                agent_budget,
                agents_used,
                agents_remaining,
                agent_usage_incomplete,
                active_agents,
                elapsed_ms,
                pause_message,
                result_summary,
                ..
            } => {
                self.workflows.retain(|run| run.run_id != *run_id);
                if status == "cleared" {
                    return;
                }
                self.workflows.push(WorkflowRunSnapshot {
                    run_id: run_id.clone(),
                    definition_id: definition_id.clone(),
                    definition_scope: definition_scope.clone(),
                    definition_hash: definition_hash.clone(),
                    name: name.clone(),
                    objective: objective.clone(),
                    status: status.clone(),
                    management_available: false,
                    phases: phases
                        .iter()
                        .map(|p| (p.title.clone(), p.state.clone()))
                        .collect(),
                    current_phase: current_phase.clone(),
                    agents: agents
                        .iter()
                        .map(|a| WorkflowAgentRowView {
                            agent_id: a.agent_id.clone(),
                            label: a.label.clone(),
                            phase: a.phase.clone(),
                            model: a.model.clone(),
                            state: a.state.clone(),
                            tokens_used: a.tokens_used,
                            duration_ms: a.duration_ms,
                        })
                        .collect(),
                    agent_budget: *agent_budget,
                    agents_used: *agents_used,
                    agents_remaining: *agents_remaining,
                    agent_usage_incomplete: *agent_usage_incomplete,
                    active_agents: *active_agents,
                    elapsed_ms: *elapsed_ms,
                    received_at: now,
                    pause_message: pause_message.clone(),
                    result_summary: result_summary.clone(),
                });
            }
            GrowUpdate::GoalUpdated {
                goal_id,
                objective,
                status,
                token_budget,
                tokens_used,
                usage_breakdown,
                usage_incomplete,
                elapsed_ms,
                created_at,
                updated_at,
                status_message,
            } => {
                if status == "cleared" {
                    if goal_id.is_empty()
                        || self.goal.as_ref().is_some_and(|g| g.goal_id == *goal_id)
                    {
                        self.goal = None;
                    }
                    return;
                }
                if let Some(status) = GoalDisplayStatus::parse(status) {
                    self.goal = Some(GoalDisplayState {
                        goal_id: goal_id.clone(),
                        objective: objective.clone(),
                        status,
                        token_budget: *token_budget,
                        tokens_used: *tokens_used,
                        usage_breakdown: *usage_breakdown,
                        usage_incomplete: *usage_incomplete,
                        elapsed_ms: *elapsed_ms,
                        created_at: created_at.clone(),
                        updated_at: updated_at.clone(),
                        status_message: status_message.clone(),
                        received_at: now,
                        elapsed_floor_ms: *elapsed_ms,
                    });
                }
            }
            _ => {}
        }
    }
}
