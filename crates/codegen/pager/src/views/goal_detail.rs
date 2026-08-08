//! Expanded goal detail overlay — full-screen popup showing goal progress
//! with token budget bar, todo list, and event history.
//!
//! Rendered as a centered overlay when `AgentView::show_goal_detail` is true
//! and `goal_state` is `Some`. Dismissed by `Esc` or `g`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::agent::{GoalDisplayState, GoalDisplayStatus};
use crate::render::SafeBuf;
use crate::theme::Theme;
use crate::views::agent_status::{active_phase_label, format_tokens_compact};
use crate::views::progress_bar::progress_bar_spans;

/// Maximum Markdown blackboard rows displayed before truncation.
const MAX_PLAN_DISPLAY: usize = 15;

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
fn humanize_event_timestamp(ts: &str) -> String {
    if ts.is_empty() {
        return String::new();
    }
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) else {
        return strip_control_chars(ts, false);
    };
    let secs = chrono::Utc::now()
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
pub fn goal_detail_area(screen: Rect, goal: &GoalDisplayState) -> Rect {
    let width_pct = 0.90f32;
    let preferred_w = (screen.width as f32 * width_pct) as u16;
    let w = preferred_w
        .clamp(60, 140)
        .min(screen.width.saturating_sub(4));

    // Inner content width matches the render path:
    //   `inner` = block.inner(area) gives `w - 2` (the rounded border).
    //   We further indent by 1 column on each side (`x = inner.x + 1`,
    //   `w = inner.width - 2`), so the usable text width is `w - 4`.
    // Mirror that here so pause-message wrapping computes the same row
    // count the renderer will produce.
    let inner_w = w.saturating_sub(4);

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
    //   1  blackboard header
    //   N  Markdown rows (+ optional "+N more")
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
    let plan_line_count = goal
        .plan_markdown
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    let plan_lines = if plan_line_count == 0 {
        2u16 // header + "Planning…"
    } else {
        1 + plan_line_count.min(MAX_PLAN_DISPLAY) as u16
            + u16::from(plan_line_count > MAX_PLAN_DISPLAY)
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
        + plan_lines
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
pub fn render_goal_detail(
    buf: &mut Buffer,
    area: Rect,
    goal: &GoalDisplayState,
    frame_stamp: crate::motion::FrameStamp,
    context_used: Option<u64>,
    active_subagent_tokens: u64,
    close_hovered: bool,
) -> Option<Rect> {
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

    let mut y = inner.y;
    let x = inner.x + 1;
    let w = inner.width.saturating_sub(2);

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
        return Some(close_rect);
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
                return Some(close_rect);
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
        return Some(close_rect);
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
        return Some(close_rect);
    }

    // ── Blank separator ──
    y += 1;

    if y >= inner.y + inner.height {
        return Some(close_rect);
    }

    // ── Durable Markdown blackboard ──
    buf.set_line_safe(
        x,
        y,
        &Line::from(Span::styled(
            format!(
                "Plan r{} (objective r{}):",
                goal.plan_revision, goal.objective_revision
            ),
            Style::default()
                .fg(theme.text_primary)
                .add_modifier(Modifier::BOLD),
        )),
        w,
    );
    y += 1;
    let plan_lines: Vec<&str> = goal
        .plan_markdown
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if plan_lines.is_empty() {
        buf.set_line_safe(
            x,
            y,
            &Line::from(Span::styled(
                "  Planning in background…",
                Style::default().fg(theme.gray),
            )),
            w,
        );
        y += 1;
    } else {
        for line in plan_lines.iter().take(MAX_PLAN_DISPLAY) {
            if y >= inner.y + inner.height.saturating_sub(1) {
                break;
            }
            let display = truncate_to_width(&strip_control_chars(line, false), w as usize);
            buf.set_line_safe(
                x,
                y,
                &Line::from(Span::styled(
                    display,
                    Style::default().fg(theme.text_primary),
                )),
                w,
            );
            y += 1;
        }
        if plan_lines.len() > MAX_PLAN_DISPLAY {
            let remaining = plan_lines.len() - MAX_PLAN_DISPLAY;
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
        return Some(close_rect);
    }

    // ── Active subagent metrics (with a leading blank separator) ──
    if let Some(ref role) = goal.current_subagent_role {
        // Leading blank — budgeted in `subagent_lines` (renders only with the block).
        y += 1;
        if y >= inner.y + inner.height {
            return Some(close_rect);
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
                    return Some(close_rect);
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
        return Some(close_rect);
    }

    if y >= inner.y + inner.height {
        return Some(close_rect);
    }

    // ── Recent history (with a leading blank separator) ──
    if goal.last_event.is_some() {
        // Leading blank — budgeted in `history_lines` (renders only with the block).
        y += 1;
        if y >= inner.y + inner.height {
            return Some(close_rect);
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
            let ts_display =
                humanize_event_timestamp(goal.last_event_timestamp.as_deref().unwrap_or(""));
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
    if y < inner.y + inner.height {
        let hint_style = Style::default().fg(theme.gray_dim);
        let hint = "Esc: close  /goal resume | pause | status | clear";
        buf.set_line_safe(x, y, &Line::from(Span::styled(hint, hint_style)), w);
    }

    Some(close_rect)
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
    fn detail_renders_the_persisted_blackboard_and_revision() {
        let mut goal = GoalDisplayState::test_stub();
        goal.objective_revision = 2;
        goal.plan_revision = 4;
        goal.plan_markdown = "- [x] implementation\n- [ ] verification".into();
        goal.verifier_feedback = Some("missing restart evidence".into());
        goal.phase = GoalDisplayPhase::Verifying;
        let screen = Rect::new(0, 0, 100, 32);
        let area = goal_detail_area(screen, &goal);
        let mut buf = Buffer::empty(screen);
        render_goal_detail(
            &mut buf,
            area,
            &goal,
            crate::motion::FrameStamp::default(),
            None,
            0,
            false,
        );
        let text = buffer_text(&buf);
        assert!(text.contains("Plan r4 (objective r2):"));
        assert!(text.contains("- [x] implementation"));
        assert!(text.contains("Verifier: missing restart evidence"));
        assert!(text.contains("Verifying"));
    }
}
