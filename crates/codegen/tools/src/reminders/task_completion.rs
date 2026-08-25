//! Rendering and tool-result recognition for durable task notifications.

use crate::bridge::ToolBridge;
use crate::implementations::grow_build::task::types::SubagentCompletionSummary;
use crate::types::TaskSnapshot;
use crate::types::output::ToolOutput;
use crate::types::tool::ToolKind;
use crate::util::truncate::{PREVIEW_SIZE, truncate_with_preview};
use tool_types::{KillTaskOutput, SubagentCompletedOutput, TaskOutputOutput};

pub const DEFAULT_TASK_OUTPUT_TOOL: &str = "get_task_output";

/// Bash output is recoverable from its file, so its inline notification may be
/// bounded. Subagent output has no equivalent artifact and remains verbatim.
const MAX_INLINE_COMPLETION_BYTES: usize = 4_000;

pub fn format_bash_completion(
    task: &TaskSnapshot,
    task_output_name: Option<&str>,
    read_tool_name: Option<&str>,
) -> String {
    let command = task.display_command.as_deref().unwrap_or(&task.command);
    let duration_secs = task.duration_secs();
    let status = match task.signal.as_deref() {
        Some(signal) => format!("terminated by signal {signal}"),
        None => format!(
            "exit code: {}",
            task.exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".into())
        ),
    };
    let mut message = format!(
        "Background task \"{}\" completed ({}).\n\
         Command: {} | Duration: {:.1}s\n",
        task.task_id, status, command, duration_secs,
    );
    if task.signal.is_some() && duration_secs < 1.0 {
        message.push_str(
            "Note: this is much shorter than expected for a backgrounded command. \
             The wrapper bash may have been killed by signal (e.g. `pkill -f <pat>` \
             matching its own argv) before the inner command ran. Re-check the \
             command for self-matching kill patterns, signals sent by the script \
             itself, or upstream sources of SIGTERM/SIGHUP.\n",
        );
    }
    let disk_pointer_footer = read_tool_name.map(|name| {
        format!(
            "Use {} on {} for full content",
            name,
            task.output_file.display()
        )
    });
    render_completion_output_delivery(
        &mut message,
        &task.task_id,
        &task.output,
        task_output_name,
        disk_pointer_footer.as_deref(),
    );
    message
}

pub fn format_monitor_completion(task: &TaskSnapshot, task_output_name: Option<&str>) -> String {
    let reason = match task.signal.as_deref() {
        Some(signal) => format!("killed by signal {signal}"),
        None => match task.exit_code {
            Some(code) => format!("exited (code {code})"),
            None => "ended".to_string(),
        },
    };
    let description = task
        .display_command
        .as_deref()
        .and_then(|display| display.strip_prefix("[monitor] "))
        .unwrap_or("monitor");
    let tool = task_output_name.unwrap_or(DEFAULT_TASK_OUTPUT_TOOL);
    format!(
        "Monitor \"{id}\" ended: [monitor ended: {reason}].\n\
         Description: {description}\n\
         Command: {command}\n\
         Duration: {duration:.1}s\n\
         Use {tool}(\"{id}\") for full output.",
        id = task.task_id,
        command = task.command,
        duration = task.duration_secs(),
    )
}

fn split_wrapped_monitor_event(event_text: &str) -> Option<(&str, &str)> {
    let rest = event_text.strip_prefix("<monitor-event description=\"")?;
    let open_end = rest.find(">\n")?;
    let open_tag = &rest[..open_end];
    let description_end = open_tag.rfind("\" task_id=\"")?;
    let description = &open_tag[..description_end];
    let inner = rest[open_end + 2..].strip_suffix("\n</monitor-event>")?;
    Some((description, inner))
}

/// Render durable monitor progress as one hidden model input. Events retain
/// first-seen monitor order and within-monitor order without repeating the
/// description on every line.
pub fn format_monitor_events(
    events: &[crate::implementations::grow_build::monitor::types::MonitorEventNotification],
    task_output_name: Option<&str>,
) -> Option<String> {
    use std::fmt::Write as _;

    let tool_hint = task_output_name.unwrap_or(DEFAULT_TASK_OUTPUT_TOOL);
    match events {
        [] => None,
        [event] => {
            let (label, inner) = match split_wrapped_monitor_event(&event.event_text) {
                Some((description, inner)) if !description.is_empty() => (description, inner),
                Some((_, inner)) => ("event", inner),
                None => ("event", event.event_text.as_str()),
            };
            let label =
                crate::implementations::grow_build::monitor::event::sanitize_monitor_description(
                    label,
                );
            Some(format!(
                "<monitor-event task_id=\"{}\">\n[{}] {}\n</monitor-event>",
                event.task_id, label, inner,
            ))
        }
        _ => {
            type Event =
                crate::implementations::grow_build::monitor::types::MonitorEventNotification;
            let mut groups: Vec<(&str, Vec<&Event>)> = Vec::new();
            for event in events {
                match groups.iter_mut().find(|(id, _)| *id == event.task_id) {
                    Some((_, group)) => group.push(event),
                    None => groups.push((&event.task_id, vec![event])),
                }
            }
            let mut buffer = format!(
                "{} monitor events from {} {} (use {} to identify each monitor):",
                events.len(),
                groups.len(),
                if groups.len() == 1 {
                    "monitor"
                } else {
                    "monitors"
                },
                tool_hint,
            );
            for (task_id, group) in &groups {
                let description = group
                    .iter()
                    .find_map(|event| split_wrapped_monitor_event(&event.event_text))
                    .map(|(description, _)| description)
                    .filter(|description| !description.is_empty())
                    .unwrap_or("event");
                let description = crate::implementations::grow_build::monitor::event::sanitize_monitor_description(
                    description,
                );
                let _ = write!(
                    buffer,
                    "\n\n<monitor description=\"{description}\" task_id=\"{task_id}\">"
                );
                for (index, event) in group.iter().enumerate() {
                    let inner = split_wrapped_monitor_event(&event.event_text)
                        .map(|(_, inner)| inner)
                        .unwrap_or(&event.event_text);
                    let _ = write!(buffer, "\n[{}] {}", index + 1, inner);
                }
                buffer.push_str("\n</monitor>");
            }
            Some(buffer)
        }
    }
}

pub fn render_completion_output_delivery(
    buffer: &mut String,
    subject_id: &str,
    output: &str,
    task_output_name: Option<&str>,
    disk_pointer_footer: Option<&str>,
) {
    use std::fmt::Write as _;

    match task_output_name {
        Some(name) => {
            let _ = write!(
                buffer,
                "Use {name}(\"{subject_id}\") to see the full output."
            );
        }
        None => match disk_pointer_footer {
            Some(footer) => {
                let (output, _) = truncate_with_preview(
                    output,
                    MAX_INLINE_COMPLETION_BYTES,
                    PREVIEW_SIZE,
                    Some(footer),
                );
                let _ = write!(buffer, "response:\n{output}");
            }
            None => {
                let _ = write!(buffer, "response:\n{output}");
            }
        },
    }
}

pub async fn resolve_task_output_tool_name(bridge: &ToolBridge) -> Option<String> {
    bridge.tool_for_kind(ToolKind::BackgroundTaskAction).await
}

pub async fn resolve_read_tool_name(bridge: &ToolBridge) -> Option<String> {
    bridge.tool_for_kind(ToolKind::Read).await
}

pub fn format_subagent_completion(
    completion: &SubagentCompletionSummary,
    task_output_name: Option<&str>,
) -> String {
    let status = if completion.success {
        "successfully"
    } else {
        "with failure"
    };
    let mut output = format!(
        "Background subagent \"{}\" ({}: \"{}\") completed {}.\n\
         Duration: {:.1}s | Tool calls: {} | Turns: {}",
        completion.subagent_id,
        completion.subagent_type,
        completion.description,
        status,
        completion.duration_ms as f64 / 1000.0,
        completion.tool_calls,
        completion.turns,
    );
    output.push_str(match task_output_name {
        Some(_) => "\n",
        None => "\n\n",
    });
    render_completion_output_delivery(
        &mut output,
        &completion.subagent_id,
        &completion.output,
        task_output_name,
        None,
    );
    output
}

fn task_text_agent_id(text: &str) -> Option<&str> {
    if !text.starts_with("This is the output of the subagent:") {
        return None;
    }
    let after = text.split_once("\nAgent ID: ")?.1;
    let end = after
        .find(|character: char| character.is_whitespace())
        .unwrap_or(after.len());
    (end != 0).then_some(&after[..end])
}

/// Identify completion facts already surfaced by a tool result so the shell
/// can atomically acknowledge their durable notification receipts.
pub fn consumed_completion_ids(output: &ToolOutput) -> Vec<&str> {
    let mut ids = Vec::new();
    if let ToolOutput::Text(text) = output
        && let Some(id) = task_text_agent_id(&text.text)
    {
        ids.push(id);
    }
    match output {
        ToolOutput::TaskOutput(TaskOutputOutput::Result(result))
            if crate::implementations::grow_build::task_output::is_terminal_status(
                &result.status,
            ) =>
        {
            ids.push(result.task_id.as_str());
        }
        ToolOutput::TaskOutput(TaskOutputOutput::Result(_)) => {}
        ToolOutput::TaskOutput(TaskOutputOutput::MultiResult(results)) => {
            ids.extend(
                results
                    .results
                    .iter()
                    .filter(|result| {
                        crate::implementations::grow_build::task_output::is_terminal_status(
                            &result.status,
                        )
                    })
                    .map(|result| result.task_id.as_str()),
            );
        }
        ToolOutput::TaskOutput(TaskOutputOutput::TaskNotFound(_)) => {}
        ToolOutput::KillTask(KillTaskOutput::Result(result)) => {
            ids.push(result.task_id.as_str());
        }
        ToolOutput::KillTask(KillTaskOutput::TaskNotFound(_)) => {}
        ToolOutput::SubagentCompleted(SubagentCompletedOutput { subagent_id, .. }) => {
            ids.push(subagent_id.as_str());
        }
        ToolOutput::Text(text) => {
            if let Some(id) = text.consumed_completion_task_id.as_deref() {
                ids.push(id);
            }
        }
        ToolOutput::Bash(_)
        | ToolOutput::BackgroundTaskStarted(_)
        | ToolOutput::GrepSearch(_)
        | ToolOutput::ReadFile(_)
        | ToolOutput::ListDir(_)
        | ToolOutput::SearchReplace(_)
        | ToolOutput::Todo(_)
        | ToolOutput::WebFetch(_)
        | ToolOutput::MCP(_)
        | ToolOutput::Skill(_)
        | ToolOutput::SearchTool(_)
        | ToolOutput::PlanControl(_)
        | ToolOutput::AskUserQuestion(_)
        | ToolOutput::Monitor(_)
        | ToolOutput::SchedulerCreate(_)
        | ToolOutput::SchedulerDelete(_)
        | ToolOutput::SchedulerList(_)
        | ToolOutput::CreateGoal(_)
        | ToolOutput::UpdateGoal(_)
        | ToolOutput::GetGoal(_)
        | ToolOutput::ContextRecall(_)
        | ToolOutput::Workflow(_)
        | ToolOutput::Dynamic(_) => {}
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementations::grow_build::monitor::types::MonitorEventNotification;
    use crate::types::output::TextOutput;

    fn task_snapshot(id: &str, output: &str) -> TaskSnapshot {
        TaskSnapshot {
            task_id: id.into(),
            command: "echo test".into(),
            display_command: None,
            cwd: String::new(),
            start_time: std::time::SystemTime::now(),
            end_time: Some(std::time::SystemTime::now()),
            output: output.into(),
            output_file: "/tmp/task.log".into(),
            truncated: false,
            exit_code: Some(0),
            signal: None,
            completed: true,
            kind: Default::default(),
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: Some("session".into()),
            goal_id: None,
            description: None,
            is_backgrounded: true,
        }
    }

    #[test]
    fn text_tool_result_identifies_consumed_completion() {
        let output = ToolOutput::Text(TextOutput {
            text: "done".into(),
            consumed_completion_task_id: Some("task-1".into()),
        });
        assert_eq!(consumed_completion_ids(&output), vec!["task-1"]);
    }

    #[test]
    fn task_form_subagent_result_identifies_consumed_completion() {
        let output = ToolOutput::Text(TextOutput {
            text: "This is the output of the subagent:\n\nresponse:\nresult\n\nAgent ID: subagent-1 (resume later)".into(),
            consumed_completion_task_id: None,
        });
        assert_eq!(consumed_completion_ids(&output), vec!["subagent-1"]);
    }

    #[test]
    fn every_terminal_task_output_consumes_its_completion_receipt() {
        let result = |id: &str, status: &str| tool_types::TaskOutputResult {
            task_id: id.into(),
            command: String::new(),
            status: status.into(),
            exit_code: None,
            started: String::new(),
            ended: None,
            duration_secs: 0.0,
            output: String::new(),
            output_file: String::new(),
            truncated: false,
            truncation_hint: String::new(),
            raw_output_bytes: 0,
        };
        for status in ["completed", "failed", "cancelled", "timed_out"] {
            let output = ToolOutput::TaskOutput(TaskOutputOutput::Result(result("task", status)));
            assert_eq!(consumed_completion_ids(&output), vec!["task"]);
        }
        let running = ToolOutput::TaskOutput(TaskOutputOutput::Result(result("task", "running")));
        assert!(consumed_completion_ids(&running).is_empty());
    }

    #[test]
    fn bash_pointer_and_inline_delivery_are_exclusive() {
        let task = task_snapshot("task-1", &"x".repeat(MAX_INLINE_COMPLETION_BYTES * 2));
        let pointer = format_bash_completion(&task, Some("task_output"), Some("read_file"));
        assert!(pointer.contains("task_output(\"task-1\")"));
        assert!(!pointer.contains("response:"));

        let inline = format_bash_completion(&task, None, Some("read_file"));
        assert!(inline.contains("[Output truncated"));
        assert!(inline.contains("Use read_file on /tmp/task.log for full content"));
    }

    #[test]
    fn subagent_inline_output_is_not_truncated() {
        let large_output = "y".repeat(MAX_INLINE_COMPLETION_BYTES * 3);
        let completion = SubagentCompletionSummary {
            subagent_id: "subagent-1".into(),
            subagent_type: "general-purpose".into(),
            description: "inspect".into(),
            success: true,
            duration_ms: 10,
            tool_calls: 2,
            turns: 1,
            output: std::sync::Arc::from(large_output.as_str()),
        };
        let message = format_subagent_completion(&completion, None);
        assert!(message.contains(&large_output));
        assert!(!message.contains("[Output truncated"));
    }

    #[test]
    fn monitor_batch_groups_by_task_without_repeating_description() {
        let event = |task_id: &str, text: &str| MonitorEventNotification {
            task_id: task_id.into(),
            event_text: format!(
                "<monitor-event description=\"heartbeat\" task_id=\"{task_id}\">\n{text}\n</monitor-event>"
            ),
        };
        let rendered = format_monitor_events(
            &[event("monitor-1", "first"), event("monitor-1", "second")],
            None,
        )
        .expect("events render");
        assert_eq!(rendered.matches("description=\"heartbeat\"").count(), 1);
        assert!(rendered.contains("[1] first\n[2] second"));
    }
}
