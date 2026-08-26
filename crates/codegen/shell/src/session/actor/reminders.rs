//! System-reminder injection concern for `SessionActor`.
use super::*;
/// Build the date-rollover reminder when the local calendar
/// date has advanced past the date last surfaced to the model.
///
/// Returns `None` when the date is unchanged (already announced) or has moved
/// backwards (e.g. a manual clock adjustment), so the caller injects nothing
/// in the common case. Pure (no `self`, no clock access) so the rollover
/// boundary logic is unit-testable; see `reminder_policy_tests`.
pub(crate) fn date_rollover_reminder(
    today: chrono::NaiveDate,
    last_announced: chrono::NaiveDate,
) -> Option<String> {
    if today <= last_announced {
        return None;
    }
    Some(format!(
        "The local date has changed since this session started. Today's date is now \
         {today}. Any date shown earlier in this session was set at startup and is now stale; \
         use {today} as the current date."
    ))
}
/// Body of the one-shot interrupt `<system-reminder>` injected on the next real
/// user turn after a mid-stream abort that left the model with no other signal.
/// Wrapped in grow's `<system-reminder>` shape by [`SessionActor::push_system_reminder`].
/// See [`SessionActor::maybe_inject_interrupt_reminder`].
pub(crate) const INTERRUPT_REMINDER: &str = "[Request interrupted by user]";
const WORKFLOW_RESULT_SUMMARY_REMINDER_CAP: usize = 4 * 1024;
const WORKFLOW_OBJECTIVE_REMINDER_CAP: usize = 256;
fn workflow_completion_detail(detail: &str) -> std::borrow::Cow<'_, str> {
    let normalized = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized == detail {
        tools::util::truncate_str_with_marker(detail, WORKFLOW_RESULT_SUMMARY_REMINDER_CAP)
    } else {
        std::borrow::Cow::Owned(
            tools::util::truncate_str_with_marker(
                &normalized,
                WORKFLOW_RESULT_SUMMARY_REMINDER_CAP,
            )
            .into_owned(),
        )
    }
}
impl SessionActor {
    pub(super) fn push_workflow_launch_reminder(
        &self,
        display_name: &str,
        run_id: &str,
        objective: &str,
        command_line: &str,
        resumed: bool,
    ) {
        let verb = if resumed { "resumed" } else { "launched" };
        let command_line = command_line
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let mut body = format!(
            "The user {verb} background workflow '{display_name}' (run id {run_id}) with the \
             slash command: {}\nThis was handled host-side; no tool call was involved.",
            tools::util::truncate_str(&command_line, WORKFLOW_OBJECTIVE_REMINDER_CAP)
        );
        let objective = objective.split_whitespace().collect::<Vec<_>>().join(" ");
        let objective_redundant = !objective.is_empty()
            && (objective == command_line || command_line.ends_with(&format!(" {objective}")));
        if !objective.is_empty() && !objective_redundant {
            body.push_str(&format!(
                "\nObjective: {}",
                tools::util::truncate_str(&objective, WORKFLOW_OBJECTIVE_REMINDER_CAP)
            ));
        }
        body.push_str(&format!(
            "\nIt runs in the background: live status snapshots appear at ordinary turn starts, \
             its final result arrives through the durable notification inbox, and the user can \
             watch it in /workflows. If it pauses, \
             it can be resumed by calling the workflow tool with action: \"control_run\", \
             run_id: \"{run_id}\", operation: \"resume\". Keep run ids internal — the user \
             knows runs by display name. No \
             action needed unless the user asks."
        ));
        self.push_system_reminder(&body);
    }
    pub(super) async fn inject_workflow_status_reminder(&self) {
        if self.goal_loop_active() {
            return;
        }
        let tracker = self.workflow_tracker().await;
        let report = tracker.lock().take_status_report();
        if report.is_empty() {
            return;
        }
        self.push_system_reminder(&format_workflow_status_reminder(&report));
    }

    /// Render the exact terminal manifest snapshot into the durable
    /// notification payload. Report discovery descends from the pinned
    /// session-directory capability rather than reopening its display path.
    pub(super) async fn workflow_completion_notification(
        &self,
        run: &crate::session::workflow::tracker::WorkflowRunState,
    ) -> String {
        let bridge = self.agent.borrow().tool_bridge().clone();
        let read_tool_name =
            tools::reminders::task_completion::resolve_read_tool_name(&bridge).await;
        let report_path = self.workflow_report_path(&run.run_id).await;
        format_workflow_completion_notification(
            run,
            report_path.as_deref(),
            read_tool_name.as_deref(),
        )
    }

    async fn workflow_report_path(&self, run_id: &str) -> Option<std::path::PathBuf> {
        let directory = self.session_directory.try_clone().ok()?;
        let relative = std::path::PathBuf::from("workflows")
            .join(run_id)
            .join("scratch");
        tokio::task::spawn_blocking(move || {
            let scratch = directory
                .open_relative(&relative, "workflow scratch directory", false)
                .ok()?;
            scratch
                .open_regular(std::ffi::OsStr::new("report.md"), "workflow report")
                .ok()?;
            Some(scratch.display_path().join("report.md"))
        })
        .await
        .ok()
        .flatten()
    }
}
pub(super) fn format_workflow_status_reminder(
    runs: &[crate::session::workflow::tracker::WorkflowRunState],
) -> String {
    use std::fmt::Write as _;
    let n = runs.len();
    let noun = if n == 1 {
        "background workflow run"
    } else {
        "background workflow runs"
    };
    let mut buf = format!("Status of {n} {noun} in this session:\n");
    for run in runs {
        let _ = write!(
            buf,
            "\n- Workflow '{}' (run id {}) — status: {}",
            run.name,
            run.run_id,
            run.status.as_str()
        );
        if let Some(definition_id) = run.definition_id.as_ref() {
            let provenance = run
                .definition_scope
                .zip(run.definition_hash.as_deref())
                .map(|(scope, hash)| {
                    format!("{}@{}", scope.as_str(), hash.get(..8).unwrap_or(hash))
                })
                .unwrap_or_else(|| "unknown hash".into());
            let _ = write!(buf, "\n  Definition: {definition_id} ({provenance})");
        }
        let objective = run
            .objective
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if !objective.is_empty() {
            let _ = write!(
                buf,
                "\n  Objective: {}",
                tools::util::truncate_str(&objective, WORKFLOW_OBJECTIVE_REMINDER_CAP)
            );
        }
        if let Some(cur) = run.current_phase.as_deref() {
            match run.phases.iter().position(|p| p.title == cur) {
                Some(pos) => {
                    let _ = write!(buf, "\n  Phase: {} ({}/{})", cur, pos + 1, run.phases.len());
                }
                None => {
                    let _ = write!(buf, "\n  Phase: {cur}");
                }
            }
        }
        if !run.agents.is_empty() {
            let done = run.agents.iter().filter(|a| a.state == "done").count();
            let running = run.agents.iter().filter(|a| a.state == "running").count();
            let failed = run.agents.iter().filter(|a| a.state == "failed").count();
            let mut parts = vec![format!("{done} done")];
            if running > 0 {
                parts.push(format!("{running} running"));
            }
            if failed > 0 {
                parts.push(format!("{failed} failed"));
            }
            let _ = write!(buf, "\n  Agents: {}", parts.join(", "));
        }
        match run.agent_budget {
            Some(budget) => {
                let _ = write!(buf, "\n  Agents: {} of {} budget", run.agents_used, budget);
            }
            None if run.agents_used > 0 => {
                let _ = write!(buf, "\n  Agents: {}", run.agents_used);
            }
            None => {}
        }
        if run.agent_usage_incomplete {
            let _ = write!(
                buf,
                "\n  Agent accounting incomplete: the session was interrupted before all \
                 logical agent reservations were reconciled"
            );
        }
        let _ = write!(
            buf,
            "\n  Elapsed: {}",
            format_workflow_elapsed(run.elapsed_ms_floor)
        );
        if run.status.is_paused() {
            if let Some(msg) = run.pause_message.as_deref() {
                let _ = write!(
                    buf,
                    "\n  Paused: {}",
                    tools::util::truncate_str(msg, WORKFLOW_RESULT_SUMMARY_REMINDER_CAP)
                );
            }
            let max_budget_exhausted = run.status
                == crate::session::workflow::tracker::WorkflowRunStatus::BudgetLimited
                && run.agents_used >= workflow::MAX_AGENT_BUDGET;
            if max_budget_exhausted {
                let _ = write!(buf, "\n  Not resumable: start a new workflow run.");
            } else {
                let budget_suffix = if run.status
                    == crate::session::workflow::tracker::WorkflowRunStatus::BudgetLimited
                {
                    " and a raised agent_budget (the resume is rejected while usage \
                     is at or over the cap)"
                } else {
                    ""
                };
                let _ = write!(
                    buf,
                    "\n  Resumable: call the workflow tool with action: \"control_run\", \
                     run_id: \"{}\", operation: \"resume\"{}.",
                    run.run_id, budget_suffix
                );
            }
        }
    }
    buf.push_str(
        "\nThese run in the background — do not poll task tools for them. Live status snapshots \
         appear at ordinary turn starts and terminal results arrive through the durable \
         notification inbox. Keep run ids internal (the user knows runs by display name).",
    );
    buf
}
fn format_workflow_elapsed(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
fn format_workflow_completion_notification(
    run: &crate::session::workflow::tracker::WorkflowRunState,
    report_path: Option<&std::path::Path>,
    read_tool_name: Option<&str>,
) -> String {
    use std::fmt::Write as _;
    let mut buf = format!(
        "A background workflow run stopped:\n\n- Workflow '{}' (run id {}) — status: {}",
        run.name,
        run.run_id,
        run.status.as_str()
    );
    if let Some(definition_id) = run.definition_id.as_ref() {
        let provenance = run
            .definition_scope
            .zip(run.definition_hash.as_deref())
            .map(|(scope, hash)| format!("{}@{}", scope.as_str(), hash.get(..8).unwrap_or(hash)))
            .unwrap_or_else(|| "unknown hash".into());
        let _ = write!(buf, "\n  Definition: {definition_id} ({provenance})");
    }
    let objective = run
        .objective
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !objective.is_empty() {
        let _ = write!(
            buf,
            "\n  Objective: {}",
            tools::util::truncate_str(&objective, WORKFLOW_OBJECTIVE_REMINDER_CAP)
        );
    }
    let _ = write!(
        buf,
        "\n  Elapsed: {}",
        format_workflow_elapsed(run.elapsed_ms_floor)
    );
    if let Some(summary) = run.result_summary.as_deref() {
        let capped = tools::util::truncate_str(summary, WORKFLOW_RESULT_SUMMARY_REMINDER_CAP);
        buf.push_str("\n  Result:\n");
        for line in capped.lines() {
            let _ = writeln!(buf, "    {line}");
        }
        if capped.len() < summary.len() {
            let _ = writeln!(
                buf,
                "    [... result truncated ({} bytes total)]",
                summary.len()
            );
        }
    } else if let Some(detail) = run.pause_message.as_deref() {
        let detail = workflow_completion_detail(detail);
        let _ = write!(buf, "\n  Detail: {detail}\n");
    } else {
        buf.push('\n');
    }
    if run.status == crate::session::workflow::tracker::WorkflowRunStatus::BudgetLimited {
        if run.agents_used >= workflow::MAX_AGENT_BUDGET {
            let _ = writeln!(
                buf,
                "  Not resumable: this run reached the maximum agent budget; start a new \
                     workflow run."
            );
        } else {
            let _ = writeln!(
                buf,
                "  Resumable: call the workflow tool with action: \"control_run\", \
                     run_id: \"{}\", operation: \"resume\", and a raised agent_budget (the \
                     resume is rejected while usage is at or over the cap).",
                run.run_id
            );
        }
    }
    if run.save_prompt {
        let _ = writeln!(
            buf,
            "  This temporary Definition completed successfully. Offer to publish this hash and ask the user to choose Project or User scope. Publishing requires Workflow behavior; if the current behavior differs, direct the user to /workflow first."
        );
    }
    if run.status == crate::session::workflow::tracker::WorkflowRunStatus::Failed {
        let _ = writeln!(
            buf,
            "  Resumable: call the workflow tool with action: \"control_run\", \
                 run_id: \"{}\", operation: \"resume\" — completed agents replay from the \
                 journal and the failed step re-executes.",
            run.run_id
        );
    }
    if let Some(report_path) = report_path {
        match read_tool_name {
            Some(read_tool_name) => {
                let _ = writeln!(
                    buf,
                    "  Full report: {} (use {} on that path to view it)",
                    report_path.display(),
                    read_tool_name,
                );
            }
            None => {
                let _ = writeln!(
                    buf,
                    "  Full report stored at {}. This Agent has no file-read tool; report the path to the user if the inline summary is insufficient.",
                    report_path.display(),
                );
            }
        }
    }
    buf.push_str(
        "\nReport this outcome to the user and take the appropriate next action. Keep the run id internal; the user knows the run by display name.",
    );
    buf
}

fn format_running_task_checkpoint_notification(
    task: &tools::computer::types::TaskSnapshot,
    checkpoint_time: std::time::SystemTime,
) -> String {
    use std::fmt::Write as _;

    let command = task.display_command.as_deref().unwrap_or(&task.command);
    let kind_label = match task.kind {
        tools::computer::types::TaskKind::Bash => "",
        tools::computer::types::TaskKind::Monitor => " [monitor]",
    };
    let elapsed = checkpoint_time
        .duration_since(task.start_time)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let elapsed = if elapsed < 60 {
        format!("{elapsed}s")
    } else if elapsed < 3_600 {
        format!("{}m", elapsed / 60)
    } else {
        let hours = elapsed / 3_600;
        let minutes = (elapsed % 3_600) / 60;
        if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minutes}m")
        }
    };
    let mut body = String::from(
        "A background task was still running when this session last checkpointed and may still be in progress:\n",
    );
    let _ = writeln!(
        body,
        "- \"{}\"{} (running for {} at checkpoint): {}",
        task.task_id, kind_label, elapsed, command
    );
    let _ = writeln!(body, "  Output log: {}", task.output_file.display());
    body.push_str(
        "Check whether it is still running and inspect its output log to determine whether it completed successfully.",
    );
    body
}

fn running_task_notification_owner(
    task: &tools::computer::types::TaskSnapshot,
) -> Option<chat_state::NotificationOwner> {
    match (task.goal_id.clone(), task.goal_definition_revision) {
        (None, None) => Some(chat_state::NotificationOwner::Session),
        (Some(goal_id), Some(definition_revision)) if definition_revision > 0 => {
            Some(chat_state::NotificationOwner::Goal {
                goal_id,
                definition_revision,
            })
        }
        _ => None,
    }
}
impl SessionActor {
    /// Injects a one-shot date-rollover `<system-reminder>` when a long session crosses local
    /// midnight, since the cached `<user_info>` prefix keeps its startup date to preserve the prompt
    /// cache. Self-dedupes via `last_announced_local_date` (at most once per day).
    pub(super) async fn maybe_inject_date_rollover_reminder(&self) {
        let today = chrono::Local::now().date_naive();
        let last = self.last_announced_local_date.get();
        let Some(reminder) = date_rollover_reminder(today, last) else {
            return;
        };
        self.last_announced_local_date.set(today);
        self.push_system_reminder(&reminder);
        tracing::debug!(
            previous = %last,
            today = %today,
            "Injected date rollover reminder"
        );
    }
    /// Inject a one-shot `<system-reminder>` telling the model its previous turn
    /// was interrupted mid-stream, when nothing else will (no in-flight tool to
    /// repair into a "cancelled" tool-result, no permission tool-result). The
    /// flag is armed by [`Self::cancel_running_task`] only on the no-active-tool
    /// abort path, and is consumed exactly once (caller gates to real user
    /// prompts). Skipped for the harness that owns this surface; unlike the date-rollover reminder,
    /// no template scoping applies to an interrupt notice.
    pub(super) async fn maybe_inject_interrupt_reminder(&self) {
        if !self.events.take_pending_interrupt_reminder() {
            return;
        }
        self.push_system_reminder(INTERRUPT_REMINDER);
        tracing::debug!("Injected prior-turn interrupt reminder");
    }
    /// Push a `<system-reminder>`-wrapped user message into the conversation.
    pub(super) fn push_system_reminder(&self, content: &str) {
        self.push_system_reminder_with_tag(content, "system-reminder");
    }
    /// The active reminder wrapper tag, backed by the canonical tag constants
    /// in `tools::reminders`.
    pub(super) fn reminder_wrapper_tag(&self) -> &'static str {
        tools::reminders::DEFAULT_REMINDER_TAG
    }
    /// Push a `<{tag}>`-wrapped user message.
    pub(super) fn push_system_reminder_with_tag(&self, content: &str, tag: &str) {
        let content = content.replace(&format!("</{tag}>"), &format!("<\\/{tag}>"));
        let message = ConversationItem::system_reminder(format!("<{tag}>\n{content}\n</{tag}>"));
        self.chat_state_handle.push_user_message(message);
    }
    /// Checkpoint every running background task into the canonical durable
    /// notification inbox before the final session flush. These receipts are
    /// context for the next real turn and never autonomously wake the model.
    pub(super) async fn checkpoint_running_task_notifications(&self) {
        let mut tasks = self
            .tool_bridge_handle()
            .list_background_tasks()
            .await
            .into_iter()
            .filter(tools::computer::types::TaskSnapshot::is_outstanding)
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| left.task_id.cmp(&right.task_id));
        if tasks.is_empty() {
            return;
        }

        let checkpoint_time = std::time::SystemTime::now();
        let checkpoint_id = uuid::Uuid::now_v7().to_string();
        let task_count = tasks.len();
        let mut checkpointed = 0usize;
        for task in tasks {
            let task_kind = match task.kind {
                tools::computer::types::TaskKind::Bash => chat_state::NotificationTaskKind::Task,
                tools::computer::types::TaskKind::Monitor => {
                    chat_state::NotificationTaskKind::Monitor
                }
            };
            let Some(owner) = running_task_notification_owner(&task) else {
                tracing::error!(
                    task_id = task.task_id,
                    goal_id = ?task.goal_id,
                    goal_definition_revision = ?task.goal_definition_revision,
                    "refusing to checkpoint a task with an incomplete Goal owner"
                );
                continue;
            };
            let source = chat_state::NotificationSource::TaskStillRunning {
                task_id: task.task_id.clone(),
                task_kind,
                owner,
            };
            let body = format_running_task_checkpoint_notification(&task, checkpoint_time);
            match self
                .receive_notification(
                    source,
                    chat_state::NotificationSourceVersion::Opaque {
                        value: checkpoint_id.clone(),
                    },
                    body,
                )
                .await
            {
                Ok(_) => checkpointed += 1,
                Err(error) => {
                    tracing::warn!(
                        task_id = task.task_id,
                        %error,
                        "failed to checkpoint running task notification"
                    );
                }
            }
        }
        tracing::info!(
            checkpointed,
            failed = task_count - checkpointed,
            checkpoint_id,
            "checkpointed running background tasks into durable notifications"
        );
    }
}
#[cfg(test)]
mod workflow_reminder_tests {
    use super::*;
    use crate::session::workflow::tracker::{
        WorkflowRunState, WorkflowRunStatus, WorkflowRuntimeRoute,
    };
    fn failed_run(detail: String) -> WorkflowRunState {
        WorkflowRunState {
            run_id: "wf_1".to_owned(),
            definition_id: None,
            definition_scope: None,
            definition_hash: None,
            save_prompt: false,
            revision: 2,
            execution_epoch: 0,
            runtime_route: WorkflowRuntimeRoute::for_test(
                "test-model",
                None,
                sampling_types::ModelImageInputKey::new("test-model", "responses", "test-endpoint"),
            )
            .unwrap(),
            name: "demo".to_owned(),
            objective: "exercise formatter".to_owned(),
            status: WorkflowRunStatus::Failed,
            phases: Vec::new(),
            current_phase: None,
            agent_budget: None,
            max_concurrency: 3,
            agents_used: 0,
            agent_usage_incomplete: false,
            elapsed_ms_floor: 1_000,
            pause_message: Some(detail),
            journal_path: None,
            result_summary: None,
            agents: Vec::new(),
        }
    }
    #[test]
    fn completion_detail_is_normalized_and_utf8_safely_capped_with_marker() {
        let detail = format!(
            "first\n\tsecond   {} tail",
            "😀".repeat(WORKFLOW_RESULT_SUMMARY_REMINDER_CAP)
        );
        let run = failed_run(detail);
        let reminder = format_workflow_completion_notification(&run, None, None);
        let rendered_detail = reminder
            .split_once("  Detail: ")
            .unwrap()
            .1
            .lines()
            .next()
            .unwrap()
            .trim_end();
        assert!(reminder.contains("run_id: \"wf_1\", operation: \"resume\""));
        assert!(rendered_detail.starts_with("first second "));
        assert!(rendered_detail.ends_with('…'));
        assert!(rendered_detail.len() <= WORKFLOW_RESULT_SUMMARY_REMINDER_CAP);
        assert!(!rendered_detail.contains('\n'));
        assert!(!rendered_detail.contains('\t'));
        assert!(!rendered_detail.contains("  "));
    }

    #[test]
    fn completion_notification_contains_the_terminal_snapshot_result() {
        let mut run = failed_run(String::new());
        run.status = WorkflowRunStatus::Complete;
        run.pause_message = None;
        run.result_summary = Some("verified output from the completed workflow".to_string());
        let report_path = std::path::Path::new("/session/workflows/wf_1/scratch/report.md");

        let notification =
            format_workflow_completion_notification(&run, Some(report_path), Some("read_file"));

        assert!(notification.contains("status: complete"));
        assert!(notification.contains("verified output from the completed workflow"));
        assert!(notification.contains(report_path.to_str().unwrap()));
        assert!(notification.contains("use read_file"));
        assert!(!notification.contains("Review the workflow completion reminder"));
    }

    #[test]
    fn running_task_checkpoint_preserves_model_facing_command_and_output_path() {
        let start = std::time::UNIX_EPOCH + std::time::Duration::from_secs(10_000);
        let task = tools::computer::types::TaskSnapshot {
            goal_definition_revision: None,
            task_id: "monitor-1".into(),
            command: "internal isolation wrapper".into(),
            display_command: Some("cargo test -p shell".into()),
            cwd: "/workspace".into(),
            start_time: start,
            end_time: None,
            output: String::new(),
            output_file: "/tmp/monitor-1.log".into(),
            truncated: false,
            exit_code: None,
            signal: None,
            completed: false,
            kind: tools::computer::types::TaskKind::Monitor,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: Some("session-1".into()),
            goal_id: None,
            description: None,
            is_backgrounded: true,
        };

        let notification = format_running_task_checkpoint_notification(
            &task,
            start + std::time::Duration::from_secs(3_660),
        );

        assert!(notification.contains("\"monitor-1\" [monitor]"));
        assert!(notification.contains("running for 1h 1m at checkpoint"));
        assert!(notification.contains("cargo test -p shell"));
        assert!(!notification.contains("internal isolation wrapper"));
        assert!(notification.contains("/tmp/monitor-1.log"));
        assert!(notification.contains("may still be in progress"));
    }

    #[test]
    fn running_task_checkpoint_keeps_immutable_goal_owner() {
        let mut task = tools::computer::types::TaskSnapshot {
            goal_definition_revision: Some(1),
            task_id: "goal-monitor".into(),
            command: "watch".into(),
            display_command: None,
            cwd: "/workspace".into(),
            start_time: std::time::UNIX_EPOCH,
            end_time: None,
            output: String::new(),
            output_file: "/tmp/goal-monitor.log".into(),
            truncated: false,
            exit_code: None,
            signal: None,
            completed: false,
            kind: tools::computer::types::TaskKind::Monitor,
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: Some("session-1".into()),
            goal_id: Some("goal-1".into()),
            description: None,
            is_backgrounded: true,
        };
        assert_eq!(
            running_task_notification_owner(&task),
            Some(chat_state::NotificationOwner::Goal {
                definition_revision: 1,
                goal_id: "goal-1".into()
            })
        );
        task.goal_id = None;
        task.goal_definition_revision = None;
        assert_eq!(
            running_task_notification_owner(&task),
            Some(chat_state::NotificationOwner::Session)
        );
    }
}
