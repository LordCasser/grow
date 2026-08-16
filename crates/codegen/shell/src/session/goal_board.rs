//! Canonical Goal blackboard parser, host-side plan assembler, and typed
//! progress patcher.
//!
//! Markdown is the only durable task state. This module is the single write
//! boundary: every planner document and primary-Agent progress patch is parsed
//! and validated before it can be persisted or projected to the pager.

use std::collections::{HashMap, HashSet};

use tool_types::{
    GoalPlanAssemblyError, GoalPlanAssemblyIssue, GoalPlanSectionPayload, GoalPlanSpec,
    GoalPlanTaskSpec, GoalProgressUpdate, GoalTaskProjection, GoalTaskStatus,
};
use unicode_width::UnicodeWidthStr;

const MAX_BOARD_BYTES: usize = 64 * 1024;
const MAX_TASKS: usize = 128;
const MAX_DEPTH: usize = 4;
const MAX_SUMMARY_COLUMNS: usize = 160;
const MAX_METADATA_BYTES: usize = 4096;

const HEADINGS: [&str; 5] = [
    "# Goal",
    "## Plan",
    "## Goal acceptance",
    "## Verification evidence",
    "## Open gaps",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoalBoardError(pub String);

impl std::fmt::Display for GoalBoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GoalBoardError {}

#[derive(Debug, Clone)]
struct ParsedTask {
    id: String,
    parent_id: Option<String>,
    depth: usize,
    indent: usize,
    status: GoalTaskStatus,
    summary: String,
    line: usize,
}

#[derive(Debug, Clone)]
pub struct ParsedGoalBoard {
    markdown: String,
    tasks: Vec<ParsedTask>,
    plan_end: usize,
}

impl ParsedGoalBoard {
    pub fn markdown(&self) -> &str {
        &self.markdown
    }

    pub fn task_projection(&self) -> Vec<GoalTaskProjection> {
        self.tasks
            .iter()
            .map(|task| {
                let descendants = self
                    .tasks
                    .iter()
                    .filter(|candidate| candidate.id.starts_with(&format!("{}.", task.id)));
                let (completed_descendants, total_descendants) =
                    descendants.fold((0_u32, 0_u32), |(done, total), descendant| {
                        (
                            done + u32::from(descendant.status == GoalTaskStatus::Done),
                            total + 1,
                        )
                    });
                GoalTaskProjection {
                    id: task.id.clone(),
                    parent_id: task.parent_id.clone(),
                    depth: task.depth as u8,
                    status: task.status,
                    summary: task.summary.clone(),
                    completed_descendants,
                    total_descendants,
                }
            })
            .collect()
    }
}

/// Remove only an outer Markdown transport fence. Inner code blocks remain
/// part of the canonical document and are validated/rendered normally.
pub fn normalize_goal_board_markdown(markdown: impl Into<String>) -> String {
    let markdown = markdown.into();
    let trimmed = markdown.trim();
    let Some((opening, rest)) = trimmed.split_once('\n') else {
        return trimmed.to_string();
    };
    if !matches!(
        opening.trim().to_ascii_lowercase().as_str(),
        "```markdown" | "```md"
    ) {
        return trimmed.to_string();
    }
    let Some(body) = rest.strip_suffix("```") else {
        return trimmed.to_string();
    };
    body.trim().to_string()
}

pub fn parse_goal_board(
    objective: &str,
    markdown: impl Into<String>,
) -> Result<ParsedGoalBoard, GoalBoardError> {
    let markdown = normalize_goal_board_markdown(markdown);
    if markdown.len() > MAX_BOARD_BYTES {
        return Err(GoalBoardError(format!(
            "Goal blackboard exceeds {MAX_BOARD_BYTES} bytes"
        )));
    }
    let lines: Vec<&str> = markdown.lines().collect();
    let structural = structural_line_mask(&lines)?;

    let mut heading_lines = Vec::with_capacity(HEADINGS.len());
    for (index, heading) in HEADINGS.iter().enumerate() {
        let matches: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(line, value)| structural[*line] && value.trim_end() == *heading)
            .map(|(line, _)| line)
            .collect();
        if matches.len() != 1 {
            return Err(GoalBoardError(format!(
                "Goal blackboard must contain exactly one `{heading}` heading"
            )));
        }
        let line = matches[0];
        if index == 0 && line != 0 {
            return Err(GoalBoardError("`# Goal` must be the first line".into()));
        }
        if heading_lines
            .last()
            .is_some_and(|previous| *previous >= line)
        {
            return Err(GoalBoardError(
                "Goal blackboard headings are out of canonical order".into(),
            ));
        }
        heading_lines.push(line);
    }
    for (line, value) in lines.iter().enumerate() {
        if structural[line]
            && (value.starts_with("# ") || value.starts_with("## "))
            && !HEADINGS.contains(value)
        {
            return Err(GoalBoardError(format!(
                "unexpected top-level Goal heading on line {}",
                line + 1
            )));
        }
    }

    validate_objective(objective, &lines[1..heading_lines[1]])?;

    let plan_start = heading_lines[1] + 1;
    let plan_end = heading_lines[2];
    let mut tasks = Vec::new();
    let mut ids = HashSet::new();
    for line in plan_start..plan_end {
        if !structural[line] {
            continue;
        }
        let value = lines[line];
        if looks_like_checkbox(value) {
            let task = parse_task_line(value, line)?;
            if tasks.len() == MAX_TASKS {
                return Err(GoalBoardError(format!(
                    "Goal blackboard may contain at most {MAX_TASKS} tasks"
                )));
            }
            if !ids.insert(task.id.clone()) {
                return Err(GoalBoardError(format!(
                    "duplicate Goal task id `{}`",
                    task.id
                )));
            }
            if let Some(parent) = task.parent_id.as_ref()
                && !ids.contains(parent)
            {
                return Err(GoalBoardError(format!(
                    "Goal task `{}` appears before or without parent `{parent}`",
                    task.id
                )));
            }
            tasks.push(task);
        }
    }
    if !tasks.iter().any(|task| task.depth == 1) {
        return Err(GoalBoardError(
            "Goal Plan must contain at least one top-level task".into(),
        ));
    }

    for (line, value) in lines.iter().enumerate() {
        if structural[line] && looks_like_checkbox(value) && !(plan_start..plan_end).contains(&line)
        {
            return Err(GoalBoardError(format!(
                "checkboxes are only allowed in `## Plan` (line {})",
                line + 1
            )));
        }
    }
    for task in tasks
        .iter()
        .filter(|task| task.status == GoalTaskStatus::Done)
    {
        if tasks.iter().any(|candidate| {
            candidate.id.starts_with(&format!("{}.", task.id))
                && candidate.status != GoalTaskStatus::Done
        }) {
            return Err(GoalBoardError(format!(
                "done Goal task `{}` has an unfinished descendant",
                task.id
            )));
        }
    }

    Ok(ParsedGoalBoard {
        markdown,
        tasks,
        plan_end,
    })
}

pub fn apply_progress_updates(
    objective: &str,
    markdown: &str,
    updates: &[GoalProgressUpdate],
) -> Result<ParsedGoalBoard, GoalBoardError> {
    if updates.is_empty() {
        return Err(GoalBoardError(
            "at least one Goal progress update is required".into(),
        ));
    }
    let parsed = parse_goal_board(objective, markdown.to_string())?;
    let mut by_id: HashMap<&str, &ParsedTask> = parsed
        .tasks
        .iter()
        .map(|task| (task.id.as_str(), task))
        .collect();
    let mut seen = HashSet::new();
    let mut ordered = Vec::with_capacity(updates.len());
    for update in updates {
        if !seen.insert(update.task_id.as_str()) {
            return Err(GoalBoardError(format!(
                "duplicate progress update for `{}`",
                update.task_id
            )));
        }
        let Some(task) = by_id.remove(update.task_id.as_str()) else {
            return Err(GoalBoardError(format!(
                "unknown Goal task id `{}`",
                update.task_id
            )));
        };
        if update.status.is_none()
            && update.progress.is_none()
            && update.evidence.is_none()
            && update.gap.is_none()
        {
            return Err(GoalBoardError(format!(
                "progress update for `{}` changes no fields",
                update.task_id
            )));
        }
        for (field, value) in [
            ("Progress", update.progress.as_deref()),
            ("Evidence", update.evidence.as_deref()),
            ("Gap", update.gap.as_deref()),
        ] {
            if let Some(value) = value {
                validate_patch_text(field, value)?;
            }
        }
        ordered.push((task, update));
    }
    ordered.sort_by_key(|(task, _)| std::cmp::Reverse(task.line));

    let mut lines: Vec<String> = parsed.markdown.lines().map(str::to_owned).collect();
    for (task, update) in ordered {
        if let Some(status) = update.status {
            let checked = if status == GoalTaskStatus::Done {
                'x'
            } else {
                ' '
            };
            lines[task.line] = format!(
                "{}- [{checked}] **{}** `{}` — {}",
                " ".repeat(task.indent),
                task.id,
                status.as_str(),
                task.summary
            );
        }

        let block_end = parsed
            .tasks
            .iter()
            .find(|candidate| candidate.line > task.line && candidate.depth <= task.depth)
            .map(|candidate| candidate.line)
            .unwrap_or(parsed.plan_end);
        let child_start = parsed
            .tasks
            .iter()
            .find(|candidate| candidate.line > task.line && candidate.line < block_end)
            .map(|candidate| candidate.line)
            .unwrap_or(block_end);
        let mut insert_at = child_start;
        let field_indent = " ".repeat(task.indent + 2);
        let mut missing = Vec::new();
        for (label, value) in [
            ("Progress", update.progress.as_deref()),
            ("Evidence", update.evidence.as_deref()),
            ("Gap", update.gap.as_deref()),
        ] {
            let Some(value) = value else { continue };
            let prefix = format!("{field_indent}- {label}:");
            if let Some(line) =
                (task.line + 1..child_start).find(|line| lines[*line].starts_with(&prefix))
            {
                lines[line] = format!("{prefix} {}", value.trim());
            } else {
                missing.push(format!("{prefix} {}", value.trim()));
            }
        }
        for line in missing {
            lines.insert(insert_at, line);
            insert_at += 1;
        }
    }

    parse_goal_board(objective, lines.join("\n"))
}

/// Replace only runtime-owned evidence/gap sections. Verifier output is data,
/// never executable Markdown task structure.
pub fn apply_runtime_feedback(
    objective: &str,
    markdown: &str,
    verification_evidence: Option<&str>,
    open_gaps: Option<&str>,
) -> Result<ParsedGoalBoard, GoalBoardError> {
    let parsed = parse_goal_board(objective, markdown.to_string())?;
    let mut lines: Vec<String> = parsed.markdown.lines().map(str::to_owned).collect();
    for (heading, next_heading, value) in [
        ("## Open gaps", None, open_gaps),
        (
            "## Verification evidence",
            Some("## Open gaps"),
            verification_evidence,
        ),
    ] {
        let Some(value) = value else { continue };
        let start = lines
            .iter()
            .position(|line| line == heading)
            .expect("validated board contains runtime heading");
        let end = next_heading
            .and_then(|next| lines.iter().position(|line| line == next))
            .unwrap_or(lines.len());
        let value = value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace("[ ]", "\\[ \\]")
            .replace("[x]", "\\[x\\]")
            .replace("[X]", "\\[X\\]");
        lines.splice(
            start + 1..end,
            [String::new(), format!("- {value}"), String::new()],
        );
    }
    parse_goal_board(objective, lines.join("\n"))
}

/// Assemble the canonical Goal blackboard Markdown from a structured plan.
///
/// The planner submits pure data: task ids, indentation, and every piece of
/// document syntax are derived here by document-order depth-first traversal,
/// so an assembled board is canonical by construction. Every violated rule is
/// reported as a structured, entry-addressed issue and all issues are
/// aggregated into one error — invalid input is never truncated or partially
/// rendered. The assembled document is finally re-parsed through
/// [`parse_goal_board`]; a runtime inconsistency is returned as an error
/// rather than bypassed.
pub fn assemble_goal_board(
    objective: &str,
    spec: &GoalPlanSpec,
) -> Result<String, GoalPlanAssemblyError> {
    let mut items = Vec::new();

    let mut plan_tasks: Option<&[GoalPlanTaskSpec]> = None;
    let mut goal_acceptance: Option<&[String]> = None;
    let mut open_gaps: Option<&[String]> = None;
    for (index, section) in spec.sections.iter().enumerate() {
        let (duplicate, name) = match section {
            GoalPlanSectionPayload::PlanTasks { tasks } => {
                (plan_tasks.replace(tasks.as_slice()).is_some(), "plan_tasks")
            }
            GoalPlanSectionPayload::GoalAcceptance { items } => (
                goal_acceptance.replace(items.as_slice()).is_some(),
                "goal_acceptance",
            ),
            GoalPlanSectionPayload::OpenGaps { items } => {
                (open_gaps.replace(items.as_slice()).is_some(), "open_gaps")
            }
        };
        if duplicate {
            items.push(assembly_issue(
                format!("sections[{index}]"),
                format!("duplicate `{name}` section; each section may appear at most once"),
            ));
        }
    }

    if let Some(tasks) = plan_tasks {
        if tasks.is_empty() {
            items.push(assembly_issue(
                "tasks".into(),
                "at least one plan task is required".into(),
            ));
        }
        let mut sequence = 0_usize;
        validate_task_tree(tasks, 1, "tasks", &mut sequence, &mut items);
    } else {
        items.push(assembly_issue(
            "sections".into(),
            "required `plan_tasks` section is missing".into(),
        ));
    }

    if let Some(acceptance) = goal_acceptance {
        if acceptance.is_empty() {
            items.push(assembly_issue(
                "goal_acceptance.items".into(),
                "at least one Goal acceptance item is required".into(),
            ));
        }
        for (index, item) in acceptance.iter().enumerate() {
            validate_single_line(
                format!("goal_acceptance.items[{index}]"),
                "Goal acceptance item",
                item,
                &mut items,
            );
        }
    } else {
        items.push(assembly_issue(
            "sections".into(),
            "required `goal_acceptance` section is missing".into(),
        ));
    }

    if let Some(gaps) = open_gaps {
        for (index, item) in gaps.iter().enumerate() {
            validate_single_line(
                format!("open_gaps.items[{index}]"),
                "Open gap item",
                item,
                &mut items,
            );
        }
    }

    let (tasks, acceptance) = match (plan_tasks, goal_acceptance) {
        (Some(tasks), Some(acceptance)) if items.is_empty() => (tasks, acceptance),
        _ => return Err(GoalPlanAssemblyError { items }),
    };

    let mut lines = vec!["# Goal".to_string(), String::new()];
    lines.extend(objective.lines().map(|line| format!("> {line}")));
    lines.push(String::new());
    lines.push("## Plan".to_string());
    lines.push(String::new());
    render_task_tree(&mut lines, tasks, "T", 1);
    lines.push(String::new());
    lines.push("## Goal acceptance".to_string());
    lines.push(String::new());
    for item in acceptance {
        lines.push(format!("- {}", escape_checkbox_markers(item.trim())));
    }
    lines.push(String::new());
    lines.push("## Verification evidence".to_string());
    lines.push(String::new());
    lines.push("- Pending".to_string());
    lines.push(String::new());
    lines.push("## Open gaps".to_string());
    lines.push(String::new());
    match open_gaps {
        Some(gaps) if !gaps.is_empty() => {
            for item in gaps {
                lines.push(format!("- {}", escape_checkbox_markers(item.trim())));
            }
        }
        _ => lines.push("- None".to_string()),
    }

    let board = lines.join("\n");
    if let Err(error) = parse_goal_board(objective, board.clone()) {
        return Err(GoalPlanAssemblyError {
            items: vec![assembly_issue(
                "board".into(),
                format!("assembled board failed canonical validation: {error}"),
            )],
        });
    }
    Ok(board)
}

fn assembly_issue(path: String, reason: String) -> GoalPlanAssemblyIssue {
    GoalPlanAssemblyIssue { path, reason }
}

/// Depth-first validation in document order. `depth` is 1-based; `sequence`
/// counts every visited task so the plan-wide task limit can be attributed to
/// the exact entries that exceed it. `status` legality needs no check here:
/// the `GoalTaskStatus` type rejects invalid tokens at deserialization.
fn validate_task_tree(
    tasks: &[GoalPlanTaskSpec],
    depth: usize,
    path: &str,
    sequence: &mut usize,
    items: &mut Vec<GoalPlanAssemblyIssue>,
) {
    for (index, task) in tasks.iter().enumerate() {
        *sequence += 1;
        let task_path = format!("{path}[{index}]");
        if *sequence > MAX_TASKS {
            items.push(assembly_issue(
                task_path.clone(),
                format!("plan exceeds the maximum of {MAX_TASKS} tasks"),
            ));
        }
        if depth > MAX_DEPTH {
            items.push(assembly_issue(
                task_path.clone(),
                format!("task nesting exceeds the maximum depth of {MAX_DEPTH}"),
            ));
        }
        let summary = task.summary.trim();
        if summary.is_empty() {
            items.push(assembly_issue(
                format!("{task_path}.summary"),
                "task summary must not be empty".into(),
            ));
        } else if UnicodeWidthStr::width(summary) > MAX_SUMMARY_COLUMNS {
            items.push(assembly_issue(
                format!("{task_path}.summary"),
                format!(
                    "task summary must span at most {MAX_SUMMARY_COLUMNS} display columns, found {}",
                    UnicodeWidthStr::width(summary)
                ),
            ));
        }
        for (field, value) in [
            ("scope", &task.scope),
            ("acceptance", &task.acceptance),
            ("evidence", &task.evidence),
            ("gap", &task.gap),
        ] {
            if let Some(value) = value {
                validate_single_line(
                    format!("{task_path}.{field}"),
                    &format!("task {field}"),
                    value,
                    items,
                );
            }
        }
        validate_task_tree(
            &task.children,
            depth + 1,
            &format!("{task_path}.children"),
            sequence,
            items,
        );
    }
}

/// One non-empty, single-line, size-bounded text field. Mirrors the text
/// limits `apply_progress_updates` imposes on board patch fields.
fn validate_single_line(
    path: String,
    label: &str,
    value: &str,
    items: &mut Vec<GoalPlanAssemblyIssue>,
) {
    if value.trim().is_empty() || value.contains(['\n', '\r']) || value.len() > MAX_METADATA_BYTES {
        items.push(assembly_issue(
            path,
            format!(
                "{label} must be a non-empty single line of at most {MAX_METADATA_BYTES} bytes"
            ),
        ));
    }
}

/// Depth-first rendering in document order. Ids are host-assigned as
/// `T1`, `T1.1`, `T1.2`, `T2`, … and indentation is derived from depth, so
/// the output matches `parse_task_line` byte for byte. Metadata sub-lines use
/// the same `  - Label: value` shape `apply_progress_updates` rewrites.
fn render_task_tree(
    lines: &mut Vec<String>,
    tasks: &[GoalPlanTaskSpec],
    prefix: &str,
    depth: usize,
) {
    for (index, task) in tasks.iter().enumerate() {
        let id = format!("{prefix}{}", index + 1);
        let indent = " ".repeat((depth - 1) * 2);
        let status = task.status.unwrap_or(GoalTaskStatus::Pending);
        let checked = if status == GoalTaskStatus::Done {
            'x'
        } else {
            ' '
        };
        lines.push(format!(
            "{indent}- [{checked}] **{id}** `{}` — {}",
            status.as_str(),
            task.summary.trim()
        ));
        for (label, value) in [
            ("Scope", &task.scope),
            ("Acceptance", &task.acceptance),
            ("Evidence", &task.evidence),
            ("Gap", &task.gap),
        ] {
            if let Some(value) = value {
                lines.push(format!("{indent}  - {label}: {}", value.trim()));
            }
        }
        render_task_tree(lines, &task.children, &format!("{id}."), depth + 1);
    }
}

/// Checkbox-looking prefixes would turn list items into fake task state and
/// be rejected by `parse_goal_board`; escape them exactly like
/// `apply_runtime_feedback` escapes runtime-owned section text.
fn escape_checkbox_markers(value: &str) -> String {
    value
        .replace("[ ]", "\\[ \\]")
        .replace("[x]", "\\[x\\]")
        .replace("[X]", "\\[X\\]")
}

fn validate_objective(objective: &str, lines: &[&str]) -> Result<(), GoalBoardError> {
    let actual: Vec<String> = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.strip_prefix('>')
                .map(|value| value.strip_prefix(' ').unwrap_or(value).to_string())
                .ok_or_else(|| {
                    GoalBoardError("Goal objective must be encoded as blockquote lines".into())
                })
        })
        .collect::<Result<_, _>>()?;
    let expected: Vec<String> = objective.lines().map(str::to_owned).collect();
    if actual != expected {
        return Err(GoalBoardError(
            "Goal objective blockquote does not match the current objective revision".into(),
        ));
    }
    Ok(())
}

fn structural_line_mask(lines: &[&str]) -> Result<Vec<bool>, GoalBoardError> {
    let mut structural = Vec::with_capacity(lines.len());
    let mut fence: Option<String> = None;
    for line in lines {
        let trimmed = line.trim_start();
        let marker = if trimmed.starts_with("```") {
            Some("```")
        } else if trimmed.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        structural.push(fence.is_none());
        if let Some(marker) = marker {
            if fence.as_deref() == Some(marker) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(marker.to_string());
            }
        }
    }
    if fence.is_some() {
        return Err(GoalBoardError(
            "Goal blackboard contains an unclosed code fence".into(),
        ));
    }
    Ok(structural)
}

fn looks_like_checkbox(line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]")
}

fn parse_task_line(line: &str, line_number: usize) -> Result<ParsedTask, GoalBoardError> {
    if line.starts_with('\t') || line.contains("\t- [") {
        return Err(GoalBoardError(format!(
            "Goal task indentation must use spaces on line {}",
            line_number + 1
        )));
    }
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let (checked, rest) = if let Some(rest) = rest.strip_prefix("- [ ] ") {
        (false, rest)
    } else if let Some(rest) = rest
        .strip_prefix("- [x] ")
        .or_else(|| rest.strip_prefix("- [X] "))
    {
        (true, rest)
    } else {
        return Err(GoalBoardError(format!(
            "malformed Goal checkbox on line {}",
            line_number + 1
        )));
    };
    let Some(rest) = rest.strip_prefix("**") else {
        return Err(task_syntax_error(line_number));
    };
    let Some((id, rest)) = rest.split_once("** `") else {
        return Err(task_syntax_error(line_number));
    };
    let Some((status, summary)) = rest.split_once("` — ") else {
        return Err(task_syntax_error(line_number));
    };
    let status = match status {
        "pending" => GoalTaskStatus::Pending,
        "in_progress" => GoalTaskStatus::InProgress,
        "blocked" => GoalTaskStatus::Blocked,
        "done" => GoalTaskStatus::Done,
        _ => {
            return Err(GoalBoardError(format!(
                "invalid Goal task status `{status}` on line {}",
                line_number + 1
            )));
        }
    };
    if checked != (status == GoalTaskStatus::Done) {
        return Err(GoalBoardError(format!(
            "Goal checkbox and status disagree on line {}",
            line_number + 1
        )));
    }
    let summary = summary.trim();
    if summary.is_empty() || UnicodeWidthStr::width(summary) > MAX_SUMMARY_COLUMNS {
        return Err(GoalBoardError(format!(
            "Goal task summary on line {} must be 1..={MAX_SUMMARY_COLUMNS} display columns",
            line_number + 1
        )));
    }
    let segments = parse_task_id(id).ok_or_else(|| {
        GoalBoardError(format!(
            "invalid Goal task id `{id}` on line {}",
            line_number + 1
        ))
    })?;
    let depth = segments.len();
    if depth > MAX_DEPTH {
        return Err(GoalBoardError(format!(
            "Goal task `{id}` exceeds maximum depth {MAX_DEPTH}"
        )));
    }
    if indent != (depth - 1) * 2 {
        return Err(GoalBoardError(format!(
            "Goal task `{id}` indentation does not match its id depth"
        )));
    }
    let parent_id = (depth > 1).then(|| {
        let split = id.rfind('.').expect("nested id has a dot");
        id[..split].to_string()
    });
    Ok(ParsedTask {
        id: id.to_string(),
        parent_id,
        depth,
        indent,
        status,
        summary: summary.to_string(),
        line: line_number,
    })
}

fn parse_task_id(id: &str) -> Option<Vec<u32>> {
    let rest = id.strip_prefix('T')?;
    let segments: Vec<u32> = rest
        .split('.')
        .map(|segment| {
            if segment.is_empty() || segment.starts_with('0') {
                return None;
            }
            segment.parse::<u32>().ok().filter(|value| *value > 0)
        })
        .collect::<Option<_>>()?;
    (!segments.is_empty()).then_some(segments)
}

fn task_syntax_error(line: usize) -> GoalBoardError {
    GoalBoardError(format!(
        "Goal task line {} must use `- [ ] **T1** `status` — summary`",
        line + 1
    ))
}

fn validate_patch_text(field: &str, value: &str) -> Result<(), GoalBoardError> {
    if value.trim().is_empty() || value.contains(['\n', '\r']) || value.len() > 4096 {
        return Err(GoalBoardError(format!(
            "Goal {field} must be a non-empty single line of at most 4096 bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> String {
        "# Goal\n\n> ship safely\n\n## Plan\n\n- [ ] **T1** `in_progress` — Implement the change\n  - Scope: runtime\n  - Acceptance: tests pass\n  - [x] **T1.1** `done` — Inspect callers\n    - Evidence: call graph checked\n- [ ] **T2** `pending` — Verify behavior\n  - Scope: tests\n  - Acceptance: no regressions\n\n## Goal acceptance\n\n- Tests pass\n\n## Verification evidence\n\n- Pending\n\n## Open gaps\n\n- None"
            .to_string()
    }

    #[test]
    fn canonical_board_projects_stable_task_tree() {
        let parsed = parse_goal_board("ship safely", board()).unwrap();
        let tasks = parsed.task_projection();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].id, "T1");
        assert_eq!(tasks[0].completed_descendants, 1);
        assert_eq!(tasks[0].total_descendants, 1);
        assert_eq!(tasks[1].parent_id.as_deref(), Some("T1"));
    }

    #[test]
    fn typed_patch_preserves_structure_and_advances_only_owned_fields() {
        let patched = apply_progress_updates(
            "ship safely",
            &board(),
            &[GoalProgressUpdate {
                task_id: "T2".into(),
                status: Some(GoalTaskStatus::Done),
                progress: None,
                evidence: Some("cargo test passed".into()),
                gap: None,
            }],
        )
        .unwrap();
        assert!(
            patched
                .markdown()
                .contains("- [x] **T2** `done` — Verify behavior")
        );
        assert!(
            patched
                .markdown()
                .contains("  - Evidence: cargo test passed")
        );
    }

    #[test]
    fn objective_and_parent_completion_are_enforced() {
        assert!(parse_goal_board("different", board()).is_err());
        let invalid = board().replace("- [ ] **T1** `in_progress`", "- [x] **T1** `done`");
        assert!(parse_goal_board("ship safely", &invalid).is_ok());
        let invalid = invalid.replace("- [x] **T1.1** `done`", "- [ ] **T1.1** `pending`");
        assert!(parse_goal_board("ship safely", invalid).is_err());
    }

    #[test]
    fn outer_transport_fence_is_unwrapped_but_inner_fence_is_retained() {
        let with_inner = board().replace("- Pending", "```text\n- [ ] not a task\n```\n- Pending");
        let wrapped = format!("```markdown\n{with_inner}\n```");
        let parsed = parse_goal_board("ship safely", wrapped).unwrap();
        assert!(parsed.markdown().contains("```text"));
        assert_eq!(parsed.task_projection().len(), 3);
    }

    #[test]
    fn one_patch_can_complete_child_and_parent_without_order_dependence() {
        let patched = apply_progress_updates(
            "ship safely",
            &board().replace("- [x] **T1.1** `done`", "- [ ] **T1.1** `in_progress`"),
            &[
                GoalProgressUpdate {
                    task_id: "T1".into(),
                    status: Some(GoalTaskStatus::Done),
                    progress: None,
                    evidence: Some("phase accepted".into()),
                    gap: None,
                },
                GoalProgressUpdate {
                    task_id: "T1.1".into(),
                    status: Some(GoalTaskStatus::Done),
                    progress: None,
                    evidence: Some("call graph checked".into()),
                    gap: None,
                },
            ],
        )
        .unwrap();
        let tasks = patched.task_projection();
        assert_eq!(tasks[0].status, GoalTaskStatus::Done);
        assert_eq!(tasks[1].status, GoalTaskStatus::Done);
    }

    #[test]
    fn rejects_duplicate_or_orphaned_task_ids_and_outside_checkboxes() {
        let duplicate = board().replace(
            "- [ ] **T2** `pending` — Verify behavior",
            "- [ ] **T1** `pending` — Duplicate",
        );
        assert!(parse_goal_board("ship safely", duplicate).is_err());

        let orphan = board().replace("**T1.1**", "**T3.1**");
        assert!(parse_goal_board("ship safely", orphan).is_err());

        let outside = board().replace(
            "## Open gaps\n\n- None",
            "## Open gaps\n\n- [ ] not task state",
        );
        assert!(parse_goal_board("ship safely", outside).is_err());
    }

    #[test]
    fn rejects_checkbox_status_disagreement_and_overwide_cjk_summary() {
        let mismatch = board().replace("- [ ] **T2** `pending`", "- [x] **T2** `pending`");
        assert!(parse_goal_board("ship safely", mismatch).is_err());

        let wide = "界".repeat(MAX_SUMMARY_COLUMNS / 2 + 1);
        let overwide = board().replace("Verify behavior", &wide);
        assert!(parse_goal_board("ship safely", overwide).is_err());
    }

    #[test]
    fn progress_patch_is_cas_friendly_and_never_partially_applies() {
        let original = board();
        let result = apply_progress_updates(
            "ship safely",
            &original,
            &[
                GoalProgressUpdate {
                    task_id: "T2".into(),
                    status: Some(GoalTaskStatus::Done),
                    progress: None,
                    evidence: Some("valid".into()),
                    gap: None,
                },
                GoalProgressUpdate {
                    task_id: "T404".into(),
                    status: Some(GoalTaskStatus::Done),
                    progress: None,
                    evidence: None,
                    gap: None,
                },
            ],
        );
        assert!(result.is_err());
        assert!(original.contains("- [ ] **T2** `pending`"));
    }

    #[test]
    fn board_size_task_count_and_depth_limits_are_enforced() {
        let oversized = format!("{}\n{}", board(), "x".repeat(MAX_BOARD_BYTES));
        assert!(parse_goal_board("ship safely", oversized).is_err());

        let tasks = (1..=MAX_TASKS + 1)
            .map(|index| format!("- [ ] **T{index}** `pending` — Task {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let too_many = format!(
            "# Goal\n\n> ship safely\n\n## Plan\n\n{tasks}\n\n## Goal acceptance\n\n- done\n\n## Verification evidence\n\n- pending\n\n## Open gaps\n\n- none"
        );
        assert!(parse_goal_board("ship safely", too_many).is_err());

        let too_deep = board().replace(
            "    - Evidence: call graph checked",
            "    - Evidence: call graph checked\n    - [ ] **T1.1.1** `pending` — Level three\n      - [ ] **T1.1.1.1** `pending` — Level four\n        - [ ] **T1.1.1.1.1** `pending` — Level five",
        );
        assert!(parse_goal_board("ship safely", too_deep).is_err());
    }
}

#[cfg(test)]
mod assemble_tests {
    use super::*;

    fn plan_task(summary: &str) -> GoalPlanTaskSpec {
        GoalPlanTaskSpec {
            summary: summary.into(),
            status: None,
            scope: None,
            acceptance: None,
            evidence: None,
            gap: None,
            children: Vec::new(),
        }
    }

    fn plan_spec(tasks: Vec<GoalPlanTaskSpec>, acceptance: &[&str]) -> GoalPlanSpec {
        GoalPlanSpec {
            sections: vec![
                GoalPlanSectionPayload::PlanTasks { tasks },
                GoalPlanSectionPayload::GoalAcceptance {
                    items: acceptance.iter().map(|item| item.to_string()).collect(),
                },
            ],
        }
    }

    fn error_paths(error: &GoalPlanAssemblyError) -> Vec<String> {
        error.items.iter().map(|item| item.path.clone()).collect()
    }

    #[test]
    fn assembled_multi_level_board_round_trips_and_projects_host_assigned_ids() {
        let mut grandchild = plan_task("验证三层层级 depth-three nesting");
        grandchild.status = Some(GoalTaskStatus::Done);
        grandchild.evidence = Some("cargo test 绿".into());
        let mut child = plan_task("Wire the host assembler");
        child.status = Some(GoalTaskStatus::Done);
        child.scope = Some("goal_board".into());
        child.acceptance = Some("board round-trips".into());
        child.children = vec![grandchild];
        let mut root = plan_task("把黑板拼装移入宿主 move assembly host-side");
        root.status = Some(GoalTaskStatus::InProgress);
        root.scope = Some("planner contract".into());
        root.acceptance = Some("canonical grammar".into());
        root.evidence = Some("parser tests".into());
        root.gap = Some("tool registration pending".into());
        root.children = vec![child];
        let mut spec = plan_spec(vec![root, plan_task("Second root")], &["tests pass"]);
        spec.sections.push(GoalPlanSectionPayload::OpenGaps {
            items: vec!["runtime wiring".into()],
        });

        let objective = "ship the goal runtime safely";
        let assembled = assemble_goal_board(objective, &spec).unwrap();
        let parsed = parse_goal_board(objective, assembled.clone()).unwrap();
        let tasks = parsed.task_projection();

        assert_eq!(tasks.len(), 4);
        assert_eq!(
            (
                tasks[0].id.as_str(),
                tasks[0].depth,
                tasks[0].parent_id.as_deref()
            ),
            ("T1", 1, None)
        );
        assert_eq!(
            (
                tasks[1].id.as_str(),
                tasks[1].depth,
                tasks[1].parent_id.as_deref()
            ),
            ("T1.1", 2, Some("T1"))
        );
        assert_eq!(
            (
                tasks[2].id.as_str(),
                tasks[2].depth,
                tasks[2].parent_id.as_deref()
            ),
            ("T1.1.1", 3, Some("T1.1"))
        );
        assert_eq!(
            (
                tasks[3].id.as_str(),
                tasks[3].depth,
                tasks[3].parent_id.as_deref()
            ),
            ("T2", 1, None)
        );
        assert_eq!(tasks[0].status, GoalTaskStatus::InProgress);
        assert_eq!(tasks[1].status, GoalTaskStatus::Done);
        assert_eq!(tasks[3].status, GoalTaskStatus::Pending);
        assert_eq!(
            (tasks[0].completed_descendants, tasks[0].total_descendants),
            (2, 2)
        );

        let lines: Vec<&str> = assembled.lines().collect();
        assert_eq!(lines[0], "# Goal");
        assert_eq!(lines[2], "> ship the goal runtime safely");
        assert!(
            assembled.contains(
                "- [ ] **T1** `in_progress` — 把黑板拼装移入宿主 move assembly host-side"
            )
        );
        assert!(assembled.contains("  - Scope: planner contract"));
        assert!(assembled.contains("  - Acceptance: canonical grammar"));
        assert!(assembled.contains("  - Evidence: parser tests"));
        assert!(assembled.contains("  - Gap: tool registration pending"));
        assert!(assembled.contains("  - [x] **T1.1** `done` — Wire the host assembler"));
        assert!(
            assembled.contains("    - [x] **T1.1.1** `done` — 验证三层层级 depth-three nesting")
        );
        assert!(assembled.contains("      - Evidence: cargo test 绿"));
        assert!(assembled.contains("- [ ] **T2** `pending` — Second root"));
        assert!(assembled.contains("## Goal acceptance\n\n- tests pass"));
        assert!(assembled.contains("## Verification evidence\n\n- Pending"));
        assert!(assembled.contains("## Open gaps\n\n- runtime wiring"));
    }

    #[test]
    fn missing_open_gaps_section_renders_none() {
        let spec = plan_spec(vec![plan_task("Only task")], &["accepted"]);
        let assembled = assemble_goal_board("objective", &spec).unwrap();
        assert!(assembled.contains("## Open gaps\n\n- None"));
        assert!(parse_goal_board("objective", assembled).is_ok());
    }

    #[test]
    fn depth_five_is_rejected_with_entry_path() {
        let mut root = plan_task("L1");
        let mut cursor = &mut root;
        for level in 2..=5 {
            cursor.children.push(plan_task(&format!("L{level}")));
            cursor = cursor.children.last_mut().unwrap();
        }
        let error =
            assemble_goal_board("objective", &plan_spec(vec![root], &["accepted"])).unwrap_err();
        assert_eq!(
            error_paths(&error),
            vec!["tasks[0].children[0].children[0].children[0].children[0]"]
        );
    }

    #[test]
    fn task_129_is_rejected_with_entry_path() {
        let tasks = (1..=MAX_TASKS + 1)
            .map(|index| plan_task(&format!("Task {index}")))
            .collect();
        let error = assemble_goal_board("objective", &plan_spec(tasks, &["accepted"])).unwrap_err();
        assert_eq!(error_paths(&error), vec![format!("tasks[{MAX_TASKS}]")]);
        assert!(
            error.items[0]
                .reason
                .contains(&format!("maximum of {MAX_TASKS} tasks"))
        );
    }

    #[test]
    fn empty_or_whitespace_summary_is_rejected() {
        let spec = plan_spec(vec![plan_task("   "), plan_task("")], &["accepted"]);
        let error = assemble_goal_board("objective", &spec).unwrap_err();
        assert_eq!(
            error_paths(&error),
            vec!["tasks[0].summary", "tasks[1].summary"]
        );
    }

    #[test]
    fn overwide_cjk_summary_is_rejected_and_the_boundary_assembles() {
        let boundary = plan_spec(
            vec![plan_task(&"界".repeat(MAX_SUMMARY_COLUMNS / 2))],
            &["accepted"],
        );
        assert!(assemble_goal_board("objective", &boundary).is_ok());

        let overwide = plan_spec(
            vec![plan_task(&"界".repeat(MAX_SUMMARY_COLUMNS / 2 + 1))],
            &["accepted"],
        );
        let error = assemble_goal_board("objective", &overwide).unwrap_err();
        assert_eq!(error_paths(&error), vec!["tasks[0].summary"]);
        assert!(error.items[0].reason.contains("display columns"));
    }

    #[test]
    fn multiline_and_oversized_metadata_are_rejected() {
        let mut task = plan_task("Valid summary");
        task.scope = Some("line one\nline two".into());
        task.gap = Some("x".repeat(MAX_METADATA_BYTES + 1));
        let error =
            assemble_goal_board("objective", &plan_spec(vec![task], &["accepted"])).unwrap_err();
        assert_eq!(error_paths(&error), vec!["tasks[0].scope", "tasks[0].gap"]);
        assert!(error.items[0].reason.contains("single line"));
        assert!(
            error.items[1]
                .reason
                .contains(&format!("at most {MAX_METADATA_BYTES} bytes"))
        );
    }

    #[test]
    fn empty_goal_acceptance_and_multiline_items_are_rejected() {
        let empty =
            assemble_goal_board("objective", &plan_spec(vec![plan_task("Task")], &[])).unwrap_err();
        assert_eq!(error_paths(&empty), vec!["goal_acceptance.items"]);

        let multiline = plan_spec(vec![plan_task("Task")], &["fine", "bad\nline"]);
        let error = assemble_goal_board("objective", &multiline).unwrap_err();
        assert_eq!(error_paths(&error), vec!["goal_acceptance.items[1]"]);
    }

    #[test]
    fn multiple_errors_are_aggregated_in_document_order() {
        let mut overwide = plan_task(&"界".repeat(81));
        overwide.evidence = Some("shared offender".into());
        let mut empty = plan_task("");
        empty.scope = Some("line one\nline two".into());
        empty.children = vec![plan_task("")];
        let spec = plan_spec(vec![overwide, empty], &["bad\nline", "good"]);
        let error = assemble_goal_board("objective", &spec).unwrap_err();
        assert_eq!(
            error_paths(&error),
            vec![
                "tasks[0].summary",
                "tasks[1].summary",
                "tasks[1].scope",
                "tasks[1].children[0].summary",
                "goal_acceptance.items[0]",
            ]
        );
    }

    #[test]
    fn objective_with_blank_lines_round_trips_as_blockquotes() {
        let objective = "first line\n\nsecond line\nthird";
        let spec = plan_spec(vec![plan_task("Task")], &["accepted"]);
        let assembled = assemble_goal_board(objective, &spec).unwrap();
        let lines: Vec<&str> = assembled.lines().collect();
        assert_eq!(
            lines[2..6],
            ["> first line", "> ", "> second line", "> third"]
        );
        assert!(parse_goal_board(objective, assembled).is_ok());
    }

    #[test]
    fn checkbox_like_list_items_are_escaped_and_still_parse() {
        let mut spec = plan_spec(vec![plan_task("Task")], &["[ ] not a task"]);
        spec.sections.push(GoalPlanSectionPayload::OpenGaps {
            items: vec!["[X] done marker".into()],
        });
        let assembled = assemble_goal_board("objective", &spec).unwrap();
        assert!(assembled.contains("- \\[ \\] not a task"));
        assert!(assembled.contains("- \\[X\\] done marker"));
        assert!(parse_goal_board("objective", assembled).is_ok());
    }

    #[test]
    fn missing_and_duplicate_sections_are_rejected() {
        let empty = GoalPlanSpec {
            sections: Vec::new(),
        };
        let error = assemble_goal_board("objective", &empty).unwrap_err();
        assert_eq!(error_paths(&error), vec!["sections", "sections"]);
        assert!(
            error.items[0]
                .reason
                .contains("`plan_tasks` section is missing")
        );
        assert!(
            error.items[1]
                .reason
                .contains("`goal_acceptance` section is missing")
        );

        let duplicate = GoalPlanSpec {
            sections: vec![
                GoalPlanSectionPayload::PlanTasks {
                    tasks: vec![plan_task("A")],
                },
                GoalPlanSectionPayload::PlanTasks {
                    tasks: vec![plan_task("B")],
                },
                GoalPlanSectionPayload::GoalAcceptance {
                    items: vec!["accepted".into()],
                },
            ],
        };
        let error = assemble_goal_board("objective", &duplicate).unwrap_err();
        assert_eq!(error_paths(&error), vec!["sections[1]"]);
        assert!(
            error.items[0]
                .reason
                .contains("duplicate `plan_tasks` section")
        );
    }
}
