//! Shared, observational projection for offline export and replay.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::acp::meta::NotificationMeta;
use crate::acp::tracker::AcpUpdateTracker;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::tool::{HookPhase, HookRunEntry, HookRunStatus};
use crate::scrollback::blocks::{
    BgTaskBlock, NoticeCategory, NoticeTone, SessionEvent, SubagentBlock, SubagentPermissionEvent,
    WorkflowBlock, WorkflowBlockPhase, WorkflowBlockStatus,
};
use crate::scrollback::entry::EntryId;
use crate::scrollback::state::ScrollbackState;
use shell::extensions::notification::{
    HookRunStatusDto, RetryState, SessionUpdate as GrowUpdate, UiNoticeTone,
};
use shell::session::storage::SessionUpdate;
use shell::session::storage::transcript::TranscriptEvent;

pub struct TranscriptProjection {
    pub scrollback: ScrollbackState,
    tracker: AcpUpdateTracker,
    subagents: HashMap<String, (String, String, Option<String>, bool)>,
    finished_subagents: HashSet<String>,
    workflows: HashMap<String, EntryId>,
    workflow_revisions: HashMap<String, u64>,
    cleared_workflows: HashSet<String>,
    goal: Option<(String, EntryId, String)>,
    retired_goals: HashSet<String>,
    behavior: Option<(tools::types::BehaviorId, Option<String>)>,
    background_tasks: HashMap<String, EntryId>,
    finished_background_tasks: HashSet<String>,
    user_messages: HashSet<String>,
    current_prompt: Option<String>,
    terminal_prompts: HashSet<String>,
}

impl Default for TranscriptProjection {
    fn default() -> Self {
        let mut scrollback = ScrollbackState::new();
        let mut appearance = scrollback.appearance().clone();
        // Entries constructed from history otherwise inherit Local::now() and
        // would render a fabricated current-time timestamp in replay.
        appearance.show_timestamps = false;
        scrollback.set_appearance(appearance);
        let mut tracker = AcpUpdateTracker::new();
        tracker.include_historical_thinking();
        Self {
            scrollback,
            tracker,
            subagents: HashMap::new(),
            finished_subagents: HashSet::new(),
            workflows: HashMap::new(),
            workflow_revisions: HashMap::new(),
            cleared_workflows: HashSet::new(),
            goal: None,
            retired_goals: HashSet::new(),
            behavior: None,
            background_tasks: HashMap::new(),
            finished_background_tasks: HashSet::new(),
            user_messages: HashSet::new(),
            current_prompt: None,
            terminal_prompts: HashSet::new(),
        }
    }
}

impl TranscriptProjection {
    pub fn behavior_label(&self) -> Option<String> {
        self.behavior.as_ref().map(|(mode, phase)| {
            format!(
                "{}{}",
                mode.as_id(),
                phase
                    .as_ref()
                    .map_or(String::new(), |phase| format!(" / {phase}"))
            )
        })
    }

    pub fn goal_status(&self) -> Option<&str> {
        self.goal.as_ref().map(|(_, _, status)| status.as_str())
    }

    pub fn has_subagent(&self, child_session_id: &str) -> bool {
        self.subagents.contains_key(child_session_id)
    }

    fn apply_behavior(&mut self, update: &acp_transport::protocol::CurrentModeUpdate) {
        let Some(mode) = tools::types::BehaviorId::try_from_id(update.current_mode_id.0.as_ref())
        else {
            return;
        };
        let phase = update
            .meta
            .as_ref()
            .and_then(|meta| meta.get("grow/planPhase"))
            .and_then(serde_json::Value::as_str)
            .filter(|_| mode == tools::types::BehaviorId::Plan)
            .map(str::to_owned);
        let next = (mode, phase);
        if self.behavior.as_ref() != Some(&next) {
            self.behavior = Some(next);
            self.scrollback.push_block(RenderBlock::typed_notice(
                NoticeTone::Info,
                NoticeCategory::Control,
                format!("Behavior · {}", self.behavior_label().expect("set above")),
                None,
            ));
        }
        if let Some(change) = update
            .meta
            .as_ref()
            .and_then(|meta| meta.get("grow/behaviorChange"))
            && matches!(
                change.get("status").and_then(serde_json::Value::as_str),
                Some("confirmation_required" | "rejected")
            )
            && let Some(message) = change.get("message").and_then(serde_json::Value::as_str)
        {
            self.scrollback.push_block(RenderBlock::typed_notice(
                NoticeTone::Warning,
                NoticeCategory::Control,
                message,
                None,
            ));
        }
    }

    fn clear_goal(&mut self) {
        if let Some((goal_id, entry_id, _)) = self.goal.take() {
            self.retired_goals.insert(goal_id);
            self.scrollback.remove_entry(entry_id);
        }
    }

    pub fn unresolved_tools(&self) -> usize {
        self.tracker.unresolved_transcript_tools()
    }

    pub fn apply(&mut self, event: TranscriptEvent) {
        match event.update {
            SessionUpdate::Acp(notification) => {
                if let acp_transport::protocol::SessionUpdate::CurrentModeUpdate(update) =
                    &notification.update
                {
                    self.apply_behavior(update);
                    return;
                }
                if let acp_transport::protocol::SessionUpdate::UserMessageChunk(chunk) =
                    &notification.update
                    && let Some(message_id) = chunk
                        .meta
                        .as_ref()
                        .and_then(|meta| meta.get("messageId"))
                        .and_then(|value| value.as_str())
                    && !self.user_messages.insert(message_id.to_owned())
                {
                    // A persisted duplicate echo must not create a second
                    // stream boundary halfway through its existing reply.
                    return;
                }
                let mut meta = NotificationMeta::from_json(notification.meta.as_ref());
                meta.is_replay = true;
                let prompt = meta
                    .prompt_id
                    .clone()
                    .or_else(|| match &notification.update {
                        acp_transport::protocol::SessionUpdate::UserMessageChunk(chunk) => chunk
                            .meta
                            .as_ref()
                            .and_then(|meta| meta.get("messageId"))
                            .and_then(|value| value.as_str())
                            .map(str::to_owned),
                        _ => None,
                    });
                if matches!(
                    &notification.update,
                    acp_transport::protocol::SessionUpdate::UserMessageChunk(_)
                        | acp_transport::protocol::SessionUpdate::AgentMessageChunk(_)
                        | acp_transport::protocol::SessionUpdate::AgentThoughtChunk(_)
                ) {
                    if let Some(prompt) = prompt {
                        if self
                            .current_prompt
                            .as_ref()
                            .is_some_and(|current| *current != prompt)
                        {
                            self.tracker.finish_transcript_turn(&mut self.scrollback);
                            self.scrollback.seal_subagent_permission_group();
                        }
                        self.current_prompt = Some(prompt);
                    }
                }
                self.tracker.handle_transcript_update(
                    notification.update,
                    &meta,
                    &mut self.scrollback,
                );
            }
            SessionUpdate::Grow(notification) => {
                let captured_snapshot = notification
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get("transcriptSnapshot"))
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let mut meta = NotificationMeta::from_json(
                    notification.meta.as_ref().and_then(|v| v.as_object()),
                );
                meta.is_replay = true;
                self.apply_grow(notification.update, meta, captured_snapshot);
            }
            SessionUpdate::ResponseReplayProjection(_) => {
                unreachable!("reader expands projections")
            }
        }
    }

    fn apply_grow(&mut self, update: GrowUpdate, meta: NotificationMeta, captured_snapshot: bool) {
        match update {
            GrowUpdate::ControlStateUpdate(update) => {
                use shell::extensions::notification::ControlPhase;
                if update.current.domain() != update.domain
                    || update
                        .desired
                        .as_ref()
                        .is_some_and(|target| target.domain() != update.domain)
                    || !matches!(update.phase, ControlPhase::Applied | ControlPhase::Rejected)
                {
                    return;
                }
                if let Some(message) = update.message {
                    let id = meta.event_id.unwrap_or_else(|| {
                        format!(
                            "control:{}:{:?}:{}",
                            update.epoch, update.domain, update.revision
                        )
                    });
                    self.scrollback.push_block(RenderBlock::terminal_notice(
                        id,
                        if update.phase == ControlPhase::Applied {
                            NoticeTone::Success
                        } else {
                            NoticeTone::Error
                        },
                        NoticeCategory::Control,
                        message,
                        None,
                    ));
                }
            }
            GrowUpdate::SamplingAttempt {
                request_id,
                attempt,
                state,
            } => {
                self.tracker.handle_sampling_attempt(
                    request_id,
                    attempt,
                    state,
                    &mut self.scrollback,
                );
            }
            GrowUpdate::UiNotice(notice) if notice.tone != UiNoticeTone::Progress => {
                crate::app::project_ui_notice(&mut self.scrollback, notice, meta.event_id, true);
            }
            GrowUpdate::SubagentSpawned {
                child_session_id,
                description,
                subagent_type,
                model,
                workflow_run_id,
                ..
            } => {
                if self.subagents.contains_key(&child_session_id) {
                    return;
                }
                self.subagents.insert(
                    child_session_id.clone(),
                    (
                        description.clone(),
                        subagent_type.clone(),
                        model.clone(),
                        workflow_run_id.is_some(),
                    ),
                );
                if workflow_run_id.is_none() {
                    self.scrollback.push_block(RenderBlock::Subagent(
                        SubagentBlock::started(
                            description,
                            child_session_id,
                            subagent_type,
                            model,
                            false,
                        )
                        .with_event_id(meta.event_id),
                    ));
                }
            }
            GrowUpdate::SubagentFinished {
                child_session_id,
                status,
                duration_ms,
                error,
                ..
            } => {
                if !self.finished_subagents.insert(child_session_id.clone()) {
                    return;
                }
                let (description, kind, model, is_workflow_child) = self
                    .subagents
                    .get(&child_session_id)
                    .cloned()
                    .unwrap_or_else(|| (String::new(), "subagent".into(), None, false));
                if is_workflow_child {
                    return;
                }
                let elapsed = Duration::from_millis(duration_ms);
                let block = match status.as_str() {
                    "completed" => {
                        SubagentBlock::completed(&description, &child_session_id, elapsed)
                    }
                    "cancelled" => {
                        SubagentBlock::cancelled(&description, &child_session_id, elapsed)
                    }
                    _ => SubagentBlock::failed(&description, &child_session_id, elapsed, error),
                };
                self.scrollback.push_block(RenderBlock::Subagent(
                    block
                        .with_identity(&kind, model)
                        .with_event_id(meta.event_id),
                ));
            }
            GrowUpdate::HookExecution {
                occurrence_id,
                event_name,
                tool_call_id,
                runs,
                annotations,
                ..
            } => {
                if !self.tracker.claim_hook_occurrence(&occurrence_id) {
                    return;
                }
                let entries = runs
                    .into_iter()
                    .map(|run| HookRunEntry {
                        name: run.name,
                        status: match run.status {
                            HookRunStatusDto::Success { elapsed_ms } => HookRunStatus::Success {
                                elapsed: Duration::from_millis(elapsed_ms),
                            },
                            HookRunStatusDto::Skipped => HookRunStatus::Skipped,
                            HookRunStatusDto::Blocked { detail, elapsed_ms } => {
                                HookRunStatus::Blocked {
                                    detail,
                                    elapsed: Duration::from_millis(elapsed_ms),
                                }
                            }
                            HookRunStatusDto::Failed { error, elapsed_ms } => {
                                HookRunStatus::Failed {
                                    error,
                                    elapsed: Duration::from_millis(elapsed_ms),
                                }
                            }
                        },
                        output: run.output,
                    })
                    .collect();
                let tool_exists = tool_call_id
                    .as_deref()
                    .and_then(|id| self.tracker.tool_entry_id(id, &self.scrollback))
                    .is_some();
                if let Some(id) = tool_call_id.as_deref().filter(|_| tool_exists)
                    && matches!(event_name.as_str(), "pre_tool_use" | "post_tool_use")
                {
                    self.tracker.attach_tool_hooks(
                        id,
                        if event_name == "pre_tool_use" {
                            HookPhase::Pre
                        } else {
                            HookPhase::Post
                        },
                        entries,
                        &mut self.scrollback,
                    );
                } else {
                    self.tracker.restore_hook_history(
                        format!("{event_name} · {occurrence_id}"),
                        entries,
                        annotations,
                        &mut self.scrollback,
                    );
                }
            }
            GrowUpdate::WorkflowUpdated {
                run_id,
                revision,
                name,
                objective,
                status,
                phases,
                current_phase,
                active_agents,
                elapsed_ms,
                ..
            } => {
                let elapsed = Duration::from_millis(elapsed_ms);
                if self.workflow_revisions.get(&run_id).is_some_and(|last| {
                    (revision > 0 && revision <= *last)
                        || (status != "cleared" && revision == 0 && *last > 0)
                }) || (status != "cleared"
                    && revision == 0
                    && self.cleared_workflows.contains(&run_id))
                {
                    return;
                }
                if revision > 0 {
                    self.workflow_revisions.insert(run_id.clone(), revision);
                }
                if status == "cleared" {
                    self.cleared_workflows.insert(run_id.clone());
                    if let Some(id) = self.workflows.get(&run_id).copied() {
                        self.scrollback.finish_running(id);
                    }
                    return;
                }
                let entry_id = match self.workflows.get(&run_id).copied() {
                    Some(id) if self.scrollback.get_by_id(id).is_some() => id,
                    _ => {
                        let id = self.scrollback.push_block(RenderBlock::Workflow(
                            WorkflowBlock::started(&run_id, &name, &objective),
                        ));
                        self.workflows.insert(run_id.clone(), id);
                        id
                    }
                };
                if let Some(entry) = self.scrollback.get_by_id_mut(entry_id)
                    && let RenderBlock::Workflow(block) = &mut entry.block
                {
                    block.status = match status.as_str() {
                        "active" => WorkflowBlockStatus::Running,
                        "complete" => WorkflowBlockStatus::Done { elapsed },
                        "failed" | "interrupted" => WorkflowBlockStatus::Failed { elapsed },
                        "cancelled" => WorkflowBlockStatus::Cancelled { elapsed },
                        _ => WorkflowBlockStatus::Paused { elapsed },
                    };
                    block.phases = phases
                        .into_iter()
                        .map(|phase| WorkflowBlockPhase {
                            title: phase.title,
                            state: phase.state,
                        })
                        .collect();
                    block.current_phase = current_phase;
                    block.active_agents = active_agents;
                    block.elapsed = elapsed;
                    entry.invalidate_cache();
                }
                self.scrollback
                    .set_entry_running_with_clock(entry_id, status == "active", false);
            }
            GrowUpdate::GoalUpdated {
                goal_id,
                objective,
                status,
                status_message,
                token_budget,
                tokens_used,
                usage_incomplete,
                usage_breakdown,
                elapsed_ms,
                ..
            } => {
                if status == "cleared" {
                    if goal_id.is_empty()
                        || self
                            .goal
                            .as_ref()
                            .is_some_and(|(current, _, _)| *current == goal_id)
                    {
                        self.clear_goal();
                    }
                    return;
                }
                if crate::app::session::GoalDisplayStatus::parse(&status).is_none()
                    || goal_id.is_empty()
                {
                    return;
                }
                if captured_snapshot {
                    self.retired_goals.remove(&goal_id);
                }
                if self.retired_goals.contains(&goal_id) {
                    return;
                }
                if self
                    .goal
                    .as_ref()
                    .is_some_and(|(current, _, _)| *current != goal_id)
                {
                    self.clear_goal();
                }
                let message = format!(
                    "Goal · {objective} · {status} · {}{tokens_used} tokens{}{}",
                    if usage_incomplete { ">=" } else { "" },
                    token_budget.map_or_else(
                        || " · no token budget".to_owned(),
                        |budget| format!(" / {budget}")
                    ),
                    status_message
                        .as_deref()
                        .map_or(String::new(), |s| format!(" · {s}"))
                );
                let historical = tokens_used.saturating_sub(usage_breakdown.total()).max(0);
                let marker = if usage_incomplete || historical > 0 {
                    ">="
                } else {
                    ""
                };
                let (cache_rate, measured) = crate::app::status_blocks::goal_cache_hit_rate(
                    usage_breakdown,
                    usage_incomplete || historical > 0,
                );
                let cache_rate = if measured && cache_rate != "N/A" {
                    format!("measured {cache_rate}")
                } else {
                    cache_rate
                };
                let details = Some(format!(
                    "Recorded elapsed: {elapsed_ms} ms\nInput (cache hit): {marker}{}\nInput (cache miss): {marker}{}\nInput (unclassified): {marker}{}\nCache hit rate: {cache_rate}\nOutput: {marker}{}\nHistory without categories: {historical}\nBudget basis: all input + output, including cache hits",
                    usage_breakdown.cached_input_tokens,
                    usage_breakdown.uncached_input_tokens,
                    usage_breakdown.unclassified_input_tokens,
                    usage_breakdown.output_tokens,
                ));
                match self.goal.as_ref().map(|(_, id, _)| *id) {
                    Some(id) => {
                        if let Some((_, _, current_status)) = &mut self.goal {
                            *current_status = status;
                        }
                        if let Some(entry) = self.scrollback.get_by_id_mut(id)
                            && let RenderBlock::Notice(block) = &mut entry.block
                        {
                            block.text = message;
                            block.details = details;
                            entry.invalidate_cache();
                        }
                    }
                    None => {
                        let id = self.scrollback.push_block(RenderBlock::typed_notice(
                            NoticeTone::Info,
                            NoticeCategory::Control,
                            message,
                            details,
                        ));
                        self.goal = Some((goal_id, id, status));
                    }
                }
            }
            GrowUpdate::TaskBackgrounded {
                tool_call_id,
                task_id,
                command,
                mut description,
                monitor_description,
                ..
            } => {
                if self.background_tasks.contains_key(&task_id)
                    || self.finished_background_tasks.contains(&task_id)
                {
                    return;
                }
                let pending = self.tracker.pending_tool_entry_id(&tool_call_id);
                description = monitor_description.or(description).or_else(|| {
                    self.tracker
                        .bg_deferred_tools
                        .remove(&tool_call_id)
                        .flatten()
                });
                self.tracker.remove_pending_tool(&tool_call_id);
                // Execution updates after demotion belong to the background
                // task, not to a new foreground tool row.
                self.tracker.bg_deferred_tools.insert(tool_call_id, None);
                if description.is_none()
                    && let Some(entry) = pending.and_then(|id| self.scrollback.get_by_id(id))
                    && let RenderBlock::ToolCall(crate::scrollback::ToolCallBlock::Execute(tool)) =
                        &entry.block
                {
                    description = tool.description.clone();
                }
                let block = RenderBlock::BgTask(
                    BgTaskBlock::started(command, &task_id).with_description(description),
                );
                let entry = if let Some(id) =
                    pending.filter(|id| self.scrollback.get_by_id(*id).is_some())
                {
                    let entry = self.scrollback.get_by_id_mut(id).expect("checked above");
                    entry.block = block;
                    entry.invalidate_cache();
                    self.scrollback.finish_running(id);
                    self.scrollback.mark_height_dirty(id);
                    id
                } else {
                    self.scrollback.push_block(block)
                };
                self.background_tasks.insert(task_id, entry);
            }
            GrowUpdate::TaskCompleted { task_snapshot } => {
                let task_id = task_snapshot.task_id;
                if !self.finished_background_tasks.insert(task_id.clone()) {
                    return;
                }
                let elapsed = task_snapshot
                    .end_time
                    .and_then(|end| end.duration_since(task_snapshot.start_time).ok())
                    .unwrap_or_default();
                let success = task_snapshot.exit_code == Some(0)
                    || (task_snapshot.exit_code.is_none() && task_snapshot.signal.is_none());
                let description = task_snapshot
                    .display_command
                    .filter(|d| d.trim() != task_snapshot.command.trim());
                if let Some(entry_id) = self.background_tasks.remove(&task_id) {
                    self.scrollback.finish_running(entry_id);
                }
                if task_snapshot.signal.as_deref() != Some("session_restart") {
                    let block = if success {
                        BgTaskBlock::completed(task_snapshot.command, task_id, elapsed)
                    } else {
                        BgTaskBlock::failed(
                            task_snapshot.command,
                            task_id,
                            elapsed,
                            task_snapshot.exit_code,
                            task_snapshot.signal,
                        )
                    };
                    self.scrollback
                        .push_block(RenderBlock::BgTask(block.with_description(description)));
                }
            }
            GrowUpdate::SubagentPermissionDecision {
                child_session_id,
                subagent_type,
                description,
                tool_call_id,
                tool_name,
                access_kind,
                access_summary,
                access_detail,
                outcome,
                source,
                reason,
                classifier_reason,
                latency_ms,
            } => {
                self.scrollback
                    .push_subagent_permission(SubagentPermissionEvent {
                        child_session_id,
                        subagent_title: None,
                        subagent_type,
                        description,
                        tool_call_id,
                        tool_name,
                        access_kind,
                        access_summary,
                        access_detail,
                        outcome,
                        source,
                        reason,
                        classifier_reason,
                        latency_ms,
                    });
            }
            GrowUpdate::TurnCompleted {
                prompt_id,
                stop_reason,
                agent_result,
                ..
            } => {
                if !self.terminal_prompts.insert(prompt_id.clone()) {
                    return;
                }
                // A delayed terminal belongs to its original prompt. It must
                // not end a newer response or its permission-audit epoch.
                if self
                    .current_prompt
                    .as_ref()
                    .is_none_or(|current| *current == prompt_id)
                {
                    self.tracker.finish_transcript_turn(&mut self.scrollback);
                    self.scrollback.seal_subagent_permission_group();
                    self.current_prompt = None;
                }
                let reason = if stop_reason.is_empty() {
                    "unknown"
                } else {
                    &stop_reason
                };
                let mut text = format!("Turn {prompt_id} · {reason}");
                if matches!(reason, "error" | "rate_limit")
                    && let Some(result) = agent_result.filter(|value| !value.is_empty())
                {
                    text.push_str(&format!(": {result}"));
                }
                let block = match meta.event_id {
                    Some(id) => RenderBlock::terminal_notice(
                        id,
                        NoticeTone::Info,
                        NoticeCategory::Control,
                        text,
                        None,
                    ),
                    None => RenderBlock::typed_notice(
                        NoticeTone::Info,
                        NoticeCategory::Control,
                        text,
                        None,
                    ),
                };
                self.scrollback.push_block(block);
            }
            GrowUpdate::RetryState(RetryState::Exhausted { reason, .. }) => {
                self.scrollback
                    .push_block(RenderBlock::notice(format!("Retry exhausted: {reason}")));
            }
            GrowUpdate::RetryState(RetryState::Failed { message, .. }) => {
                self.scrollback.push_block(RenderBlock::notice(message));
            }
            GrowUpdate::AutoCompactCompleted {
                tokens_before,
                tokens_after,
                elapsed_ms,
                ..
            } => {
                self.scrollback
                    .push_block(RenderBlock::session_event_with_id(
                        SessionEvent::CompactionCompleted {
                            tokens_before,
                            tokens_after,
                            elapsed_ms,
                        },
                        meta.event_id,
                    ));
            }
            GrowUpdate::AutoCompactFailed { error } => {
                self.scrollback
                    .push_block(RenderBlock::session_event_with_id(
                        SessionEvent::CompactionFailed { error },
                        meta.event_id,
                    ));
            }
            GrowUpdate::AutoCompactCancelled { .. } => {
                self.scrollback
                    .push_block(RenderBlock::session_event_with_id(
                        SessionEvent::CompactionCancelled,
                        meta.event_id,
                    ));
            }
            GrowUpdate::ImageDropped { notes } | GrowUpdate::ImageProjected { notes } => {
                self.scrollback
                    .push_block(RenderBlock::notice(notes.join("\n")));
            }
            GrowUpdate::ImageProcessing { message } => {
                self.scrollback.push_block(RenderBlock::notice(message));
            }
            GrowUpdate::ImageCompressed { images, message } if images.is_empty() => {
                self.scrollback.push_block(RenderBlock::notice(message));
            }
            GrowUpdate::MemoryFlushCompleted { result, .. }
            | GrowUpdate::MemoryDreamCompleted { result, .. } => {
                self.scrollback.push_block(RenderBlock::notice(result));
            }
            GrowUpdate::MemorySessionSaved { path } => {
                self.scrollback
                    .push_block(RenderBlock::notice(format!("Memory saved: {path}")));
            }
            GrowUpdate::SessionRecap { summary, auto, .. } => {
                self.scrollback
                    .push_block(RenderBlock::session_event_with_id(
                        SessionEvent::Recap { summary, auto },
                        meta.event_id,
                    ));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
