//! Agent status bar — composable right-aligned status items with separators.
//!
//! Provides [`AgentStatusBar`] which collects items as `Line<'static>` spans,
//! lays them out right-aligned with dim `│` separators, and renders into a
//! buffer row.  Returns hit-test areas keyed by item ID.
//!
//! # Example
//!
//! ```ignore
//! let mut status = AgentStatusBar::new(&theme);
//! status.push("context", context_line);
//! status.push("badge", badge_line);
//! let areas = status.render(buf, status_bar_rect);
//! let context_area = areas.get("context");
//! ```

use std::collections::HashMap;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::context_bar::SEPARATOR;
use crate::app::session::McpInitProgress;
use crate::app::session::{GoalDisplayState, GoalDisplayStatus};
use crate::theme::Theme;

/// A named status bar item.
struct StatusEntry {
    /// Identifier for hit-test lookup (e.g., "context", "badge").
    id: &'static str,
    /// Pre-built styled content.
    line: Line<'static>,
    /// Display width in columns.
    width: u16,
}

/// Builder for the agent status bar.
///
/// Collect items with [`push`], then call [`render`] to lay them out
/// right-aligned with separators and get back hit-test areas.
pub struct AgentStatusBar<'a> {
    items: Vec<StatusEntry>,
    theme: &'a Theme,
    /// Padding from the right edge of the status bar area.
    right_pad: u16,
}

impl<'a> AgentStatusBar<'a> {
    /// Create a new empty status bar.
    pub fn new(theme: &'a Theme) -> Self {
        Self {
            items: Vec::new(),
            theme,
            right_pad: 0,
        }
    }

    /// Add an item to the status bar.
    ///
    /// Items are rendered left-to-right in push order, but the entire
    /// group is right-aligned within the status bar area.
    pub fn push(&mut self, id: &'static str, line: Line<'static>) {
        let width = line.width() as u16;
        self.items.push(StatusEntry { id, line, width });
    }

    /// Build a separator span: ` │ ` in dim color.
    fn separator(&self) -> Span<'static> {
        Span::styled(
            format!(" {SEPARATOR} "),
            Style::default()
                .fg(self.theme.gray_dim)
                .bg(self.theme.bg_base),
        )
    }

    /// Render all items right-aligned into the given area.
    ///
    /// Layout: `··· item0 │ item1 │ item2` — separators appear only *between*
    /// items, never before the first or after the last.
    ///
    /// Returns a map of item ID → screen `Rect` for hit-testing.
    pub fn render(self, buf: &mut Buffer, area: Rect) -> HashMap<&'static str, Rect> {
        if area.height == 0 || area.width == 0 || self.items.is_empty() {
            return HashMap::new();
        }

        // Fill background
        buf.set_style(area, Style::default().bg(self.theme.bg_base));

        let sep = self.separator();
        let sep_w = sep.width() as u16; // 3

        // Keep the rightmost entries complete when the row is narrow. Clip at
        // most the leftmost visible entry, so a wider item added on the left
        // cannot push context or queue state beyond the buffer.
        let available = area.width.saturating_sub(self.right_pad);
        let mut first_visible = self.items.len();
        let mut first_visible_width = None;
        let mut total_width = 0u16;
        for (idx, entry) in self.items.iter().enumerate().rev() {
            let separator_width = if first_visible == self.items.len() {
                0
            } else {
                sep_w
            };
            let required = entry.width.saturating_add(separator_width);
            let remaining = available.saturating_sub(total_width);
            if required > remaining {
                let clipped_width = remaining.saturating_sub(separator_width);
                if clipped_width > 0 {
                    total_width = total_width
                        .saturating_add(separator_width)
                        .saturating_add(clipped_width);
                    first_visible = idx;
                    first_visible_width = Some(clipped_width);
                }
                break;
            }
            total_width = total_width.saturating_add(required);
            first_visible = idx;
        }
        if first_visible == self.items.len() {
            return HashMap::new();
        }

        // Right-align: compute starting x
        let start_x = area.x.saturating_add(
            area.width
                .saturating_sub(self.right_pad.saturating_add(total_width)),
        );

        let mut x = start_x;
        let mut areas = HashMap::new();

        for (i, entry) in self.items[first_visible..].iter().enumerate() {
            // Separator before every item except the first.
            if i > 0 {
                buf.set_span(x, area.y, &sep, sep_w);
                x += sep_w;
            }

            let visible_width = if i == 0 {
                first_visible_width.unwrap_or(entry.width)
            } else {
                entry.width
            };
            // Render item, clipping only the leftmost retained entry if needed.
            buf.set_line(x, area.y, &entry.line, visible_width);
            areas.insert(
                entry.id,
                Rect {
                    x,
                    y: area.y,
                    width: visible_width,
                    height: 1,
                },
            );
            x += visible_width;
        }

        areas
    }
}

// ---------------------------------------------------------------------------
// Goal and usage status lines
// ---------------------------------------------------------------------------

/// Format a token count compactly: `500`, `1.5k`, `50k`, `1.5M`.
pub(crate) fn format_tokens_compact(tokens: i64) -> String {
    let sign = if tokens < 0 { "-" } else { "" };
    format!("{sign}{}", format_tokens_compact_u64(tokens.unsigned_abs()))
}

/// Build the ordinary session usage projection shown in the Goal slot.
pub fn session_usage_status_line(
    usage: Option<&shell::extensions::notification::PromptUsage>,
    theme: &Theme,
    hovered: bool,
) -> Line<'static> {
    let text = match usage {
        Some(usage) => {
            let totals = &usage.totals;
            let total_tokens = totals.input_tokens.saturating_add(totals.output_tokens);
            let lower_bound = if usage.usage_is_incomplete { "≥" } else { "" };
            let recorded = if usage.usage_is_incomplete {
                " recorded"
            } else {
                ""
            };
            format!(
                "{lower_bound}{} tokens · {} cache{recorded}",
                format_tokens_compact_u64(total_tokens),
                crate::app::status_blocks::cache_hit_rate(
                    totals.input_tokens,
                    totals.cached_read_tokens,
                )
            )
        }
        None => "— tokens · — cache".to_string(),
    };
    let mut style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    if hovered {
        style = style
            .add_modifier(ratatui::style::Modifier::BOLD)
            .add_modifier(ratatui::style::Modifier::UNDERLINED);
    }
    Line::from(Span::styled(text, style))
}

fn format_tokens_compact_u64(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        let m = tokens as f64 / 1_000_000.0;
        format!("{m:.1}M").replace(".0M", "M")
    } else if tokens >= 1_000 {
        let k = tokens as f64 / 1_000.0;
        format!("{k:.1}k").replace(".0k", "k")
    } else {
        tokens.to_string()
    }
}

/// Format elapsed milliseconds compactly: `5s`, `3m`, `2h`.
fn format_elapsed_compact(ms: u64) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

fn goal_phase_label(goal: &GoalDisplayState) -> String {
    match goal.status {
        GoalDisplayStatus::Paused | GoalDisplayStatus::Blocked => {
            goal.status.stopped_label().into()
        }
        GoalDisplayStatus::BudgetLimited => "Budget".into(),
        GoalDisplayStatus::Complete => "Done".into(),
        GoalDisplayStatus::Active => "Active".into(),
    }
}

/// Build a compact goal status `Line` for the agent status bar.
///
/// Format: `[Goal: {label}]  {tokens}  {elapsed}`
///
/// When `hovered` is true the label is bolded/underlined to signal
/// clickability. When the goal is `Active`, the shared frame drives its spinner.
pub fn goal_status_line(
    goal: &GoalDisplayState,
    theme: &Theme,
    hovered: bool,
    frame_stamp: crate::motion::FrameStamp,
    _context_used: Option<u64>,
    _active_subagent_tokens: u64,
) -> Line<'static> {
    let label = goal_phase_label(goal);

    let tokens_str = format_tokens_compact(goal.tokens_used);
    let lower_bound = if goal.usage_incomplete { "≥" } else { "" };
    let tokens_display = match goal.token_budget {
        Some(budget) if budget > 0 => {
            format!(
                "{lower_bound}{}/{} tokens",
                tokens_str,
                format_tokens_compact(budget)
            )
        }
        _ => format!("{lower_bound}{} tokens", tokens_str),
    };

    let elapsed_str = format_elapsed_compact(goal.live_elapsed_ms_at(frame_stamp.now()));

    let dim_style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    // Restartable stopped Goals use an inverted warning-colour chip so their
    // state is visible without implying foreground activity.
    let mut label_style = if goal.status.uses_warning_chip() {
        Style::default().fg(theme.bg_base).bg(theme.warning)
    } else {
        Style::default().fg(theme.accent_plan).bg(theme.bg_base)
    };

    if hovered {
        label_style = label_style
            .add_modifier(ratatui::style::Modifier::BOLD)
            .add_modifier(ratatui::style::Modifier::UNDERLINED);
    }

    let is_active = matches!(goal.status, GoalDisplayStatus::Active);

    let chip_name = "Goal";
    let goal_text = if is_active {
        let frames = crate::glyphs::dot_spinner_frames();
        let frame = crate::motion::spinner_glyph(frame_stamp, frames);
        format!("{frame} {chip_name}: {label}")
    } else {
        format!("{chip_name}: {label}")
    };

    Line::from(vec![
        Span::styled("[", dim_style),
        Span::styled(goal_text, label_style),
        Span::styled("]", dim_style),
        Span::styled(format!("  {tokens_display}  {elapsed_str}"), dim_style),
    ])
}

// ---------------------------------------------------------------------------
// MCP connecting indicator
// ---------------------------------------------------------------------------

/// Build the compact MCP-connecting indicator for the agent status bar.
///
/// Format: `⠋ MCP (1/4)` — a time-based braille spinner followed by the
/// connected/total server count.
/// Rendered in `theme.gray_dim` so it reads as dim, matching the directory path
/// shown on the same row.
///
/// Returns `None` while `progress.total == 0` (a startup seed). That state
/// renders `⠋ Starting session…` above the prompt (see
/// [`crate::views::turn_status`]) rather than as a chip here — the top-bar chip
/// only shows real server counts once the shell reports `total > 0`.
pub fn mcp_status_line(
    progress: &McpInitProgress,
    frame: crate::motion::FrameStamp,
    theme: &Theme,
) -> Option<Line<'static>> {
    if progress.total == 0 {
        return None;
    }
    let frames = crate::glyphs::braille_spinner_frames();
    let spinner = crate::motion::spinner_glyph(frame, frames);
    let style = Style::default().fg(theme.gray_dim).bg(theme.bg_base);
    Some(Line::from(vec![
        Span::styled(format!("{} ", spinner), style),
        Span::styled(
            format!("MCP ({}/{})", progress.connected, progress.total),
            style,
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(
        input: u64,
        output: u64,
        cached: u64,
        incomplete: bool,
    ) -> shell::extensions::notification::PromptUsage {
        shell::extensions::notification::PromptUsage {
            totals: shell::extensions::notification::PromptUsageModel {
                input_tokens: input,
                output_tokens: output,
                cached_read_tokens: cached,
                model_calls: 1,
                ..Default::default()
            },
            usage_is_incomplete: incomplete,
            ..Default::default()
        }
    }

    fn line_text(line: Line<'static>) -> String {
        line.spans
            .into_iter()
            .map(|span| span.content.into_owned())
            .collect()
    }

    #[test]
    fn compact_formatting_is_stable() {
        assert_eq!(format_tokens_compact(1_500), "1.5k");
        assert_eq!(format_tokens_compact(1_000_000), "1M");
        assert_eq!(format_elapsed_compact(180_000), "3m");
    }

    #[test]
    fn session_usage_formats_unknown_large_incomplete_and_invalid_cache() {
        let theme = Theme::current();
        assert_eq!(
            line_text(session_usage_status_line(None, &theme, false)),
            "— tokens · — cache"
        );
        assert_eq!(
            line_text(session_usage_status_line(
                Some(&usage(1_350_000, 150_000, 1_215_000, false)),
                &theme,
                false,
            )),
            "1.5M tokens · 90.00% cache"
        );
        assert_eq!(
            line_text(session_usage_status_line(
                Some(&usage(100, 20, 90, true)),
                &theme,
                false,
            )),
            "≥120 tokens · 90.00% cache recorded"
        );
        assert_eq!(
            line_text(session_usage_status_line(
                Some(&usage(0, 5, 1, false)),
                &theme,
                false,
            )),
            "5 tokens · N/A cache"
        );
        assert_eq!(
            line_text(session_usage_status_line(
                Some(&usage(10, 5, 11, false)),
                &theme,
                false,
            )),
            "15 tokens · N/A cache"
        );
    }

    #[test]
    fn narrow_status_bar_keeps_rightmost_entries_and_bounds_hit_areas() {
        let theme = Theme::current();
        let area = Rect::new(7, 3, 12, 1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 24, 6));
        let mut status = AgentStatusBar::new(&theme);
        status.push("usage", Line::from("1.5M tokens · 90.00% cache"));
        status.push("context", Line::from("ctx 50%"));

        let areas = status.render(&mut buf, area);

        let usage = areas.get("usage").expect("left usage is minimally clipped");
        assert!(usage.width > 0);
        assert!(usage.x >= area.x);
        assert!(usage.right() <= area.right());
        let context = areas
            .get("context")
            .expect("rightmost context remains visible");
        assert!(context.x >= area.x);
        assert!(context.right() <= area.right());
    }
}
