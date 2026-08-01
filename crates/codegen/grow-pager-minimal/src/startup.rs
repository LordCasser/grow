//! Minimal-mode folder-trust and session-startup rendering.

use std::path::PathBuf;

use grow_pager::app::app_view::TrustState;
use grow_pager::theme::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub(super) enum MinimalStartupHint {
    TrustFolder { workspace: PathBuf },
    Starting,
}

pub(super) fn minimal_startup_hint(trust: &TrustState) -> MinimalStartupHint {
    match trust {
        TrustState::Pending { workspace } => MinimalStartupHint::TrustFolder {
            workspace: workspace.clone(),
        },
        TrustState::Done => MinimalStartupHint::Starting,
    }
}

pub(super) fn startup_hint_rows(hint: &MinimalStartupHint, width: u16) -> u16 {
    match hint {
        MinimalStartupHint::TrustFolder { workspace } => {
            1 + wrapped_rows(&workspace.display().to_string(), width) + 1 + 2 + 1 + 2 + 1 + 1
        }
        MinimalStartupHint::Starting => 1,
    }
}

fn wrapped_rows(text: &str, width: u16) -> u16 {
    let chars = text.chars().filter(|ch| !ch.is_control()).count();
    chars.max(1).div_ceil(width.max(1) as usize) as u16
}

fn put_line(buf: &mut Buffer, area: Rect, y: u16, bottom: u16, line: Line<'_>) -> u16 {
    if y < bottom {
        buf.set_line(area.x, y, &line, area.width);
        y + 1
    } else {
        y
    }
}

fn render_wrapped(
    buf: &mut Buffer,
    area: Rect,
    start_y: u16,
    bottom: u16,
    text: &str,
    style: Style,
) -> u16 {
    let width = area.width.max(1);
    let bounds = buf.area();
    let (max_x, max_y) = (bounds.right(), bounds.bottom());
    let mut col = 0u16;
    let mut y = start_y;
    for ch in text.chars().filter(|ch| !ch.is_control()) {
        if col >= width {
            col = 0;
            y = y.saturating_add(1);
        }
        if y >= bottom {
            return bottom;
        }
        let x = area.x + col;
        if x < max_x && y < max_y {
            buf[(x, y)].set_char(ch).set_style(style);
        }
        col += 1;
    }
    y.saturating_add(1)
}

pub(super) fn render_startup(
    buf: &mut Buffer,
    area: Rect,
    theme: &Theme,
    hint: &MinimalStartupHint,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bottom = area.bottom();
    let mut y = area.y;
    let muted = theme.muted().bg(Color::Reset);
    let bold = Style::default()
        .fg(theme.text_primary)
        .add_modifier(Modifier::BOLD)
        .bg(Color::Reset);

    match hint {
        MinimalStartupHint::TrustFolder { workspace } => {
            y = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(Span::styled(
                    "Do you trust the contents of this directory?",
                    bold,
                )),
            );
            y = render_wrapped(
                buf,
                area,
                y,
                bottom,
                &workspace.display().to_string(),
                Style::default().fg(theme.accent_user).bg(Color::Reset),
            );
            y = put_line(buf, area, y, bottom, Line::default());
            y = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(Span::styled(
                    "Grow may run or modify contents in this directory,",
                    muted,
                )),
            );
            y = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(Span::styled("posing security risks.", muted)),
            );
            y = put_line(buf, area, y, bottom, Line::default());
            y = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(vec![
                    Span::styled("y", bold),
                    Span::styled("  Yes, proceed", muted),
                ]),
            );
            y = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(vec![
                    Span::styled("n", bold),
                    Span::styled("  No, quit", muted),
                ]),
            );
            y = put_line(buf, area, y, bottom, Line::default());
            let _ = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(Span::styled(
                    "Enter or y to trust · n or Esc to quit",
                    muted,
                )),
            );
        }
        MinimalStartupHint::Starting => {
            let _ = put_line(
                buf,
                area,
                y,
                bottom,
                Line::from(Span::styled("Starting your session…", muted)),
            );
        }
    }
}
