//! Pure functions for exporting a conversation transcript as human-readable Markdown.
//!
//! Used by the `/export` slash command (and its dispatch handler). The converter walks
//! `RenderBlock`s and produces `## User` / `## Assistant` / `## Tools` sections with
//! compact one-line tool summaries. Non-conversation blocks (system chrome, thinking,
//! subagent lifecycle, etc.) are intentionally skipped so the output is useful for
//! "continue elsewhere" or archival.

use super::blocks::tool::{HookRunEntry, HookRunStatus};
use super::entry::ScrollbackEntry;
use super::{RenderBlock, ToolCallBlock};

/// Convert an iterator of `RenderBlock` references into a Markdown transcript.
///
/// The output is a clean, readable document suitable for saving or clipboard:
/// - `## User` for user prompts (raw text)
/// - `## Assistant` for agent responses (prefers raw source Markdown via `copy_text(true)`)
/// - `## Tools` section with one-line summaries for every tool call kind
///
/// Consecutive assistant messages are coalesced under a single header.
/// Thinking / system / subagent / credit / etc. blocks are skipped.
///
/// This function is pure and easily unit-testable with synthetic blocks (including `Stub`).
pub fn render_blocks_to_markdown<'a>(blocks: impl IntoIterator<Item = &'a RenderBlock>) -> String {
    let mut out = String::new();
    let mut last_was_agent = false;
    let mut in_tools_section = false;

    for b in blocks {
        match b {
            RenderBlock::UserPrompt(u) => {
                if in_tools_section {
                    out.push('\n');
                    in_tools_section = false;
                }
                out.push_str("## User\n\n");
                out.push_str(&u.copy_text());
                out.push_str("\n\n");
                last_was_agent = false;
            }
            RenderBlock::AgentMessage(a) => {
                if !last_was_agent {
                    if in_tools_section {
                        out.push('\n');
                        in_tools_section = false;
                    }
                    out.push_str("## Assistant\n\n");
                }
                // Prefer raw source Markdown for fidelity in the exported .md
                out.push_str(&a.copy_text(true));
                out.push_str("\n\n");
                last_was_agent = true;
            }
            RenderBlock::ToolCall(tc) => {
                if !in_tools_section {
                    out.push_str("## Tools\n\n");
                    in_tools_section = true;
                }
                out.push_str("- ");
                out.push_str(&tool_summary(tc));
                out.push('\n');
                last_was_agent = false;
            }
            // Skip all non-conversation chrome: Thinking, System, SessionEvent, BgTask,
            // Subagent, Btw, Stub, etc. Thinking blocks are
            // treated as intra-Assistant glue (no new header).
            _ => {}
        }
    }

    let trimmed_len = out.trim_end().len();
    out.truncate(trimmed_len);
    out
}

fn tool_summary(tc: &ToolCallBlock) -> String {
    match tc {
        ToolCallBlock::Read(r) => {
            let range = r
                .line_range
                .as_ref()
                .map_or(String::new(), |lr| format!(" ({})", lr));
            format!("Read: {}{}", r.path, range)
        }
        ToolCallBlock::Edit(e) => format!("Edit: {}", e.path),
        ToolCallBlock::Execute(ex) => {
            let desc = ex
                .description
                .as_deref()
                .map_or(String::new(), |d| format!(" ({})", d));
            format!("Execute: {}{}", ex.command, desc)
        }
        ToolCallBlock::ListDir(l) => format!("ListDir: {}", l.path),
        ToolCallBlock::Search(s) => format!("Search: {}", s.pattern),
        ToolCallBlock::WebFetch(w) => format!("WebFetch: {}", w.url),
        ToolCallBlock::UseTool(u) => format!("UseTool: {}", u.tool_name),
        ToolCallBlock::IntegrationSearch(_) => "IntegrationSearch (MCP tool discovery)".into(),
        ToolCallBlock::MemorySearch(_) => "MemorySearch".into(),
        ToolCallBlock::Skill(o) | ToolCallBlock::Other(o) => format!("Tool: {}", o.name),
        ToolCallBlock::Lifecycle(_) => "Lifecycle event".into(),
    }
}

/// Full, source-faithful Markdown for the observational export and replay
/// projection. The compact `/export` renderer above intentionally stays small.
pub fn render_blocks_to_full_markdown<'a>(
    blocks: impl IntoIterator<Item = &'a RenderBlock>,
    direct_children: &[String],
) -> String {
    use super::blocks::{BgTaskKind, SubagentBlockKind};

    let mut out = String::new();
    for block in blocks {
        match block {
            RenderBlock::Stub(_) => continue,
            RenderBlock::UserPrompt(prompt) => section(&mut out, "User", &prompt.copy_text()),
            RenderBlock::AgentMessage(message) => {
                section(&mut out, "Assistant", &message.copy_text(true));
            }
            RenderBlock::Thinking(thinking) => {
                section(&mut out, "Thinking", &thinking.copy_text(true));
            }
            RenderBlock::ToolCall(tool) => {
                let title = format!("Tool · {}", tool_summary(tool));
                // The viewer's copy accessor intentionally covers only some tool
                // variants. Searchable source fields retain the stored result,
                // error and input across every variant without rendering a view.
                let content = tool.searchable_text().unwrap_or_default();
                if content.is_empty() {
                    section(&mut out, &title, "Output unavailable or not recorded.");
                } else {
                    section(&mut out, &title, &code_fence(&content));
                }
            }
            RenderBlock::Subagent(child) => {
                let status = match &child.kind {
                    SubagentBlockKind::Started => "started",
                    SubagentBlockKind::Completed { .. } => "completed",
                    SubagentBlockKind::Failed { .. } => "failed",
                    SubagentBlockKind::Cancelled { .. } => "cancelled",
                };
                let label = if child.description.is_empty() {
                    child.subagent_type.as_str()
                } else {
                    child.description.as_str()
                };
                let target = if direct_children.contains(&child.child_session_id) {
                    format!(
                        "[{}](subagents/{}/transcript.md)",
                        escape_link_label(label),
                        child.child_session_id
                    )
                } else {
                    escape_link_label(label)
                };
                section(&mut out, "Subagent", &format!("{target} · {status}"));
            }
            RenderBlock::Notice(notice) => {
                let mut body = notice.text.clone();
                if let Some(details) = &notice.details
                    && !details.is_empty()
                {
                    body.push_str("\n\n");
                    body.push_str(details);
                }
                if let Some(content) = block.copy_text(true)
                    && !content.is_empty()
                    && !body.contains(&content)
                {
                    body.push_str("\n\n");
                    body.push_str(&content);
                }
                section(&mut out, "Notice", &body);
            }
            RenderBlock::SessionEvent(event) => {
                section(&mut out, "Event", &event.event.message());
                for (name, runs) in &event.stop_hooks {
                    hook_section(&mut out, name, runs);
                }
            }
            RenderBlock::BgTask(task) => {
                let status = match task.kind {
                    BgTaskKind::Started => "running at snapshot",
                    BgTaskKind::Completed { .. } => "completed",
                    BgTaskKind::Failed { .. } => "failed",
                };
                let mut body = format!(
                    "{} · {status}\n\n{}",
                    task.task_id,
                    code_fence(&task.command)
                );
                if let Some(description) = &task.description {
                    body.push_str("\n\n");
                    body.push_str(description);
                }
                section(&mut out, "Background task", &body);
            }
            RenderBlock::SubagentPermission(permission) => {
                if let Some(content) = permission.searchable_text() {
                    section(&mut out, "Subagent permission", &content);
                }
            }
            RenderBlock::Workflow(workflow) => {
                let mut body = format!(
                    "{} · {}\n\n{}",
                    workflow.name, workflow.run_id, workflow.objective
                );
                body.push_str("\n\nStatus: ");
                body.push_str(match workflow.status {
                    super::blocks::WorkflowBlockStatus::Running => "running at snapshot",
                    super::blocks::WorkflowBlockStatus::Done { .. } => "complete",
                    super::blocks::WorkflowBlockStatus::Failed { .. } => "failed",
                    super::blocks::WorkflowBlockStatus::Cancelled { .. } => "cancelled",
                    super::blocks::WorkflowBlockStatus::Paused { .. } => "paused",
                });
                for phase in &workflow.phases {
                    body.push_str(&format!("\n- {}: {}", phase.title, phase.state));
                }
                section(&mut out, "Workflow", &body);
            }
            RenderBlock::Btw(btw) => {
                section(
                    &mut out,
                    &format!("Btw · {}", btw.question),
                    &btw.content().text(),
                );
            }
            RenderBlock::ContextInfo(info) => {
                section(&mut out, "Context", &format!("Model: {}", info.model));
            }
        }
    }
    out.trim_end().to_owned()
}

/// Full transcript with details attached to a scrollback entry rather than to
/// its block (notably tool Hook evidence).
pub fn render_entries_to_full_markdown<'a>(
    entries: impl IntoIterator<Item = &'a ScrollbackEntry>,
    direct_children: &[String],
) -> String {
    let mut out = String::new();
    for entry in entries {
        let rendered =
            render_blocks_to_full_markdown(std::iter::once(&entry.block), direct_children);
        if !rendered.is_empty() {
            out.push_str(&rendered);
            out.push_str("\n\n");
        }
        if entry.is_running && matches!(&entry.block, RenderBlock::ToolCall(_)) {
            out.push_str("Status: still running at the captured snapshot.\n\n");
        }
        if let Some(hooks) = &entry.hook_data {
            hook_section(&mut out, "pre_tool_use", &hooks.pre_hooks);
            hook_section(&mut out, "post_tool_use", &hooks.post_hooks);
            for (name, runs) in &hooks.lifecycle {
                hook_section(&mut out, name, runs);
            }
        }
    }
    out.trim_end().to_owned()
}

fn hook_section(out: &mut String, label: &str, runs: &[HookRunEntry]) {
    if runs.is_empty() {
        return;
    }
    let mut body = String::new();
    for run in runs {
        let status = match &run.status {
            HookRunStatus::Success { elapsed } => format!("success ({} ms)", elapsed.as_millis()),
            HookRunStatus::Skipped => "skipped".to_owned(),
            HookRunStatus::Blocked { detail, elapsed } => {
                format!("blocked ({} ms): {detail}", elapsed.as_millis())
            }
            HookRunStatus::Failed { error, elapsed } => {
                format!("failed ({} ms): {error}", elapsed.as_millis())
            }
        };
        body.push_str(&format!("- {} · {}\n", run.name, status));
        if let Some(output) = &run.output {
            body.push_str(&code_fence(output));
            body.push_str("\n\n");
        }
    }
    section(out, &format!("Hook · {label}"), body.trim_end());
}

fn section(out: &mut String, heading: &str, body: &str) {
    out.push_str("## ");
    out.push_str(&heading.replace('\n', " "));
    out.push_str("\n\n");
    out.push_str(body);
    out.push_str("\n\n");
}

fn code_fence(content: &str) -> String {
    let longest = content
        .split(|ch| ch != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}\n{content}\n{fence}")
}

fn escape_link_label(label: &str) -> String {
    label
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_blocks_yield_empty_string() {
        let out = render_blocks_to_markdown(std::iter::empty::<&RenderBlock>());
        assert!(out.is_empty());
    }

    #[test]
    fn full_transcript_keeps_reasoning_and_uses_safe_fences() {
        let blocks = [
            RenderBlock::user_prompt("hello"),
            RenderBlock::thinking("consider `x`"),
            RenderBlock::agent_message("answer"),
        ];
        let markdown = render_blocks_to_full_markdown(blocks.iter(), &[]);
        assert!(markdown.contains("## User\n\nhello"));
        assert!(markdown.contains("## Thinking\n\nconsider `x`"));
        assert!(markdown.contains("## Assistant\n\nanswer"));
        assert_eq!(
            code_fence("before ``` after"),
            "````\nbefore ``` after\n````"
        );
    }

    #[test]
    fn full_entry_export_keeps_tool_output_and_attached_hooks() {
        let mut tool =
            super::super::entry::ScrollbackEntry::new(RenderBlock::ToolCall(ToolCallBlock::Other(
                super::super::blocks::tool::OtherToolCallBlock::new("test_tool", "input")
                    .with_output("result"),
            )));
        tool.hook_data = Some(super::super::blocks::tool::ToolCallHookData {
            pre_hooks: vec![HookRunEntry {
                name: "audit".into(),
                status: HookRunStatus::Success {
                    elapsed: std::time::Duration::from_millis(12),
                },
                output: Some("allowed".into()),
            }],
            ..Default::default()
        });
        let markdown = render_entries_to_full_markdown([&tool], &[]);
        assert!(markdown.contains("result"));
        assert!(markdown.contains("Hook · pre_tool_use"));
        assert!(markdown.contains("allowed"));
    }

    #[test]
    fn full_entry_export_marks_unfinished_tool_as_snapshot_state() {
        let tool = super::super::entry::ScrollbackEntry::running(RenderBlock::ToolCall(
            ToolCallBlock::Other(super::super::blocks::tool::OtherToolCallBlock::new(
                "test_tool",
                "input",
            )),
        ));
        let markdown = render_entries_to_full_markdown([&tool], &[]);
        assert!(markdown.contains("still running at the captured snapshot"));
        assert!(!markdown.contains("completed"));
    }
}
