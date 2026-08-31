//! The turn-end `Stop`/`SubagentStop` gate for `SessionActor`.

use super::*;
use ::hooks::dispatcher;
use ::hooks::event::{
    self, BackgroundTaskType, StopBackgroundTask, StopSessionCron, clip_stop_entry_text,
};

pub const MAX_STOP_HOOK_CONTINUATIONS_PER_TURN: u32 = 8;

/// `command` is a shell-only field, so a monitor's watch command is carried in
/// `description` instead.
fn stop_entry_from_task(task: &tools::types::TaskSnapshot) -> StopBackgroundTask {
    let command_text =
        clip_stop_entry_text(task.display_command.as_deref().unwrap_or(&task.command));
    let (kind, command, description) = match task.kind {
        tools::computer::types::TaskKind::Bash => {
            (BackgroundTaskType::Shell, Some(command_text), None)
        }
        tools::computer::types::TaskKind::Monitor => {
            (BackgroundTaskType::Monitor, None, Some(command_text))
        }
    };
    StopBackgroundTask {
        id: task.task_id.clone(),
        r#type: kind,
        status: "running".to_string(),
        description,
        command,
        agent_type: None,
    }
}

fn stop_entry_from_subagent(
    summary: &tools::implementations::grow_build::task::types::ActiveSubagentSummary,
) -> StopBackgroundTask {
    StopBackgroundTask {
        id: summary.subagent_id.clone(),
        r#type: BackgroundTaskType::Subagent,
        status: "running".to_string(),
        description: Some(clip_stop_entry_text(&summary.description)),
        command: None,
        agent_type: Some(summary.subagent_type.clone()),
    }
}

fn stop_cron_from_scheduled(
    task: &tools::implementations::grow_build::scheduler::types::ScheduledTask,
) -> StopSessionCron {
    StopSessionCron {
        id: task.id.clone(),
        schedule: tools::implementations::grow_build::scheduler::interval::interval_to_human(
            task.interval_secs,
        ),
        recurring: task.recurring,
        prompt: clip_stop_entry_text(&task.prompt),
    }
}

const STOP_FEEDBACK_TEXT_MAX: usize = 10_000;

fn format_stop_feedback(blocks: &[dispatcher::StopBlock], additional_context: &[String]) -> String {
    use std::fmt::Write as _;
    let clip = |text: &str| event::clip_text(text, STOP_FEEDBACK_TEXT_MAX);
    let mut feedback = String::new();
    if !blocks.is_empty() {
        feedback.push_str("Stop hook feedback:\n");
        for block in blocks {
            let _ = writeln!(feedback, "- {}", clip(&block.reason));
        }
    }
    for context in additional_context {
        if !feedback.is_empty() {
            feedback.push('\n');
        }
        feedback.push_str(&clip(context));
    }
    feedback
}

impl SessionActor {
    pub(crate) async fn list_active_subagents(
        &self,
    ) -> Vec<tools::implementations::grow_build::task::types::ActiveSubagentSummary> {
        use tools::implementations::grow_build::task::types::{
            SubagentEvent, SubagentListActiveRequest,
        };
        let Some(ref event_tx) = self.tool_context.subagent_event_tx else {
            return Vec::new();
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        if event_tx
            .send(SubagentEvent::ListActive(SubagentListActiveRequest {
                parent_session_id: self.session_id_string(),
                respond_to: tx,
            }))
            .is_err()
        {
            return Vec::new();
        }
        rx.await.unwrap_or_default()
    }

    /// Snapshot in-flight background work and scheduled wakeups for the Stop
    /// hook input (filtering out tasks owned by other sessions on the shared
    /// backend).
    async fn stop_gate_work_snapshot(&self) -> (Vec<StopBackgroundTask>, Vec<StopSessionCron>) {
        let bridge = self.tool_bridge_handle();
        let my_session = self.session_id_string();
        let mut tasks: Vec<StopBackgroundTask> = bridge
            .list_background_tasks()
            .await
            .iter()
            .filter(|t| t.is_outstanding())
            .filter(|t| {
                t.owner_session_id
                    .as_deref()
                    .is_none_or(|owner| owner == my_session)
            })
            .map(stop_entry_from_task)
            .collect();
        tasks.extend(
            self.list_active_subagents()
                .await
                .iter()
                .map(stop_entry_from_subagent),
        );

        let now = chrono::Utc::now();
        let crons = bridge
            .list_scheduled_tasks()
            .await
            .iter()
            .filter(|t| !t.is_expired(now))
            .map(stop_cron_from_scheduled)
            .collect();
        (tasks, crons)
    }

    async fn announce_force_stop(&self, prevent: &dispatcher::StopBlock) {
        self.send_hook_annotation(&format!(
            "\u{26a0} Hook `{}` stopped the agent: {}",
            prevent.hook_name, prevent.reason
        ))
        .await;
    }

    async fn build_stop_payload(&self, stop_hook_active: bool) -> event::HookPayload {
        let last_assistant_message = self
            .chat_state_handle
            .get_last_assistant_text_in_turn()
            .await;
        if self.startup_hints.is_subagent {
            event::HookPayload::SubagentStop {
                phase: event::SubagentStopPhase::Gate,
                subagent_id: self.session_id_string(),
                subagent_type: self.subagent_type_label().unwrap_or_default(),
                stop_hook_active: Some(stop_hook_active),
                last_assistant_message,
            }
        } else {
            let (background_tasks, session_crons) = self.stop_gate_work_snapshot().await;
            event::HookPayload::Stop {
                reason: "end_turn".to_string(),
                stop_hook_active,
                last_assistant_message,
                background_tasks: Some(background_tasks),
                session_crons: Some(session_crons),
            }
        }
    }

    /// Run the turn-end `Stop`/`SubagentStop` hook gate and decide whether the
    /// agent may stop or must keep working. Hook failures fail open (the agent
    /// stops normally).
    pub(super) async fn run_stop_gate(
        &self,
        prompt_id: &str,
        continuations_this_turn: u32,
    ) -> StopGateDecision {
        let event = if self.startup_hints.is_subagent {
            event::HookEventName::SubagentStop
        } else {
            event::HookEventName::Stop
        };
        // At the cap no hook is consulted or notified for this forced stop,
        // unlike the force-stop path below which still notifies observers.
        if continuations_this_turn >= MAX_STOP_HOOK_CONTINUATIONS_PER_TURN {
            tracing::warn!(
                continuations_this_turn,
                "stop hook continuation limit reached; ending the turn"
            );
            self.send_hook_annotation(&format!(
                "\u{26a0} Stop hooks kept the agent working {MAX_STOP_HOOK_CONTINUATIONS_PER_TURN} times this turn: limit reached, ending the turn"
            ))
            .await;
            return StopGateDecision::AllowStop;
        }

        let payload = self.build_stop_payload(continuations_this_turn > 0).await;
        // Gate envelope via `make_hook_envelope`, not the observe-notify
        // `fire_hook`: client hooks get the awaited `grow/hooks/run` request
        // below, not a fire-and-forget event.
        let envelope = self.make_hook_envelope(event, Some(prompt_id.to_string()), payload);

        let Some(turn) = self.events.current_turn() else {
            tracing::error!(%event, "stop hook occurrence has no active causal turn");
            return StopGateDecision::AllowStop;
        };
        let cause = if event == event::HookEventName::SubagentStop {
            chat_state::HookCause::Subagent {
                subagent_id: self.session_id_string(),
            }
        } else {
            chat_state::HookCause::Turn { turn }
        };
        let aggregate = match self
            .dispatch_hook_occurrence(event, cause, envelope, event::GateKind::Stop)
            .await
        {
            Ok(aggregate) => aggregate,
            Err(error) => {
                // The subsequent Turn::Ended durable write is the terminal
                // fail-closed barrier; never admit another Step after a hook
                // lifecycle write has failed.
                tracing::error!(%error, "stop hook lifecycle was not durable");
                return StopGateDecision::AllowStop;
            }
        };
        let mut result = aggregate.into_stop_result();

        if let Some(prevent) = result.prevent_continuation.take() {
            self.announce_force_stop(&prevent).await;
            return StopGateDecision::AllowStop;
        }

        if !result.wants_continuation() {
            return StopGateDecision::AllowStop;
        }

        self.announce_keep_working(&result.blocks, &result.additional_context)
            .await;
        StopGateDecision::KeepWorking {
            feedback: format_stop_feedback(&result.blocks, &result.additional_context),
        }
    }

    /// Annotate the scrollback when a stop gate keeps the agent working: one line
    /// per block (with `HookBlocked` diagnostics), or the context lines when only
    /// `additionalContext` was returned.
    async fn announce_keep_working(
        &self,
        blocks: &[dispatcher::StopBlock],
        additional_context: &[String],
    ) {
        for block in blocks {
            self.send_hook_annotation(&format!(
                "\u{21a9} Stop blocked by hook `{}`, continuing: {}",
                block.hook_name, block.reason
            ))
            .await;
            ::diagnostics::session_ctx::log_event(::diagnostics::events::HookBlocked {
                hook_name: block.hook_name.clone(),
            });
        }
        if blocks.is_empty() {
            for context in additional_context {
                self.send_hook_annotation(&format!(
                    "\u{21a9} Stop hook feedback, continuing: {context}"
                ))
                .await;
            }
        }
    }
}

#[cfg(test)]
mod stop_gate_snapshot_tests {
    use super::*;

    fn task_snapshot(kind: tools::computer::types::TaskKind) -> tools::types::TaskSnapshot {
        tools::types::TaskSnapshot {
            goal_definition_revision: None,
            task_id: "task-1".into(),
            command: "sandbox-exec tail -f /var/log/syslog".into(),
            display_command: Some("tail -f /var/log/syslog".into()),
            cwd: "/tmp".into(),
            start_time: std::time::SystemTime::UNIX_EPOCH,
            end_time: None,
            output: String::new(),
            output_file: std::path::PathBuf::from("/tmp/out"),
            truncated: false,
            exit_code: None,
            signal: None,
            completed: false,
            kind,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: None,
            goal_id: None,
            description: None,
            is_backgrounded: false,
        }
    }

    #[test]
    fn task_snapshot_maps_to_stop_entry() {
        let shell = stop_entry_from_task(&task_snapshot(tools::computer::types::TaskKind::Bash));
        assert_eq!(shell.r#type, BackgroundTaskType::Shell);
        assert_eq!(shell.command.as_deref(), Some("tail -f /var/log/syslog"));
        assert!(shell.description.is_none());
        assert_eq!(shell.status, "running");
        assert!(shell.agent_type.is_none());

        let monitor =
            stop_entry_from_task(&task_snapshot(tools::computer::types::TaskKind::Monitor));
        assert_eq!(monitor.r#type, BackgroundTaskType::Monitor);
        assert!(monitor.command.is_none());
        assert_eq!(
            monitor.description.as_deref(),
            Some("tail -f /var/log/syslog")
        );
    }

    #[test]
    fn subagent_summary_maps_to_stop_entry() {
        let summary = tools::implementations::grow_build::task::types::ActiveSubagentSummary {
            subagent_id: "sub-1".into(),
            subagent_type: "explore".into(),
            description: "d".repeat(2000),
            elapsed_ms: 5,
        };
        let entry = stop_entry_from_subagent(&summary);
        assert_eq!(entry.r#type, BackgroundTaskType::Subagent);
        assert_eq!(entry.agent_type.as_deref(), Some("explore"));
        let description = entry.description.unwrap();
        assert!(description.ends_with("… [+1000 chars]"));
        assert!(entry.command.is_none());
    }

    #[test]
    fn format_stop_feedback_lists_blocks_then_appends_context() {
        let block = |reason: &str| dispatcher::StopBlock {
            hook_name: "h".into(),
            reason: reason.into(),
        };
        assert_eq!(
            format_stop_feedback(&[block("first"), block("second")], &[]),
            "Stop hook feedback:\n- first\n- second\n"
        );
        assert_eq!(
            format_stop_feedback(&[block("fix tests")], &["note".to_string()]),
            "Stop hook feedback:\n- fix tests\n\nnote"
        );
        assert_eq!(
            format_stop_feedback(&[], &["only context".to_string()]),
            "only context"
        );
    }

    #[test]
    fn scheduled_task_maps_to_stop_cron() {
        let task = tools::implementations::grow_build::scheduler::types::ScheduledTask::new(
            300,
            "check the build".into(),
            true,
            false,
        );
        let cron = stop_cron_from_scheduled(&task);
        assert_eq!(cron.schedule, "every 5 minutes");
        assert!(cron.recurring);
        assert_eq!(cron.prompt, "check the build");
    }
}
