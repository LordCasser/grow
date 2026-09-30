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
    pub path: &'a str,
    pub time: &'a str,
    pub play_current: f64,
    pub play_total: f64,
    pub original_duration: Option<u64>,
    pub historical: &'a str,
    pub notice: Option<&'a str>,
    pub hint: &'a str,
    pub has_parent: bool,
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
    atomic: bool,
}

impl Segment {
    fn new(text: impl Into<String>, color: ratatui::style::Color) -> Self {
        Self {
            text: text.into(),
            color,
            action: None,
            atomic: false,
        }
    }

    fn atomic(mut self) -> Self {
        self.atomic = true;
        self
    }

    fn action(mut self, action: PanelAction) -> Self {
        self.action = Some(action);
        self
    }
}

impl Panel {
    pub(super) fn height(area: Rect) -> u16 {
        if area.width == 0 || area.height == 0 {
            return 0;
        }
        let by_width = if area.width >= 100 {
            5
        } else if area.width >= 72 {
            4
        } else {
            3
        };
        let by_height = match area.height {
            0 => 0,
            1..=5 => 1,
            6..=9 => 2,
            10..=15 => 3,
            16..=23 => 4,
            _ => 5,
        };
        let height = by_width.min(by_height);
        if height == 1 {
            1
        } else {
            height.min(area.height.saturating_sub(1))
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

        if height == 1 {
            self.draw_escape_help(area, buf, 0, &theme);
            return;
        }

        if height >= 4 {
            let playback = format!(" · {}× · {}% · ", speed(info.speed), info.progress);
            let readonly = if area.width >= 100 {
                "只读 · 模拟流式 · "
            } else {
                ""
            };
            let path_budget = (area.width as usize).saturating_sub(
                UnicodeWidthStr::width("Replay · ")
                    + UnicodeWidthStr::width(info.state)
                    + UnicodeWidthStr::width(playback.as_str())
                    + UnicodeWidthStr::width(readonly),
            );
            self.draw_line(
                area,
                buf,
                0,
                vec![
                    Segment::new("Replay · ", theme.accent_assistant),
                    Segment::new(info.state, theme.text_primary),
                    Segment::new(playback, theme.gray),
                    Segment::new(readonly, theme.gray),
                    Segment::new(clip_path(info.path, path_budget), theme.text_secondary),
                ],
            );
            self.draw_line(
                area,
                buf,
                if height >= 5 { 2 } else { 1 },
                vec![
                    Segment::new(format!("历史 {}", info.time), theme.text_secondary).atomic(),
                    Segment::new(
                        format!(
                            " · 回放 {}/{}",
                            clock(info.play_current),
                            clock(info.play_total)
                        ),
                        theme.gray,
                    ),
                    duration_segment(info.original_duration, theme.gray),
                ],
            );
            self.draw_line(
                area,
                buf,
                if height >= 5 { 3 } else { 2 },
                vec![Segment::new(
                    info.notice.unwrap_or(info.historical),
                    if info.notice.is_some() {
                        theme.warning
                    } else {
                        theme.gray
                    },
                )],
            );
            if height >= 5 {
                self.draw_controls(area, buf, 1, info, &theme);
                self.draw_line(
                    area,
                    buf,
                    4,
                    vec![
                        Segment::new("? 帮助  ", theme.gray).action(PanelAction::Help),
                        Segment::new(info.hint, theme.text_secondary),
                    ],
                );
            } else {
                self.draw_controls(area, buf, 3, info, &theme);
            }
        } else if height == 3 {
            self.draw_line(
                area,
                buf,
                0,
                vec![
                    Segment::new("Replay · ", theme.accent_assistant),
                    Segment::new(info.state, theme.text_primary),
                    Segment::new(
                        format!(" · {}× · {}%", speed(info.speed), info.progress),
                        theme.gray,
                    ),
                ],
            );
            let second = info.notice.unwrap_or_else(|| {
                if area.width >= 40 {
                    info.time
                } else {
                    info.historical
                }
            });
            self.draw_line(
                area,
                buf,
                1,
                vec![Segment::new(
                    second,
                    if info.notice.is_some() {
                        theme.warning
                    } else {
                        theme.text_secondary
                    },
                )],
            );
            if area.width < 40 {
                self.draw_escape_help(area, buf, 2, &theme);
            } else {
                self.draw_controls(area, buf, 2, info, &theme);
            }
        } else {
            self.draw_line(
                area,
                buf,
                0,
                vec![
                    Segment::new("Replay · ", theme.accent_assistant),
                    Segment::new(info.state, theme.text_primary),
                    Segment::new(format!(" · {}%", info.progress), theme.gray),
                ],
            );
            self.draw_escape_help(area, buf, 1, &theme);
        }
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

    fn draw_escape_help(&mut self, area: Rect, buf: &mut Buffer, row: u16, theme: &Theme) {
        let (back, help) = if area.width < 20 {
            ("Esc", " ?")
        } else {
            ("Esc 退出", "   ? 帮助")
        };
        self.draw_line(
            area,
            buf,
            row,
            vec![
                Segment::new(back, theme.text_primary).action(PanelAction::Back),
                Segment::new(help, theme.gray).action(PanelAction::Help),
            ],
        );
    }

    fn draw_controls(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        row: u16,
        info: &PanelInfo<'_>,
        theme: &Theme,
    ) {
        let toggle = if info.state.eq_ignore_ascii_case("playing") {
            "F8 暂停"
        } else {
            "F8 继续"
        };
        if area.width < 72 {
            let toggle_segment = if info.state.eq_ignore_ascii_case("finished") {
                Segment::new(toggle, theme.gray)
            } else {
                Segment::new(toggle, theme.text_primary).action(PanelAction::Toggle)
            };
            let mut compact = vec![
                toggle_segment,
                Segment::new(
                    if info.has_parent {
                        " Esc 返回"
                    } else {
                        " Esc 退出"
                    },
                    theme.text_secondary,
                )
                .action(PanelAction::Back),
                Segment::new(" ? 帮助", theme.gray).action(PanelAction::Help),
            ];
            if !info.state.eq_ignore_ascii_case("finished") {
                compact
                    .push(Segment::new(" -", theme.text_secondary).action(PanelAction::SpeedDown));
                compact.push(Segment::new(
                    format!("{}×", speed(info.speed)),
                    theme.accent_assistant,
                ));
                compact.push(Segment::new("+", theme.text_secondary).action(PanelAction::SpeedUp));
            }
            if info.next {
                compact.push(Segment::new(" ]", theme.text_secondary).action(PanelAction::Next));
            }
            if info.follow {
                compact
                    .push(Segment::new(" 跟随✓", theme.text_secondary).action(PanelAction::Follow));
            } else {
                compact
                    .push(Segment::new(" End", theme.text_secondary).action(PanelAction::Follow));
            }
            self.draw_line(area, buf, row, compact);
            return;
        }
        let toggle_segment = if info.state.eq_ignore_ascii_case("finished") {
            Segment::new(toggle, theme.gray)
        } else {
            Segment::new(toggle, theme.text_primary).action(PanelAction::Toggle)
        };
        let mut segments = vec![
            toggle_segment,
            Segment::new(
                if info.has_parent {
                    "  Esc 返回"
                } else {
                    "  Esc 退出"
                },
                theme.text_secondary,
            )
            .action(PanelAction::Back),
            Segment::new("  ? 帮助", theme.gray).action(PanelAction::Help),
            Segment::new(
                if info.follow {
                    "  跟随✓"
                } else {
                    "  End 跟随"
                },
                theme.text_secondary,
            )
            .action(PanelAction::Follow),
        ];
        if !info.state.eq_ignore_ascii_case("finished") {
            segments
                .push(Segment::new("  - ", theme.text_secondary).action(PanelAction::SpeedDown));
            segments.push(Segment::new(
                format!("{}×", speed(info.speed)),
                theme.accent_assistant,
            ));
            segments.push(Segment::new(" +", theme.text_secondary).action(PanelAction::SpeedUp));
        }
        if info.next {
            segments
                .push(Segment::new("  ] 下一记录", theme.text_secondary).action(PanelAction::Next));
        }
        self.draw_line(area, buf, row, segments);
    }

    fn draw_line(&mut self, area: Rect, buf: &mut Buffer, row: u16, segments: Vec<Segment>) {
        if row >= area.height || area.width == 0 {
            return;
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
            if (segment.atomic || segment.action.is_some()) && text_width > remaining {
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
    }
}

fn duration_segment(duration: Option<u64>, color: ratatui::style::Color) -> Segment {
    match duration {
        Some(seconds) => Segment::new(format!(" · 原时长 {}", clock(seconds as f64)), color),
        None => Segment::new(" · 原时长 未知/估算", color),
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

fn clip_path(path: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(path) <= max_width {
        return path.to_owned();
    }
    let parts: Vec<_> = path.split(" › ").collect();
    if parts.len() < 2 || max_width < 10 {
        return clip_width(path, max_width);
    }
    let separator = " › … › ";
    let available = max_width.saturating_sub(UnicodeWidthStr::width(separator));
    let head = clip_tail(parts[0], available / 2);
    let tail = clip_tail(
        parts[parts.len() - 1],
        available - UnicodeWidthStr::width(head.as_str()),
    );
    format!("{head}{separator}{tail}")
}

fn clip_tail(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut tail = String::new();
    let mut width = 0;
    for grapheme in text.graphemes(true).rev() {
        let segment_width = UnicodeWidthStr::width(grapheme);
        if width + segment_width > max_width.saturating_sub(1) {
            break;
        }
        tail.insert_str(0, grapheme);
        width += segment_width;
    }
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info<'a>(path: &'a str, hint: &'a str) -> PanelInfo<'a> {
        PanelInfo {
            state: "Paused",
            speed: 4.0,
            progress: 38,
            path,
            time: "2026-09-29 11:49:55 UTC+08:00",
            play_current: 9.0,
            play_total: 26.0,
            original_duration: Some(7920),
            historical: "Goal paused",
            notice: None,
            hint,
            has_parent: true,
            follow: true,
            next: true,
        }
    }

    #[test]
    fn panel_height_matches_responsive_size_classes() {
        assert_eq!(Panel::height(Rect::new(0, 0, 120, 40)), 5);
        assert_eq!(Panel::height(Rect::new(0, 0, 80, 24)), 4);
        assert_eq!(Panel::height(Rect::new(0, 0, 60, 15)), 3);
        assert_eq!(Panel::height(Rect::new(0, 0, 60, 8)), 2);
        assert_eq!(Panel::height(Rect::new(0, 0, 60, 4)), 1);
        assert_eq!(Panel::height(Rect::new(0, 0, 60, 0)), 0);
        assert_eq!(Panel::height(Rect::new(0, 0, 0, 20)), 0);
    }

    #[test]
    fn drawing_exposes_only_visible_click_targets_and_clears_them_on_resize() {
        let mut panel = Panel::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
        let info = info(
            "根 agent › discovery-2",
            "Enter 详情  ] 下一记录  Esc 返回  ? 帮助",
        );
        panel.render(Rect::new(0, 20, 80, 4), &mut buffer, &info);
        assert_eq!(panel.hit(1, 23), Some(PanelAction::Toggle));
        assert!(panel.hit(1, 22).is_none());
        assert!(buffer.content().iter().any(|cell| cell.symbol() == "根"));

        let short = Rect::new(0, 22, 20, 2);
        panel.render(short, &mut buffer, &info);
        assert_eq!(panel.hit(1, 23), Some(PanelAction::Back));
        assert_eq!(panel.hit(5, 23), Some(PanelAction::Back));
        assert!(panel.hit(25, 23).is_none());
    }

    #[test]
    fn clipped_fields_use_display_width_and_keep_dates_atomic_by_priority() {
        assert_eq!(clip_width("中文路径abc", 5), "中文…");
        assert_eq!(clip_width("abc", 2), "a…");
        assert_eq!(clip_width("abc", 1), "a");
        let path = clip_path("很长的根节点 › 中间节点 › 很长的当前子代理", 18);
        assert!(path.contains('点') && path.contains('理'));
        assert!(UnicodeWidthStr::width(path.as_str()) <= 18);
    }

    #[test]
    fn dark_and_light_themes_render_controls_at_standard_and_compact_sizes() {
        let _theme_guard = crate::theme::cache::pin_theme();
        for kind in [
            crate::theme::ThemeKind::GrowNight,
            crate::theme::ThemeKind::GrowDay,
        ] {
            crate::theme::cache::set(kind);
            for outer in [Rect::new(0, 0, 120, 40), Rect::new(0, 0, 40, 8)] {
                let height = Panel::height(outer);
                let area = Rect::new(0, outer.height - height, outer.width, height);
                let mut buffer = Buffer::empty(outer);
                let mut panel = Panel::default();
                panel.render(area, &mut buffer, &info("根 › 子代理", "Enter 详情"));
                assert_eq!(
                    buffer.cell((0, area.y)).expect("panel cell").bg,
                    Theme::current().bg_base
                );
                assert!((0..outer.width).any(|x| {
                    (area.y..area.bottom()).any(|y| panel.hit(x, y) == Some(PanelAction::Help))
                }));
            }
        }
        crate::theme::cache::set(crate::theme::ThemeKind::GrowNight);
    }
}
