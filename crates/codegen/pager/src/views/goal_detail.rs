//! Goal detail overlay with two projections of the durable Markdown board.
//!
//! The default summary shows progress and task-list items only. The full-board
//! view is a scrollable Markdown document. Both are derived from the same
//! `GoalDisplayState::plan_markdown`; this module never creates a second task
//! state or writes back into the Goal runtime.
//!
//! Rendered as a centered overlay when `AgentView::show_goal_detail` is true
//! and `goal_state` is `Some`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::agent::{GoalDisplayState, GoalDisplayStatus};
use crate::render::SafeBuf;
use crate::scrollback::blocks::markdown_content::MarkdownContent;
use crate::scrollback::types::BlockLine;
use crate::theme::{Theme, ThemeKind, cache as theme_cache};
use crate::views::agent_status::{active_phase_label, format_tokens_compact};
use crate::views::progress_bar::progress_bar_spans;

/// Maximum rendered task rows displayed in the compact summary.
const MAX_TASK_DISPLAY_ROWS: usize = 10;

/// Non-persistent Markdown projection for the Goal overlay. Goal updates carry
/// the source document; parsing, syntax highlighting, and width-dependent
/// wrapping belong to the view and are cached independently of animation
/// frames and Goal persistence.
pub(crate) struct GoalBoardRenderer {
    source: String,
    full_content: Option<MarkdownContent>,
    task_content: Option<MarkdownContent>,
    width: u16,
    theme: Option<ThemeKind>,
    full_lines: Vec<BlockLine>,
    task_lines: Vec<BlockLine>,
    task_count: usize,
    completed_task_count: usize,
    full_board: bool,
    scroll: u16,
}

impl Default for GoalBoardRenderer {
    fn default() -> Self {
        Self {
            source: String::new(),
            full_content: None,
            task_content: None,
            width: 0,
            theme: None,
            full_lines: Vec::new(),
            task_lines: Vec::new(),
            task_count: 0,
            completed_task_count: 0,
            full_board: false,
            scroll: 0,
        }
    }
}

impl GoalBoardRenderer {
    fn refresh(&mut self, markdown: &str, width: u16) {
        if self.source != markdown {
            self.source.clear();
            self.source.push_str(markdown);
            self.full_content =
                (!markdown.trim().is_empty()).then(|| MarkdownContent::new(markdown));

            let tasks = markdown.lines().filter_map(parse_markdown_task);
            let mut task_markdown = String::new();
            self.task_count = 0;
            self.completed_task_count = 0;
            for task in tasks {
                if !task_markdown.is_empty() {
                    task_markdown.push('\n');
                }
                task_markdown.push_str(if task.complete { "- [x] " } else { "- [ ] " });
                task_markdown.push_str(&strip_control_chars(task.label, false));
                self.task_count += 1;
                self.completed_task_count += usize::from(task.complete);
            }
            self.task_content =
                (!task_markdown.is_empty()).then(|| MarkdownContent::new(task_markdown.as_str()));
            self.width = 0;
            self.theme = None;
            self.full_lines.clear();
            self.task_lines.clear();
            // A plan revision can replace the document completely. Keep the
            // selected projection, but never leave the reader halfway through
            // unrelated content.
            self.scroll = 0;
        }

        let theme = theme_cache::current_kind();
        if self.width != width || self.theme != Some(theme) {
            self.full_lines = self
                .full_content
                .as_ref()
                .map(|content| content.output(width as usize).lines)
                .unwrap_or_default();
            self.task_lines = self
                .task_content
                .as_ref()
                .map(|content| content.output(width as usize).lines)
                .unwrap_or_default();
            self.width = width;
            self.theme = Some(theme);
        }
    }

    pub(crate) fn reset_navigation(&mut self) {
        self.full_board = false;
        self.scroll = 0;
    }

    pub(crate) fn is_full_board(&self) -> bool {
        self.full_board
    }

    pub(crate) fn show_full_board(&mut self) {
        self.full_board = true;
        self.scroll = 0;
    }

    pub(crate) fn show_task_summary(&mut self) {
        self.full_board = false;
        self.scroll = 0;
    }

    pub(crate) fn toggle_projection(&mut self) {
        if self.full_board {
            self.show_task_summary();
        } else {
            self.show_full_board();
        }
    }

    pub(crate) fn apply_scroll_key(&mut self, code: crossterm::event::KeyCode) -> bool {
        self.full_board && crate::views::modal::apply_doc_scroll(code, &mut self.scroll)
    }

    pub(crate) fn apply_mouse_scroll(&mut self, kind: crossterm::event::MouseEventKind) -> bool {
        self.full_board && crate::views::modal::apply_doc_mouse_scroll(kind, &mut self.scroll)
    }

    pub(crate) fn apply_scroll_delta(&mut self, lines: i32) -> bool {
        if !self.full_board || lines == 0 {
            return false;
        }
        crate::views::modal::apply_doc_scroll_delta(&mut self.scroll, lines);
        true
    }
}

struct MarkdownTask<'a> {
    complete: bool,
    label: &'a str,
}

/// Extract a GitHub-flavoured task-list item without interpreting arbitrary
/// prose. Both unordered and ordered Markdown list markers are accepted, as
/// are nested blockquotes. The source remains the only canonical task state;
/// this parser is a lossy, read-only projection for the compact UI.
fn parse_markdown_task(line: &str) -> Option<MarkdownTask<'_>> {
    let mut rest = line.trim_start();
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.strip_prefix(' ').unwrap_or(after).trim_start();
    }

    rest = if let Some(after) = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
    {
        after
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let after_digits = &rest[digits..];
        after_digits
            .strip_prefix(". ")
            .or_else(|| after_digits.strip_prefix(") "))?
    };
    rest = rest.trim_start();

    let (complete, after_box) = if let Some(after) = rest.strip_prefix("[ ]") {
        (false, after)
    } else if let Some(after) = rest
        .strip_prefix("[x]")
        .or_else(|| rest.strip_prefix("[X]"))
    {
        (true, after)
    } else {
        return None;
    };
    if !after_box.is_empty() && !after_box.starts_with(char::is_whitespace) {
        return None;
    }
    Some(MarkdownTask {
        complete,
        label: after_box.trim(),
    })
}

/// Maximum per-model token rows displayed before a "+N more" summary row.
const MAX_MODEL_DISPLAY: usize = 6;

/// Rows the per-model breakdown contributes to the modal: the capped model
/// rows plus an optional "+N more" overflow row, or 0 when the breakdown is
/// suppressed (a single-model / all-inherit goal collapses to the single
/// tokens line). The cap is applied BEFORE the `u16` cast so the height sum
/// can never overflow, and this is the single source of truth shared by the
/// height calc and the render loop so they stay in lockstep.
fn per_model_row_count(models: &[(String, u64)]) -> u16 {
    if models.len() < 2 {
        return 0;
    }
    let shown = models.len().min(MAX_MODEL_DISPLAY);
    let overflow = usize::from(models.len() > MAX_MODEL_DISPLAY);
    (shown + overflow) as u16
}

// ---------------------------------------------------------------------------
// Token budget color
// ---------------------------------------------------------------------------

/// Choose the progress bar fill color based on usage percentage.
fn budget_color(pct: f32, theme: &Theme) -> Color {
    if pct > 0.80 {
        theme.accent_error
    } else if pct >= 0.50 {
        theme.warning
    } else {
        theme.accent_success
    }
}

/// Format elapsed milliseconds as a compact human-readable duration.
/// Same style as `goal_orchestrator::format_elapsed` — keep in sync.
pub(crate) fn format_elapsed(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}h{mins:02}m")
    } else if mins > 0 {
        format!("{mins}m{secs:02}s")
    } else {
        format!("{secs}s")
    }
}

// ---------------------------------------------------------------------------
// Status label
// ---------------------------------------------------------------------------

fn status_label(goal: &GoalDisplayState) -> (&'static str, Color, String) {
    let theme = Theme::current();
    match goal.status {
        GoalDisplayStatus::Active => ("Active", theme.accent_success, active_phase_label(goal)),
        GoalDisplayStatus::Paused | GoalDisplayStatus::Blocked => {
            (goal.status.pause_label(), theme.warning, String::new())
        }
        GoalDisplayStatus::BudgetLimited => ("Budget Limited", theme.accent_error, String::new()),
        GoalDisplayStatus::Complete => ("Complete", theme.accent_success, String::new()),
    }
}

// ---------------------------------------------------------------------------
// Wrapping helpers — pause-message reason block
// ---------------------------------------------------------------------------

/// Wrap a string into rows of at most `width` terminal columns.
///
/// Splits on whitespace first, then hard-splits any token wider than
/// `width`. Preserves explicit `\n` line breaks so multi-line block
/// reasons (the concatenated `blocked_reason\nmessage` form emitted by
/// the shell) render with the same structure they had on the wire.
///
/// Width is measured in terminal columns via `UnicodeWidthStr` /
/// `UnicodeWidthChar`, not Unicode code points — CJK / East-Asian Wide
/// characters take 2 columns each, combining marks take 0, and emoji
/// can take 2. Using `chars().count()` here would let model-emitted
/// block reasons containing wide chars overflow the modal's inner
/// rectangle into the right border.
///
/// `width` of zero or one returns a single un-split row to avoid
/// divide-by-zero behaviour at degenerate modal widths (unreachable in
/// practice — the modal bails below width 20).
fn wrap_pause_message_lines(text: &str, width: u16) -> Vec<String> {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    let w = width as usize;
    if w <= 1 {
        // Collapse the kept `\n` here too so "no control byte in any returned
        // line" holds even on this un-split degenerate path.
        return vec![text.replace('\n', " ")];
    }
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if UnicodeWidthStr::width(word) > w {
                // Hard-split overlong tokens so the row-width invariant
                // holds even for paths/URLs without whitespace. Build
                // each chunk by accumulating chars until the next char
                // would push the chunk past `w` columns, honouring
                // zero-width marks (they don't consume capacity).
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let mut chunk = String::new();
                let mut chunk_w = 0usize;
                for ch in word.chars() {
                    let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                    if cw > w {
                        // Pathological: a single char wider than the
                        // whole row. Emit it alone — anything else
                        // would silently drop the codepoint.
                        if !chunk.is_empty() {
                            out.push(std::mem::take(&mut chunk));
                            chunk_w = 0;
                        }
                        out.push(ch.to_string());
                        continue;
                    }
                    if chunk_w + cw > w {
                        out.push(std::mem::take(&mut chunk));
                        chunk_w = 0;
                    }
                    chunk.push(ch);
                    chunk_w += cw;
                }
                if !chunk.is_empty() {
                    out.push(chunk);
                }
                continue;
            }
            let need = if current.is_empty() {
                UnicodeWidthStr::width(word)
            } else {
                UnicodeWidthStr::width(current.as_str()) + 1 + UnicodeWidthStr::width(word)
            };
            if need > w {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Truncate `text` to at most `budget` terminal columns, appending an
/// ellipsis if truncated. Uses display width (not char count) so CJK
/// and emoji characters measure correctly — matches the
/// `wrap_pause_message_lines` pattern.
pub(crate) fn truncate_to_width(text: &str, budget: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    if UnicodeWidthStr::width(text) <= budget {
        return text.to_owned();
    }
    let target = budget.saturating_sub(1); // room for ellipsis
    let mut out = String::new();
    let mut w = 0usize;
    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > target {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('\u{2026}');
    out
}

/// Replace control characters (tab, ESC, BEL, …) with spaces so free-form
/// model/wire-derived text (objective title, humanized event detail/name/
/// timestamp, pause reason) can't break a rendered row even if ratatui's own
/// filter regresses. `keep_newlines` preserves `\n` for the pause-reason
/// wrapper (which splits on it before render); single-row sinks pass `false`.
pub(crate) fn strip_control_chars(s: &str, keep_newlines: bool) -> String {
    s.chars()
        .map(|c| {
            if c.is_control() && !(keep_newlines && c == '\n') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// Strip control chars from the objective title, then trim (the title is
/// short and centered). A bare `\n` is zero-width to `truncate_to_width` and
/// would otherwise leak into the border row.
fn sanitize_title(s: &str) -> String {
    strip_control_chars(s, false).trim().to_owned()
}

/// Build the wrapped-reason source line for a paused goal's `pause_message`,
/// control-stripped (newlines kept — [`wrap_pause_message_lines`] splits on
/// them for multi-line block reasons and they never reach a rendered row).
/// Shared by the height calc and the render so they wrap identical text.
fn format_pause_reason(msg: &str) -> String {
    format!("Reason: {}", strip_control_chars(msg, true))
}

// ---------------------------------------------------------------------------
// Public render
// ---------------------------------------------------------------------------

/// Humanize a wire goal-event name (+ optional detail) for the Recent
/// History row — the single wire→display mapping, so machine vocabulary
/// (`goal_paused`, snake_case detail) never reaches the user. Detail is
/// folded into the label for the events that carry one (pause cause,
/// premature-stop pattern); unknown events fall back to a de-snake-cased
/// form so a future shell event still renders readably.
fn humanize_goal_event(event: &str, detail: Option<&str>) -> String {
    // Variable passthroughs (model/wire-derived) are control-stripped so they
    // can't leak control bytes; the fixed labels below are `&'static`.
    let phrase = |d: Option<&str>| d.map(|s| strip_control_chars(&s.replace('_', " "), false));
    match event {
        "goal_created" => "Goal created".into(),
        "planning_started" => "Planning started".into(),
        "planning_completed" => "Planning completed".into(),
        "planning_failed" => "Planning failed".into(),
        "worker_started" => "Worker started".into(),
        "worker_completed" => "Worker completed".into(),
        "worker_failed" => "Worker failed".into(),
        "context_rotated" => "Context rotated".into(),
        // A plain user pause has no extra cause worth showing.
        "goal_paused" => match phrase(detail).filter(|d| d != "user") {
            Some(d) => format!("Paused: {d}"),
            None => "Paused".into(),
        },
        "goal_resumed" => "Resumed".into(),
        "goal_completed" => "Completed".into(),
        "goal_cleared" => "Cleared".into(),
        "budget_exceeded" => "Budget exceeded".into(),
        "premature_stop_detected" => match phrase(detail) {
            Some(d) => format!("Stopped early: {d}"),
            None => "Stopped early".into(),
        },
        other => {
            let mut s = strip_control_chars(&other.replace('_', " "), false);
            if let Some(c) = s.get_mut(0..1) {
                c.make_ascii_uppercase();
            }
            s
        }
    }
}

/// Render a wire RFC3339 event timestamp as a coarse relative time
/// ("2m ago"). Empty stays empty; an unparseable value (legacy / non-RFC3339)
/// is returned verbatim (control-stripped, since it's a raw passthrough).
fn humanize_event_timestamp(ts: &str, wall_now: std::time::SystemTime) -> String {
    if ts.is_empty() {
        return String::new();
    }
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return strip_control_chars(ts, false);
    };
    let secs = chrono::DateTime::<chrono::Utc>::from(wall_now)
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_seconds()
        .max(0) as u64;
    let ago = crate::util::format_time_ago(std::time::Duration::from_secs(secs));
    if ago == "just now" {
        ago
    } else {
        format!("{ago} ago")
    }
}

/// Compute the overlay area (centered, sized to content, clamped to screen).
fn goal_detail_width(screen: Rect) -> u16 {
    let width_pct = 0.90f32;
    let preferred_w = (screen.width as f32 * width_pct) as u16;
    preferred_w
        .clamp(60, 140)
        .min(screen.width.saturating_sub(4))
}

fn goal_detail_area(screen: Rect, goal: &GoalDisplayState, board: &mut GoalBoardRenderer) -> Rect {
    let w = goal_detail_width(screen);

    // Inner content width matches the render path:
    //   `inner` = block.inner(area) gives `w - 2` (the rounded border).
    //   We further indent by 1 column on each side (`x = inner.x + 1`,
    //   `w = inner.width - 2`), so the usable text width is `w - 4`.
    // Mirror that here so pause-message wrapping computes the same row
    // count the renderer will produce.
    let inner_w = w.saturating_sub(4);
    board.refresh(&goal.plan_markdown, inner_w.max(1));

    if board.full_board {
        let h = screen.height.saturating_sub(4).max(6);
        let x = screen.x + (screen.width.saturating_sub(w)) / 2;
        let y = screen.y + (screen.height.saturating_sub(h)) / 2;
        return Rect::new(x, y, w, h.min(screen.height));
    }

    // Compute content height based on what will actually be rendered. Each
    // optional section OWNS its leading blank separator (rendered only when
    // the section renders) so the height budget and the render path stay in
    // lockstep.
    //   2  border (top + bottom)
    //   1  status line
    //   N  pause_message reason block (wrapped, when paused + Some)
    //   1  pause hint line (only when any paused variant)
    //   1  budget/tokens line
    //   1  progress bar (only if budget set)
    //   1  blank separator (unconditional, before the progress section)
    //   1  task progress header
    //   1  task progress bar (when tasks exist)
    //   N  rendered task-list rows (+ optional "+N more")
    //   1  verifier feedback (when present)
    //   2-3 subagent block (if active): blank + role line + optional detail line
    //   N  per-model token rows (only with an active subagent + ≥2 models,
    //      capped at MAX_MODEL_DISPLAY + optional "+N more")
    //   3  recent history (if last_event present): blank + header + event line
    //   1  commands hint
    let has_budget = goal.token_budget.is_some_and(|b| b > 0);
    let budget_bar = if has_budget { 1u16 } else { 0 };
    let recovery_hint = if goal.status.is_paused() { 1u16 } else { 0 };
    // Reason block renders as `Reason: <pause_message>` wrapped to the
    // inner column width. Prefix is part of the wrapped content so
    // continuation rows just continue at column 0 without alignment
    // tricks; matches the renderer's loop exactly. Gated on
    // `is_paused()` to stay in sync with the renderer — a future shell
    // bug that leaks `pause_message` on a non-paused snapshot must not
    // grow the modal box without also rendering content into it.
    let reason_lines = if goal.status.is_paused() {
        goal.pause_message
            .as_deref()
            .map(|m| {
                let formatted = format_pause_reason(m);
                wrap_pause_message_lines(&formatted, inner_w).len() as u16
            })
            .unwrap_or(0)
    } else {
        0
    };
    let task_lines = if board.task_count == 0 {
        2u16 // header + planning/no-checklist message
    } else {
        2 + board.task_lines.len().min(MAX_TASK_DISPLAY_ROWS) as u16
            + u16::from(board.task_lines.len() > MAX_TASK_DISPLAY_ROWS)
    };
    let verifier_feedback_lines = u16::from(goal.verifier_feedback.is_some());
    let subagent_lines = if goal.current_subagent_role.is_some() {
        // blank + role line, plus the detail line ONLY when there's a live
        // metric to show — matches the render, which skips the detail row when
        // every live_* field is None (a just-spawned subagent).
        let has_detail = goal.live_subagent_tokens.is_some()
            || goal.live_context_pct.is_some()
            || goal.live_turn_count.is_some()
            || goal.live_tool_call_count.is_some();
        2 + u16::from(has_detail)
    } else {
        0
    };
    // Gated on an active subagent so the breakdown can't render orphaned;
    // `per_model_row_count` owns the ≥2 collapse + cap (and keeps this
    // height term in lockstep with the render loop below).
    let per_model_lines = if goal.current_subagent_role.is_some() {
        per_model_row_count(&goal.live_tokens_by_model)
    } else {
        0
    };
    let history_lines = if goal.last_event.is_some() {
        3u16 // blank + header + event line
    } else {
        0
    };
    let content_h = 2
        + 1
        + reason_lines
        + recovery_hint
        + 1
        + budget_bar
        + 1
        + task_lines
        + verifier_feedback_lines
        + subagent_lines
        + per_model_lines
        + history_lines
        + 1;
    let v_margin = 2u16;
    let h = content_h.min(screen.height.saturating_sub(v_margin * 2));

    let x = screen.x + (screen.width.saturating_sub(w)) / 2;
    let y = screen.y + (screen.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GoalDetailRenderOutput {
    pub(crate) area: Rect,
    pub(crate) close: Option<Rect>,
    pub(crate) projection_toggle: Option<Rect>,
}

fn render_board_line(buf: &mut Buffer, line: &BlockLine, x: u16, y: u16, width: u16) {
    if let Some(background) = line.background {
        let start = x.saturating_add(line.bg_start_col.min(width));
        for cell_x in start..x.saturating_add(width) {
            if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(cell_x, y)) {
                cell.set_bg(background);
            }
        }
    }
    buf.set_line_safe(x, y, &line.content, width);
}

fn render_projection_footer(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    full_board: bool,
    hovered: bool,
    theme: &Theme,
) -> Option<Rect> {
    let button = if full_board {
        "[Task summary]"
    } else {
        "[Full board]"
    };
    let button_width = unicode_width::UnicodeWidthStr::width(button) as u16;
    if button_width > width {
        return None;
    }
    let button_x = x + width - button_width;
    let hint = if full_board {
        "↑/↓ PgUp/PgDn scroll  Esc: summary  g/q: close"
    } else {
        "Enter/Space: full board  Esc/g/q: close  /goal edit"
    };
    let hint_width = button_x.saturating_sub(x + 1);
    buf.set_span_safe(
        x,
        y,
        &Span::styled(hint, Style::default().fg(theme.gray_dim)),
        hint_width,
    );
    let style = Style::default()
        .fg(if hovered {
            theme.text_primary
        } else {
            theme.accent_plan
        })
        .add_modifier(if hovered {
            Modifier::BOLD
        } else {
            Modifier::empty()
        });
    buf.set_span_safe(button_x, y, &Span::styled(button, style), button_width);
    Some(Rect::new(button_x, y, button_width, 1))
}

/// Render the goal detail overlay into the buffer.
///
/// Draws a bordered popup with:
/// - Title: objective
/// - Status + phase
/// - Token budget progress bar
/// - Todo progress list
/// - Active subagent metrics
/// - Recent event history
/// - Available commands hint
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_goal_detail(
    buf: &mut Buffer,
    screen: Rect,
    goal: &GoalDisplayState,
    board: &mut GoalBoardRenderer,
    frame_stamp: crate::motion::FrameStamp,
    context_used: Option<u64>,
    active_subagent_tokens: u64,
    close_hovered: bool,
    projection_toggle_hovered: bool,
) -> Option<GoalDetailRenderOutput> {
    let area = goal_detail_area(screen, goal, board);
    let theme = Theme::current();
    if area.width < 20 || area.height < 6 {
        return None;
    }

    // Clear the popup area.
    let clear_style = Style::default().bg(theme.bg_base);
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            if let Some(cell) = buf.cell_mut(ratatui::layout::Position::new(x, y)) {
                cell.reset();
                cell.set_style(clear_style);
            }
        }
    }

    // Render border.
    let border_style = Style::default().fg(theme.gray).bg(theme.bg_base);
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(theme.bg_base));
    let inner = block.inner(area);
    ratatui::widgets::Widget::render(block, area, buf);

    // Close button geometry is needed up-front so the title can be budgeted
    // to stop before it. Close button [✗] in top-right (ASCII `[x]` on legacy
    // ConHost).
    let close_text = format!("[{}]", crate::glyphs::ballot_x());
    // Display width (not byte length) so the hit-rect matches the glyph cells
    // and the title budget below is computed from the real column position.
    let close_w = unicode_width::UnicodeWidthStr::width(close_text.as_str()) as u16;
    let close_x = area.x + area.width.saturating_sub(close_w + 1);

    // Title in the top border: the live objective so the user can see WHICH
    // goal is running (with a spinner when active). The objective is
    // truncated by DISPLAY WIDTH (CJK / emoji safe) to the columns between
    // the left inset and the close button, so a long objective can never
    // collide with `[✗]` or overflow the right border.
    let is_active = matches!(goal.status, GoalDisplayStatus::Active);
    let spinner_prefix = if is_active {
        let frames = crate::glyphs::dot_spinner_frames();
        let frame = crate::motion::spinner_glyph(frame_stamp, frames);
        format!("{frame} ")
    } else {
        String::new()
    };
    let title_cols = close_x.saturating_sub(area.x + 3) as usize; // 1-col gap before [✗]
    let objective_budget = title_cols
        .saturating_sub(unicode_width::UnicodeWidthStr::width(
            spinner_prefix.as_str(),
        ))
        .saturating_sub(2); // leading + trailing space
    let cleaned = sanitize_title(&goal.objective);
    let objective = if cleaned.is_empty() {
        "Active Goal".to_owned()
    } else {
        truncate_to_width(&cleaned, objective_budget)
    };
    let title_text = format!(" {spinner_prefix}{objective} ");
    let title_style = Style::default()
        .fg(theme.accent_plan)
        .bg(theme.bg_base)
        .add_modifier(Modifier::BOLD);
    buf.set_span_safe(
        area.x + 2,
        area.y,
        &Span::styled(title_text, title_style),
        title_cols as u16,
    );

    let close_style = if close_hovered {
        Style::default()
            .fg(theme.text_primary)
            .bg(theme.bg_base)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.gray).bg(theme.bg_base)
    };
    buf.set_span_safe(
        close_x,
        area.y,
        &Span::styled(close_text, close_style),
        close_w,
    );
    let close_rect = Rect::new(close_x, area.y, close_w, 1);
    let partial_output = || GoalDetailRenderOutput {
        area,
        close: Some(close_rect),
        projection_toggle: None,
    };

    let mut y = inner.y;
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2);

    if board.full_board {
        let header = format!(
            "Blackboard r{} (objective r{}) — full document",
            goal.plan_revision, goal.objective_revision
        );
        buf.set_line_safe(
            x,
            y,
            &Line::from(Span::styled(
                header,
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            )),
            w,
        );
        y += 1;

        let footer_y = inner.y + inner.height.saturating_sub(1);
        let viewport_height = footer_y.saturating_sub(y) as usize;
        let max_scroll = board.full_lines.len().saturating_sub(viewport_height);
        board.scroll = (board.scroll as usize).min(max_scroll) as u16;
        if board.full_lines.is_empty() && y < footer_y {
            buf.set_line_safe(
                x,
                y,
                &Line::from(Span::styled(
                    "Planning in background…",
                    Style::default().fg(theme.gray),
                )),
                w,
            );
        } else {
            for (row, line) in board
                .full_lines
                .iter()
                .skip(board.scroll as usize)
                .take(viewport_height)
                .enumerate()
            {
                render_board_line(buf, line, x, y + row as u16, w);
            }
        }
        let projection_toggle =
            render_projection_footer(buf, x, footer_y, w, true, projection_toggle_hovered, &theme);
        return Some(GoalDetailRenderOutput {
            area,
            close: Some(close_rect),
            projection_toggle,
        });
    }

    // ── Status line ──
    let (status_text, status_color, phase_text) = status_label(goal);
    let mut status_spans = vec![
        Span::styled("Status: ", Style::default().fg(theme.gray)),
        Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if !phase_text.is_empty() {
        status_spans.push(Span::styled(
            format!(" \u{2014} {phase_text}"),
            Style::default().fg(theme.gray_bright),
        ));
    }

    buf.set_line_safe(x, y, &Line::from(status_spans), w);
    y += 1;

    if y >= inner.y + inner.height {
        return Some(partial_output());
    }

    if goal.status.is_paused() {
        let hint = format!(
            "Status: {} \u{2014} type /goal resume to continue",
            goal.status.pause_label()
        );
        buf.set_line_safe(
            x,
            y,
            &Line::from(Span::styled(hint, Style::default().fg(theme.warning))),
            w,
        );
        y += 1;
    }

    if goal.status.is_paused()
        && let Some(msg) = goal.pause_message.as_deref()
    {
        let formatted = format_pause_reason(msg);
        for line in wrap_pause_message_lines(&formatted, w) {
            if y >= inner.y + inner.height {
                return Some(partial_output());
            }
            buf.set_line_safe(
                x,
                y,
                &Line::from(Span::styled(line, Style::default().fg(theme.warning))),
                w,
            );
            y += 1;
        }
    }

    // ── Budget / tokens line with optional progress bar ──
    let tokens_str =
        format_tokens_compact(goal.live_tokens_used(context_used, active_subagent_tokens));
    let elapsed_str = format_elapsed(goal.live_elapsed_ms_at(frame_stamp.now()));

    let (pct, budget_display) = if let Some(budget) = goal.token_budget.filter(|&b| b > 0) {
        let live = goal.live_tokens_used(context_used, active_subagent_tokens);
        let p = (live as f64 / budget as f64).min(1.0) as f32;
        let budget_str = format_tokens_compact(budget);
        (p, format!("{tokens_str} / {budget_str} tokens"))
    } else {
        (0.0, format!("{tokens_str} tokens"))
    };
    let has_budget = goal.token_budget.is_some_and(|b| b > 0);
    let budget_label = if has_budget {
        let pct_display = format!(" ({:.0}%)", pct * 100.0);
        format!("Budget: {budget_display}{pct_display}  Elapsed: {elapsed_str}")
    } else {
        format!("Tokens: {budget_display}  Elapsed: {elapsed_str}")
    };
    buf.set_line_safe(
        x,
        y,
        &Line::from(Span::styled(
            budget_label,
            Style::default().fg(theme.gray_bright),
        )),
        w,
    );
    y += 1;

    if y >= inner.y + inner.height {
        return Some(partial_output());
    }

    // Progress bar — only when a budget is set.
    if has_budget {
        let bar_w = w.min(30);
        let fg = budget_color(pct, &theme);
        let bg = theme.scrollbar_bg;
        let bar_spans = progress_bar_spans(bar_w, pct, fg, bg);
        let pct_label = format!(" {:.0}%", pct * 100.0);
        let mut line_spans = vec![Span::styled("[", Style::default().fg(theme.gray))];
        line_spans.extend(bar_spans);
        line_spans.push(Span::styled("]", Style::default().fg(theme.gray)));
        line_spans.push(Span::styled(
            pct_label,
            Style::default().fg(theme.gray_bright),
        ));
        buf.set_line_safe(x, y, &Line::from(line_spans), w);
        y += 1;
    }

    if y >= inner.y + inner.height {
        return Some(partial_output());
    }

    // ── Blank separator ──
    y += 1;

    if y >= inner.y + inner.height {
        return Some(partial_output());
    }

    // ── Task-list projection of the durable Markdown blackboard ──
    let task_pct = if board.task_count == 0 {
        0.0
    } else {
        board.completed_task_count as f32 / board.task_count as f32
    };
    let task_header = if board.task_count == 0 {
        format!(
            "Tasks — blackboard r{} (objective r{})",
            goal.plan_revision, goal.objective_revision
        )
    } else {
        format!(
            "Tasks: {}/{} complete ({:.0}%)",
            board.completed_task_count,
            board.task_count,
            task_pct * 100.0
        )
    };
    buf.set_line_safe(
        x,
        y,
        &Line::from(Span::styled(
            task_header,
            Style::default()
                .fg(theme.text_primary)
                .add_modifier(Modifier::BOLD),
        )),
        w,
    );
    y += 1;
    if board.task_count == 0 {
        let message = if goal.plan_markdown.trim().is_empty() {
            "Planning in background…"
        } else {
            "No task-list items in the current board. Open the full board to inspect it."
        };
        buf.set_line_safe(
            x,
            y,
            &Line::from(Span::styled(message, Style::default().fg(theme.gray))),
            w,
        );
        y += 1;
    } else {
        let bar_w = w.min(30);
        let bar_spans =
            progress_bar_spans(bar_w, task_pct, theme.accent_success, theme.scrollbar_bg);
        let mut line_spans = vec![Span::styled("[", Style::default().fg(theme.gray))];
        line_spans.extend(bar_spans);
        line_spans.push(Span::styled("]", Style::default().fg(theme.gray)));
        buf.set_line_safe(x, y, &Line::from(line_spans), w);
        y += 1;

        for line in board.task_lines.iter().take(MAX_TASK_DISPLAY_ROWS) {
            if y >= inner.y + inner.height.saturating_sub(1) {
                break;
            }
            render_board_line(buf, line, x, y, w);
            y += 1;
        }
        if board.task_lines.len() > MAX_TASK_DISPLAY_ROWS {
            let remaining = board.task_lines.len() - MAX_TASK_DISPLAY_ROWS;
            buf.set_line_safe(
                x,
                y,
                &Line::from(Span::styled(
                    format!("  +{remaining} more task rows"),
                    Style::default().fg(theme.gray),
                )),
                w,
            );
            y += 1;
        }
    }
    if let Some(feedback) = goal.verifier_feedback.as_deref() {
        let feedback = truncate_to_width(
            &format!("Verifier: {}", strip_control_chars(feedback, false)),
            w as usize,
        );
        buf.set_line_safe(
            x,
            y,
            &Line::from(Span::styled(feedback, Style::default().fg(theme.warning))),
            w,
        );
        y += 1;
    }

    if y >= inner.y + inner.height {
        return Some(partial_output());
    }

    // ── Active subagent metrics (with a leading blank separator) ──
    if let Some(ref role) = goal.current_subagent_role {
        // Leading blank — budgeted in `subagent_lines` (renders only with the block).
        y += 1;
        if y >= inner.y + inner.height {
            return Some(partial_output());
        }
        let mut subagent_spans = vec![
            Span::styled("Active Subagent: ", Style::default().fg(theme.gray)),
            Span::styled(
                role.as_str(),
                Style::default()
                    .fg(theme.accent_running)
                    .add_modifier(Modifier::BOLD),
            ),
        ];
        let rounds = goal.total_worker_rounds + goal.total_verify_rounds;
        if rounds > 0 {
            subagent_spans.push(Span::styled(
                format!(" (round {rounds})"),
                Style::default().fg(theme.gray),
            ));
        }
        buf.set_line_safe(x, y, &Line::from(subagent_spans), w);
        y += 1;

        if y < inner.y + inner.height {
            // Subagent detail line.
            let mut detail_parts: Vec<String> = Vec::new();
            if let Some(tok) = goal.live_subagent_tokens {
                detail_parts.push(format!(
                    "Tokens: {}",
                    format_tokens_compact(tok.min(i64::MAX as u64) as i64)
                ));
            }
            if let Some(ctx) = goal.live_context_pct {
                detail_parts.push(format!("Context: {ctx}%"));
            }
            if let Some(turns) = goal.live_turn_count {
                detail_parts.push(format!("Turns: {turns}"));
            }
            if let Some(tools) = goal.live_tool_call_count {
                detail_parts.push(format!("Tools: {tools}"));
            }
            if !detail_parts.is_empty() {
                let detail = format!("  {}", detail_parts.join("  "));
                buf.set_line_safe(
                    x,
                    y,
                    &Line::from(Span::styled(detail, Style::default().fg(theme.gray_bright))),
                    w,
                );
                y += 1;
            }
        }

        // Per-model token breakdown, under the active-subagent block.
        // `per_model_row_count` is the shared gate/cap (height ↔ render).
        if per_model_row_count(&goal.live_tokens_by_model) > 0 {
            use unicode_width::UnicodeWidthStr;
            for (model_id, tokens) in goal.live_tokens_by_model.iter().take(MAX_MODEL_DISPLAY) {
                if y >= inner.y + inner.height {
                    return Some(partial_output());
                }
                let tokens_str = format_tokens_compact((*tokens).min(i64::MAX as u64) as i64);
                // Budget the model id to the columns left after the "  "
                // indent and "  <tokens>" suffix, measured in display
                // columns (not bytes) so wide glyphs never overflow the row.
                let id_budget = (w as usize)
                    .saturating_sub(4)
                    .saturating_sub(UnicodeWidthStr::width(tokens_str.as_str()));
                let id = truncate_to_width(model_id, id_budget);
                buf.set_line_safe(
                    x,
                    y,
                    &Line::from(Span::styled(
                        format!("  {id}  {tokens_str}"),
                        Style::default().fg(theme.gray_bright),
                    )),
                    w,
                );
                y += 1;
            }
            if goal.live_tokens_by_model.len() > MAX_MODEL_DISPLAY && y < inner.y + inner.height {
                let remaining = goal.live_tokens_by_model.len() - MAX_MODEL_DISPLAY;
                buf.set_line_safe(
                    x,
                    y,
                    &Line::from(Span::styled(
                        format!("  +{remaining} more"),
                        Style::default().fg(theme.gray),
                    )),
                    w,
                );
                y += 1;
            }
        }
    }

    if y >= inner.y + inner.height {
        return Some(partial_output());
    }

    // ── Recent history (with a leading blank separator) ──
    if goal.last_event.is_some() {
        // Leading blank — budgeted in `history_lines` (renders only with the block).
        y += 1;
        if y >= inner.y + inner.height {
            return Some(partial_output());
        }
        buf.set_line_safe(
            x,
            y,
            &Line::from(Span::styled(
                "Recent History:",
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            )),
            w,
        );
        y += 1;

        if y < inner.y + inner.height
            && let Some(ref event) = goal.last_event
        {
            // Humanize both the event label (folding in the detail) and the
            // timestamp so the user sees "2m ago  Paused: doom loop", not the
            // raw wire vocabulary. The timestamp renders first (left gutter),
            // then the humanized label — matching the span order below.
            let label = humanize_goal_event(event, goal.last_event_detail.as_deref());
            let ts_display = humanize_event_timestamp(
                goal.last_event_timestamp.as_deref().unwrap_or(""),
                frame_stamp.wall_now(),
            );
            let prefix = if ts_display.is_empty() {
                "  ".to_owned()
            } else {
                format!("  {ts_display}  ")
            };
            let spans = vec![
                Span::styled(prefix, Style::default().fg(theme.gray)),
                Span::styled(label, Style::default().fg(theme.text_secondary)),
            ];
            buf.set_line_safe(x, y, &Line::from(spans), w);
            y += 1;
        }
    }

    // ── Commands hint ──
    let projection_toggle = (y < inner.y + inner.height)
        .then(|| render_projection_footer(buf, x, y, w, false, projection_toggle_hovered, &theme));

    Some(GoalDetailRenderOutput {
        area,
        close: Some(close_rect),
        projection_toggle: projection_toggle.flatten(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::agent::{GoalDisplayPhase, GoalDisplayState};

    fn buffer_text(buf: &Buffer) -> String {
        let area = buf.area;
        (area.y..area.y + area.height)
            .map(|y| {
                (area.x..area.x + area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn compact_detail_renders_only_task_progress_from_the_blackboard() {
        let mut goal = GoalDisplayState::test_stub();
        goal.objective_revision = 2;
        goal.plan_revision = 4;
        goal.plan_markdown =
            "# Private-looking prose\nThis belongs only in the full board.\n\n- [x] implementation\n- [ ] verification"
                .into();
        goal.verifier_feedback = Some("missing restart evidence".into());
        goal.phase = GoalDisplayPhase::Verifying;
        let screen = Rect::new(0, 0, 100, 32);
        let mut board = GoalBoardRenderer::default();
        let mut buf = Buffer::empty(screen);
        render_goal_detail(
            &mut buf,
            screen,
            &goal,
            &mut board,
            crate::motion::FrameStamp::default(),
            None,
            0,
            false,
            false,
        );
        let text = buffer_text(&buf);
        assert!(text.contains("Tasks: 1/2 complete (50%)"));
        assert!(text.contains("[x] implementation"));
        assert!(text.contains("[ ] verification"));
        assert!(text.contains("implementation"));
        assert!(text.contains("verification"));
        assert!(!text.contains("Private-looking prose"));
        assert!(!text.contains("This belongs only in the full board"));
        assert!(text.contains("Verifier: missing restart evidence"));
        assert!(text.contains("Verifying"));
        assert!(text.contains("[Full board]"));
    }

    #[test]
    fn compact_detail_does_not_fall_back_to_full_prose_without_tasks() {
        let mut goal = GoalDisplayState::test_stub();
        goal.plan_markdown = "# Status\n\nEvidence exists, but no task list was supplied.".into();
        let screen = Rect::new(0, 0, 100, 24);
        let mut board = GoalBoardRenderer::default();
        let mut buf = Buffer::empty(screen);
        render_goal_detail(
            &mut buf,
            screen,
            &goal,
            &mut board,
            crate::motion::FrameStamp::default(),
            None,
            0,
            false,
            false,
        );

        let text = buffer_text(&buf);
        assert!(text.contains("No task-list items"));
        assert!(!text.contains("Evidence exists"));
        assert!(text.contains("[Full board]"));
    }

    #[test]
    fn full_board_renders_markdown_instead_of_showing_source_markers() {
        let mut goal = GoalDisplayState::test_stub();
        goal.plan_markdown = "# Current status\n\n**Ready** for verification".into();
        let screen = Rect::new(0, 0, 100, 30);
        let mut board = GoalBoardRenderer::default();
        board.show_full_board();
        let mut buf = Buffer::empty(screen);
        render_goal_detail(
            &mut buf,
            screen,
            &goal,
            &mut board,
            crate::motion::FrameStamp::default(),
            None,
            0,
            false,
            false,
        );
        let text = buffer_text(&buf);
        assert!(text.contains("Current status"));
        assert!(text.contains("Ready for verification"));
        assert!(!text.contains("# Current status"));
        assert!(!text.contains("**Ready**"));
        assert!(text.contains("[Task summary]"));
    }

    #[test]
    fn full_board_scrolls_through_the_complete_document() {
        let mut goal = GoalDisplayState::test_stub();
        goal.plan_markdown = (0..30)
            .map(|index| format!("paragraph {index}"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let screen = Rect::new(0, 0, 100, 14);
        let mut board = GoalBoardRenderer::default();
        board.show_full_board();

        let mut first = Buffer::empty(screen);
        render_goal_detail(
            &mut first,
            screen,
            &goal,
            &mut board,
            crate::motion::FrameStamp::default(),
            None,
            0,
            false,
            false,
        );
        let first_text = buffer_text(&first);
        assert!(first_text.contains("paragraph 0"));
        assert!(!first_text.contains("paragraph 29"));

        assert!(board.apply_scroll_key(crossterm::event::KeyCode::End));
        let mut last = Buffer::empty(screen);
        render_goal_detail(
            &mut last,
            screen,
            &goal,
            &mut board,
            crate::motion::FrameStamp::default(),
            None,
            0,
            false,
            false,
        );
        let last_text = buffer_text(&last);
        assert!(!last_text.contains("paragraph 0"));
        assert!(last_text.contains("paragraph 29"));
    }

    #[test]
    fn task_projection_accepts_nested_unordered_and_ordered_markdown_lists() {
        let source = concat!(
            "- [x] done\n",
            "  * [ ] nested\n",
            "> 2. [X] quoted order\n",
            "+ [ ] next\n",
            "- [maybe] prose\n",
            "plain [x] text\n"
        );
        let tasks: Vec<_> = source.lines().filter_map(parse_markdown_task).collect();
        assert_eq!(tasks.len(), 4);
        assert_eq!(tasks.iter().filter(|task| task.complete).count(), 2);
        assert_eq!(tasks[1].label, "nested");
        assert_eq!(tasks[2].label, "quoted order");
    }

    #[test]
    fn replacing_the_board_resets_full_document_scroll() {
        let mut board = GoalBoardRenderer::default();
        board.show_full_board();
        board.refresh("- [ ] old", 80);
        board.scroll = 42;
        board.refresh("- [ ] new", 80);
        assert!(board.is_full_board());
        assert_eq!(board.scroll, 0);
        assert_eq!(board.task_count, 1);
    }
}
