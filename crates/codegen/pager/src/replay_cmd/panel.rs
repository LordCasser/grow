use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PanelAction {
    Back,
    Toggle,
    SpeedUp,
    SpeedDown,
    Next,
    Follow,
    Help,
}

pub(super) struct PanelInfo<'a> {
    pub state: &'a str,
    pub speed: f64,
    pub progress: u8,
    pub time: &'a str,
    pub play_current: f64,
    pub play_total: f64,
    pub notice: Option<&'a str>,
    pub has_back_layer: bool,
    pub follow: bool,
    pub next: bool,
}

#[derive(Default)]
pub(super) struct Panel {
    hits: Vec<(Rect, PanelAction)>,
}

struct Segment {
    text: String,
    color: ratatui::style::Color,
    action: Option<PanelAction>,
}

impl Segment {
    fn new(text: impl Into<String>, color: ratatui::style::Color) -> Self {
        Self {
            text: text.into(),
            color,
            action: None,
        }
    }

    fn action(mut self, action: PanelAction) -> Self {
        self.action = Some(action);
        self
    }

    fn with_action(mut self, action: Option<PanelAction>) -> Self {
        self.action = action;
        self
    }
}

impl Panel {
    pub(super) fn height(area: Rect) -> u16 {
        if area.width == 0 {
            0
        } else {
            area.height.min(3)
        }
    }

    pub(super) fn render(&mut self, area: Rect, buf: &mut Buffer, info: &PanelInfo<'_>) {
        self.hits.clear();
        let height = area.height;
        if height == 0 {
            return;
        }

        let theme = Theme::current();
        let panel_area = Rect { height, ..area };
        buf.set_style(panel_area, Style::default().bg(theme.bg_base));

        if height < 3 || area.width < 3 {
            self.draw_compact(area, buf, info, &theme);
            return;
        }

        let inside = Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            1,
        );
        self.draw_box_line(
            area,
            buf,
            0,
            '╭',
            '╮',
            inside,
            vec![
                Segment::new(" Replay · ", theme.accent_assistant),
                Segment::new(info.state, theme.text_primary),
                Segment::new(
                    format!(" · {}× · {}% ", speed(info.speed), info.progress),
                    theme.gray,
                ),
                Segment::new("· 模拟流式 ", theme.gray),
            ],
            &theme,
        );

        let middle = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width.saturating_sub(2),
            1,
        );
        buf.set_string(
            area.x,
            area.y.saturating_add(1),
            "│",
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
        buf.set_string(
            area.right().saturating_sub(1),
            area.y.saturating_add(1),
            "│",
            Style::default().fg(theme.gray).bg(theme.bg_base),
        );
        self.draw_controls(middle, buf, info, &theme);

        let timestamp = info.notice.unwrap_or_else(|| info.time);
        let timestamp_label = if info.notice.is_some() {
            timestamp.to_owned()
        } else {
            format!(" 历史 {timestamp}")
        };
        let timestamp_color = if info.notice.is_some() {
            theme.warning
        } else {
            theme.text_secondary
        };
        self.draw_box_line(
            area,
            buf,
            2,
            '╰',
            '╯',
            Rect::new(inside.x, area.y.saturating_add(2), inside.width, 1),
            vec![
                Segment::new(timestamp_label, timestamp_color),
                Segment::new(
                    format!(
                        " · 回放 {}/{} ",
                        clock(info.play_current),
                        clock(info.play_total)
                    ),
                    theme.gray,
                ),
            ],
            &theme,
        );
    }

    pub(super) fn hit(&self, col: u16, row: u16) -> Option<PanelAction> {
        self.hits
            .iter()
            .find(|(rect, _)| {
                col >= rect.x
                    && col < rect.x.saturating_add(rect.width)
                    && row >= rect.y
                    && row < rect.y.saturating_add(rect.height)
            })
            .map(|(_, action)| *action)
    }

    fn draw_controls(&mut self, area: Rect, buf: &mut Buffer, info: &PanelInfo<'_>, theme: &Theme) {
        let width = area.width as usize;
        let finished = info.state.eq_ignore_ascii_case("finished");
        let toggle = if finished {
            "已结束"
        } else if info.state.eq_ignore_ascii_case("playing") {
            "F8 暂停"
        } else {
            "F8 继续"
        };
        let mut segments = Vec::new();
        if width >= 64 {
            segments.push(
                Segment::new(
                    format!(" {toggle} "),
                    if finished {
                        theme.gray
                    } else {
                        theme.text_primary
                    },
                )
                .with_action((!finished).then_some(PanelAction::Toggle)),
            );
            if !finished {
                segments.push(
                    Segment::new(" [-] ", theme.text_secondary).action(PanelAction::SpeedDown),
                );
                segments
                    .push(Segment::new(" [+] ", theme.text_secondary).action(PanelAction::SpeedUp));
            }
            if info.next {
                segments.push(
                    Segment::new(" [ ] 下一条 ", theme.text_secondary).action(PanelAction::Next),
                );
            }
            segments.push(
                Segment::new(
                    if info.follow {
                        " End 跟随✓ "
                    } else {
                        " End 跟随 "
                    },
                    theme.text_secondary,
                )
                .action(PanelAction::Follow),
            );
            segments.push(Segment::new(" [?] 帮助 ", theme.gray).action(PanelAction::Help));
            segments.push(
                Segment::new(
                    if info.has_back_layer {
                        " [Esc] 返回"
                    } else {
                        " [Esc] 退出"
                    },
                    theme.text_primary,
                )
                .action(PanelAction::Back),
            );
        } else if width >= 20 {
            segments.push(
                Segment::new(
                    if finished { " 完成 " } else { " F8 " },
                    if finished {
                        theme.gray
                    } else {
                        theme.text_primary
                    },
                )
                .with_action((!finished).then_some(PanelAction::Toggle)),
            );
            if !finished {
                segments
                    .push(Segment::new(" − ", theme.text_secondary).action(PanelAction::SpeedDown));
                segments
                    .push(Segment::new(" + ", theme.text_secondary).action(PanelAction::SpeedUp));
            }
            if info.next {
                segments.push(Segment::new(" ] ", theme.text_secondary).action(PanelAction::Next));
            }
            segments.push(Segment::new(" End ", theme.text_secondary).action(PanelAction::Follow));
            segments.push(Segment::new(" ? ", theme.gray).action(PanelAction::Help));
            segments.push(Segment::new(" Esc ", theme.text_primary).action(PanelAction::Back));
        } else if width >= 16 {
            segments.push(
                Segment::new(
                    if finished { " 完成 " } else { " F8 " },
                    if finished {
                        theme.gray
                    } else {
                        theme.text_primary
                    },
                )
                .with_action((!finished).then_some(PanelAction::Toggle)),
            );
            segments.push(Segment::new(" [?] ", theme.gray).action(PanelAction::Help));
            segments.push(Segment::new(" [Esc] ", theme.text_primary).action(PanelAction::Back));
        } else if width >= 7 {
            segments.push(Segment::new("[?] ", theme.gray).action(PanelAction::Help));
            segments.push(Segment::new("Esc", theme.text_primary).action(PanelAction::Back));
        } else {
            // Too little room for a complete visible key label; do not create partial hot areas.
            segments.push(Segment::new("?", theme.gray));
        }
        self.draw_line(area, buf, 0, segments);
    }

    fn draw_compact(&mut self, area: Rect, buf: &mut Buffer, info: &PanelInfo<'_>, theme: &Theme) {
        let state = vec![
            Segment::new("Replay ", theme.accent_assistant),
            Segment::new(info.state, theme.text_primary),
            Segment::new(format!(" · {}% ", info.progress), theme.gray),
        ];
        if area.height == 1 {
            let controls = self.compact_end_controls(area.width as usize, info, theme);
            let reserved = controls
                .iter()
                .map(|s| UnicodeWidthStr::width(s.text.as_str()) as u16)
                .sum::<u16>();
            let state_area = Rect {
                width: area.width.saturating_sub(reserved),
                ..area
            };
            self.draw_line(state_area, buf, 0, state);
            self.draw_line(
                Rect::new(state_area.right(), area.y, reserved, 1),
                buf,
                0,
                controls,
            );
        } else {
            self.draw_line(area, buf, 0, state);
            let controls_area = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
            self.draw_controls(controls_area, buf, info, theme);
        }
    }

    fn compact_end_controls(
        &self,
        width: usize,
        _info: &PanelInfo<'_>,
        theme: &Theme,
    ) -> Vec<Segment> {
        if width >= 12 {
            vec![
                Segment::new(" [?]", theme.gray).action(PanelAction::Help),
                Segment::new(" [Esc]", theme.text_primary).action(PanelAction::Back),
            ]
        } else if width >= 7 {
            vec![
                Segment::new(" [?]", theme.gray).action(PanelAction::Help),
                Segment::new(" Esc", theme.text_primary).action(PanelAction::Back),
            ]
        } else {
            Vec::new()
        }
    }

    fn draw_box_line(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        row: u16,
        left: char,
        right: char,
        content_area: Rect,
        segments: Vec<Segment>,
        theme: &Theme,
    ) {
        let y = area.y.saturating_add(row);
        let border_style = Style::default().fg(theme.gray).bg(theme.bg_base);
        buf.set_string(area.x, y, left.to_string(), border_style);
        let consumed = self.draw_line(content_area, buf, 0, segments);
        let right_x = area.right().saturating_sub(1);
        for x in consumed..right_x {
            buf.set_string(x, y, "─", border_style);
        }
        buf.set_string(right_x, y, right.to_string(), border_style);
    }

    fn draw_line(&mut self, area: Rect, buf: &mut Buffer, row: u16, segments: Vec<Segment>) -> u16 {
        if row >= area.height || area.width == 0 {
            return area.x;
        }
        let y = area.y.saturating_add(row);
        let mut x = area.x;
        let right = area.x.saturating_add(area.width);
        for segment in segments {
            if x >= right {
                break;
            }
            let remaining = (right - x) as usize;
            let text_width = UnicodeWidthStr::width(segment.text.as_str());
            if segment.action.is_some() && text_width > remaining {
                break;
            }
            let visible = clip_width(&segment.text, remaining);
            let width = UnicodeWidthStr::width(visible.as_str()) as u16;
            if width == 0 {
                continue;
            }
            let mut style = Style::default()
                .fg(segment.color)
                .bg(Theme::current().bg_base);
            if segment.action.is_some() {
                style = style.add_modifier(Modifier::BOLD);
            }
            buf.set_stringn(x, y, &visible, width as usize, style);
            if let Some(action) = segment.action {
                self.hits.push((Rect::new(x, y, width, 1), action));
            }
            x = x.saturating_add(width);
        }
        x
    }
}

fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".to_owned();
    }
    let seconds = seconds.floor() as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn speed(value: f64) -> String {
    if value >= 1_000_000.0 || value < 0.01 {
        return format!("{value:.1e}");
    }
    let formatted = format!("{value:.2}");
    formatted
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn clip_width(text: &str, max_width: usize) -> String {
    let total = UnicodeWidthStr::width(text);
    if total <= max_width {
        return text.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    let ellipsis = if max_width > 1 { "…" } else { "" };
    let content_width = max_width.saturating_sub(UnicodeWidthStr::width(ellipsis));
    let mut result = String::new();
    let mut width = 0;
    for grapheme in text.graphemes(true) {
        let segment_width = UnicodeWidthStr::width(grapheme);
        if width + segment_width > content_width {
            break;
        }
        result.push_str(grapheme);
        width += segment_width;
    }
    result.push_str(ellipsis);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>() -> PanelInfo<'a> {
        PanelInfo {
            state: "Paused",
            speed: 4.0,
            progress: 38,
            time: "2026-09-29 11:49:55 UTC+08:00",
            play_current: 9.0,
            play_total: 26.0,
            notice: None,
            has_back_layer: true,
            follow: true,
            next: true,
        }
    }

    fn row(buffer: &Buffer, y: u16, width: u16) -> String {
        let mut text = String::new();
        let mut x = 0;
        while x < width {
            let symbol = buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" ");
            text.push_str(symbol);
            x += (UnicodeWidthStr::width(symbol) as u16).max(1);
        }
        text
    }

    #[test]
    fn panel_height_is_three_rows_with_safe_small_terminal_fallback() {
        assert_eq!(Panel::height(Rect::new(0, 0, 120, 40)), 3);
        assert_eq!(Panel::height(Rect::new(0, 0, 80, 2)), 2);
        assert_eq!(Panel::height(Rect::new(0, 0, 60, 1)), 1);
        assert_eq!(Panel::height(Rect::new(0, 0, 60, 0)), 0);
        assert_eq!(Panel::height(Rect::new(0, 0, 0, 20)), 0);
    }

    #[test]
    fn framed_panel_shows_status_history_and_complete_control_targets() {
        let mut panel = Panel::default();
        let area = Rect::new(0, 0, 120, 3);
        let mut buffer = Buffer::empty(area);
        panel.render(area, &mut buffer, &info());

        let top = row(&buffer, 0, area.width);
        let controls = row(&buffer, 1, area.width);
        let bottom = row(&buffer, 2, area.width);
        assert!(top.starts_with('╭') && top.ends_with('╮'));
        assert!(top.contains("Replay · Paused · 4× · 38%"));
        assert!(controls.starts_with('│') && controls.ends_with('│'));
        assert!(controls.contains("F8 继续"));
        assert!(controls.contains("[ ] 下一条"));
        assert!(controls.contains("End 跟随"));
        assert!(controls.contains("[?] 帮助") && controls.contains("[Esc] 返回"));
        assert!(bottom.starts_with('╰') && bottom.ends_with('╯'));
        assert!(bottom.contains("历史 2026-09-29 11:49:55 UTC+08:00"));
        assert!(bottom.contains("回放 00:09/00:26"));
        for omitted in ["根 agent", "Goal paused", "full shortcut", "原时长"] {
            assert!(
                !buffer
                    .content()
                    .iter()
                    .any(|cell| cell.symbol().contains(omitted))
            );
        }

        for action in [
            PanelAction::Toggle,
            PanelAction::SpeedDown,
            PanelAction::SpeedUp,
            PanelAction::Next,
            PanelAction::Follow,
            PanelAction::Help,
            PanelAction::Back,
        ] {
            assert!(
                panel.hits.iter().any(|(_, target)| *target == action),
                "missing {action:?}"
            );
        }
        assert!(panel.hit(0, 1).is_none());
    }

    #[test]
    fn notice_replaces_historical_time_but_keeps_playback_elapsed() {
        let mut panel = Panel::default();
        let mut panel_info = info();
        panel_info.notice = Some("暂停中 · 正在等待输入");
        let area = Rect::new(0, 0, 100, 3);
        let mut buffer = Buffer::empty(area);
        panel.render(area, &mut buffer, &panel_info);
        let bottom = row(&buffer, 2, area.width);
        assert!(bottom.contains("暂停中 · 正在等待输入"));
        assert!(!bottom.contains(panel_info.time));
        assert!(bottom.contains("回放 00:09/00:26"));
    }

    #[test]
    fn compact_and_tiny_layouts_only_hit_complete_visible_labels() {
        let _theme_guard = crate::theme::cache::pin_theme();
        let mut panel = Panel::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 3));
        panel.render(Rect::new(0, 0, 40, 3), &mut buffer, &info());
        assert_eq!(panel.hit(1, 1), Some(PanelAction::Toggle));
        assert!(panel.hit(39, 1).is_none());
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Back)
        );
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Help)
        );

        let narrow = Rect::new(0, 0, 12, 3);
        let mut narrow_buffer = Buffer::empty(narrow);
        panel.render(narrow, &mut narrow_buffer, &info());
        assert_eq!(panel.hit(1, 1), Some(PanelAction::Help));
        assert!(panel.hits.iter().any(|(rect, action)| {
            *action == PanelAction::Back && rect.width == UnicodeWidthStr::width("Esc") as u16
        }));
        assert!(panel.hit(12, 1).is_none());

        let one_row = Rect::new(0, 0, 80, 1);
        let mut one_row_buffer = Buffer::empty(one_row);
        panel.render(one_row, &mut one_row_buffer, &info());
        assert!(row(&one_row_buffer, 0, one_row.width).contains("Replay Paused"));
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Help)
        );
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Back)
        );
    }

    #[test]
    fn finished_state_has_no_execution_targets_and_resize_clears_old_hits() {
        let mut panel = Panel::default();
        let mut finished = info();
        finished.state = "Finished";
        finished.next = false;
        let area = Rect::new(0, 0, 100, 3);
        let mut buffer = Buffer::empty(area);
        panel.render(area, &mut buffer, &finished);
        assert!(row(&buffer, 1, area.width).contains("已结束"));
        assert!(!row(&buffer, 1, area.width).contains("继续"));
        assert!(!panel.hits.iter().any(|(_, action)| matches!(
            action,
            PanelAction::Toggle | PanelAction::SpeedDown | PanelAction::SpeedUp | PanelAction::Next
        )));
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Follow)
        );
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Help)
        );
        assert!(
            panel
                .hits
                .iter()
                .any(|(_, action)| *action == PanelAction::Back)
        );

        panel.render(Rect::new(0, 0, 6, 3), &mut buffer, &finished);
        assert!(panel.hits.is_empty(), "resize must clear old hot areas");
    }

    #[test]
    fn dark_and_light_themes_render_framed_controls() {
        let _theme_guard = crate::theme::cache::pin_theme();
        for kind in [
            crate::theme::ThemeKind::GrowNight,
            crate::theme::ThemeKind::GrowDay,
        ] {
            crate::theme::cache::set(kind);
            for area in [Rect::new(0, 0, 120, 3), Rect::new(0, 0, 40, 3)] {
                let mut buffer = Buffer::empty(area);
                let mut panel = Panel::default();
                panel.render(area, &mut buffer, &info());
                assert_eq!(
                    buffer.cell((0, area.y)).expect("panel cell").bg,
                    Theme::current().bg_base
                );
                assert_eq!(buffer.cell((0, area.y)).unwrap().symbol(), "╭");
                assert!((0..area.width).any(|x| {
                    (area.y..area.bottom()).any(|y| panel.hit(x, y) == Some(PanelAction::Help))
                }));
            }
        }
        crate::theme::cache::set(crate::theme::ThemeKind::GrowNight);
    }
}
