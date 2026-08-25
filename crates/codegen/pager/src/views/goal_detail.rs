//! Long-lived Goal detail overlay.

use crossterm::event::{KeyCode, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

use crate::app::session::{GoalDisplayState, GoalDisplayStatus};
use crate::theme::Theme;
use crate::views::agent_status::format_tokens_compact;

#[derive(Default)]
pub(crate) struct GoalDetailRenderer {
    scroll: u16,
}

impl GoalDetailRenderer {
    pub(crate) fn reset_navigation(&mut self) {
        self.scroll = 0;
    }

    pub(crate) fn apply_scroll_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Up | KeyCode::Char('k') => self.apply_scroll_delta(-1),
            KeyCode::Down | KeyCode::Char('j') => self.apply_scroll_delta(1),
            KeyCode::PageUp => self.apply_scroll_delta(-8),
            KeyCode::PageDown => self.apply_scroll_delta(8),
            KeyCode::Home => self.scroll = 0,
            _ => return false,
        }
        true
    }

    pub(crate) fn apply_mouse_scroll(&mut self, kind: MouseEventKind) -> bool {
        match kind {
            MouseEventKind::ScrollUp => self.apply_scroll_delta(-3),
            MouseEventKind::ScrollDown => self.apply_scroll_delta(3),
            _ => return false,
        }
        true
    }

    pub(crate) fn apply_scroll_delta(&mut self, lines: i32) {
        self.scroll = if lines < 0 {
            self.scroll.saturating_sub(lines.unsigned_abs() as u16)
        } else {
            self.scroll.saturating_add(lines as u16)
        };
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GoalDetailRenderOutput {
    pub(crate) area: Rect,
    pub(crate) close: Option<Rect>,
}

fn status_label(status: GoalDisplayStatus) -> &'static str {
    match status {
        GoalDisplayStatus::Active => "Active",
        GoalDisplayStatus::Paused => "Paused",
        GoalDisplayStatus::Blocked => "Blocked",
        GoalDisplayStatus::UsageLimited => "Usage limited",
        GoalDisplayStatus::BudgetLimited => "Budget limited",
        GoalDisplayStatus::Complete => "Complete",
    }
}

fn centered_area(screen: Rect) -> Rect {
    let width = screen.width.saturating_sub(4).clamp(36, 100);
    let height = screen.height.saturating_sub(4).clamp(10, 22);
    Rect::new(
        screen.x + screen.width.saturating_sub(width) / 2,
        screen.y + screen.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

fn wrapped_lines(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let clean = strip_control_chars(text, true);
    let mut lines = Vec::new();
    for raw in clean.lines() {
        if raw.is_empty() {
            lines.push(Line::default());
            continue;
        }
        lines.extend(
            textwrap::wrap(raw, width.max(1))
                .into_iter()
                .map(|line| Line::from(Span::styled(line.into_owned(), style))),
        );
    }
    lines
}

pub(crate) fn render_goal_detail(
    buf: &mut Buffer,
    screen: Rect,
    goal: &GoalDisplayState,
    renderer: &mut GoalDetailRenderer,
    frame_stamp: crate::motion::FrameStamp,
    close_hovered: bool,
) -> Option<GoalDetailRenderOutput> {
    if screen.width < 20 || screen.height < 8 {
        return None;
    }
    let area = centered_area(screen);
    let theme = Theme::current();
    Clear.render(area, buf);
    let block = Block::default()
        .title(" Long-term Goal ")
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent_plan).bg(theme.bg_base))
        .style(Style::default().bg(theme.bg_base));
    let inner = block.inner(area);
    block.render(area, buf);

    let close_text = "[×]";
    let close = Rect::new(area.right().saturating_sub(5), area.y, 3, 1);
    Span::styled(
        close_text,
        Style::default()
            .fg(if close_hovered {
                theme.text_primary
            } else {
                theme.gray
            })
            .bg(theme.bg_base)
            .add_modifier(close_hovered.then_some(Modifier::BOLD).unwrap_or_default()),
    )
    .render(close, buf);

    let content_width = inner.width.saturating_sub(2) as usize;
    let mut lines = vec![
        Line::from(Span::styled(
            status_label(goal.status),
            Style::default().fg(if goal.status == GoalDisplayStatus::Active {
                theme.accent_plan
            } else if goal.status == GoalDisplayStatus::Complete {
                theme.accent_success
            } else {
                theme.warning
            }),
        )),
        Line::default(),
        Line::from(Span::styled(
            "Objective",
            Style::default()
                .fg(theme.text_primary)
                .add_modifier(Modifier::BOLD),
        )),
    ];
    lines.extend(wrapped_lines(
        &goal.objective,
        content_width,
        Style::default().fg(theme.text_primary),
    ));
    lines.push(Line::default());
    let usage = match goal.token_budget {
        Some(budget) => format!(
            "Usage  {}/{} tokens",
            format_tokens_compact(goal.tokens_used),
            format_tokens_compact(budget)
        ),
        None => format!("Usage  {} tokens", format_tokens_compact(goal.tokens_used)),
    };
    lines.push(Line::from(Span::styled(
        usage,
        Style::default().fg(theme.gray),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "Elapsed  {}",
            format_elapsed(goal.live_elapsed_ms_at(frame_stamp.now()))
        ),
        Style::default().fg(theme.gray),
    )));
    lines.push(Line::from(Span::styled(
        format!("Updated  {}", strip_control_chars(&goal.updated_at, false)),
        Style::default().fg(theme.gray_dim),
    )));
    if let Some(message) = goal.status_message.as_deref() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "Status message",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )));
        lines.extend(wrapped_lines(
            message,
            content_width,
            Style::default().fg(theme.text_primary),
        ));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "/goal edit · pause · restart · clear",
        Style::default().fg(theme.gray_dim),
    )));

    let max_scroll = lines.len().saturating_sub(inner.height as usize) as u16;
    renderer.scroll = renderer.scroll.min(max_scroll);
    Paragraph::new(lines).scroll((renderer.scroll, 0)).render(
        Rect::new(
            inner.x + 1,
            inner.y,
            inner.width.saturating_sub(2),
            inner.height,
        ),
        buf,
    );

    Some(GoalDetailRenderOutput {
        area,
        close: Some(close),
    })
}

pub(crate) fn format_elapsed(ms: u64) -> String {
    let total_seconds = ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

pub(crate) fn strip_control_chars(input: &str, keep_newlines: bool) -> String {
    input
        .chars()
        .filter(|character| {
            (!character.is_control()) || (keep_newlines && matches!(character, '\n' | '\t'))
        })
        .collect()
}

pub(crate) fn truncate_to_width(input: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if input.width() <= width {
        return input.to_string();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut output = String::new();
    let target = width - 1;
    let mut used = 0;
    for character in input.chars() {
        let next = character.width().unwrap_or(0);
        if used + next > target {
            break;
        }
        output.push(character);
        used += next;
    }
    output.push('…');
    output
}
