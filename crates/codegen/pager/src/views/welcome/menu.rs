//! Menu component — renders shortcut key menus.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::theme::Theme;

/// Row width cap (cols), matching the hero's stacked text-group width: on a
/// wide terminal the side-by-side hero text column spans nearly the whole
/// content area, and an uncapped menu row would push the flush-right
/// shortcuts to the screen edge.
const MENU_MAX_WIDTH: u16 = 51;

/// Right inset (cols) between the shortcut and the row's right edge, so keys
/// never sit flush against the row / content edge (the ~6-col margin of the
/// narrow text-only mode is the reference).
const KEY_RIGHT_PAD: u16 = 4;

/// Render the welcome menu rows as `label … shortcut`, padded within each row.
/// Returns the Rect for each item row (for hit-testing clicks and hover).
///
/// The row width is `max(content width, 30, min_width_hint)` clamped to
/// [`MENU_MAX_WIDTH`] and to `area`. When the hero passes its text-column
/// width as `min_width_hint`, the row is left-aligned with `area` so the
/// labels stay flush with the version/subtitle above, and the shortcut keeps
/// a [`KEY_RIGHT_PAD`] right inset so keys never reach the screen edge. With
/// `min_width_hint == 0` (trust gate) the row stays compact
/// and centered within `area`.
pub fn render_menu(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    items: &[(&str, &str)],
    selected: Option<usize>,
    mouse_pos: Option<(u16, u16)>,
    min_width_hint: u16,
) -> Vec<Rect> {
    let label_style = Style::default()
        .fg(theme.text_primary)
        .add_modifier(Modifier::BOLD);
    let label_selected_style = Style::default()
        .fg(theme.text_primary)
        .bg(theme.bg_highlight)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme.gray_bright);
    let key_selected_style = Style::default()
        .fg(theme.gray_bright)
        .bg(theme.bg_highlight);

    // Width: label + gap + key. Keep a 4-col gap between label and key for
    // readability.
    let content_min: u16 = items
        .iter()
        .map(|(key, label)| (key.len() + label.len() + 4) as u16)
        .max()
        .unwrap_or(0);
    let menu_width = content_min
        .max(30)
        .max(min_width_hint)
        .min(MENU_MAX_WIDTH)
        .min(area.width);

    let menu_centered = if min_width_hint > 0 {
        // Hero: left-align with the text column above (version / subtitle),
        // so the labels stay flush with the text above the menu.
        Rect {
            x: area.x,
            y: area.y,
            width: menu_width,
            height: area.height,
        }
    } else {
        let [_, menu_centered, _] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(menu_width),
            Constraint::Min(0),
        ])
        .flex(Flex::Center)
        .areas(area);
        menu_centered
    };

    let mut rects = Vec::with_capacity(items.len());
    let mut y = menu_centered.y;
    for (i, (key, label)) in items.iter().enumerate() {
        if y >= menu_centered.y + menu_centered.height {
            break;
        }

        let is_selected = selected == Some(i);
        let key_width = key.len() as u16;
        let label_len = label.len() as u16;

        let row_rect = Rect {
            x: menu_centered.x,
            y,
            width: menu_centered.width,
            height: 1,
        };
        rects.push(row_rect);

        // Fill row background when selected/hovered
        if is_selected {
            let hover_bg = Style::default().bg(theme.bg_highlight);
            for x in menu_centered.x..menu_centered.x + menu_centered.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(hover_bg);
                }
            }
        }

        // Label, flush with the left edge of the menu column.
        let lstyle = if is_selected {
            label_selected_style
        } else {
            label_style
        };
        buf.set_span(menu_centered.x, y, &Span::styled(*label, lstyle), label_len);

        // Key shortcut flush with the right edge of the menu column, minus
        // the right pad so keys never hug the row / content edge.
        let kstyle = if is_selected {
            key_selected_style
        } else {
            key_style
        };
        let key_x_start = menu_centered.x
            + menu_centered
                .width
                .saturating_sub(key_width + KEY_RIGHT_PAD);
        buf.set_span(key_x_start, y, &Span::styled(*key, kstyle), key_width);

        // [x] dismiss affordance restyling (for the import row)
        if let Some(x_offset) = key.rfind("[x]") {
            let dismiss_start = key_x_start + x_offset as u16;
            let dismiss_end = dismiss_start + 3;
            let mouse_on_dismiss = mouse_pos
                .is_some_and(|(mx, my)| my == y && mx >= dismiss_start && mx < dismiss_end);
            let dismiss_color = if mouse_on_dismiss {
                theme.text_primary
            } else {
                theme.gray_bright
            };
            let dismiss_style = if is_selected {
                Style::default()
                    .fg(dismiss_color)
                    .bg(theme.bg_highlight)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(dismiss_color)
                    .add_modifier(Modifier::BOLD)
            };
            for (offset, ch) in "[x]".chars().enumerate() {
                let col = dismiss_start + offset as u16;
                if let Some(cell) = buf.cell_mut((col, y)) {
                    cell.set_char(ch);
                    cell.set_style(dismiss_style);
                }
            }
        }

        y += 1;
    }

    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_width_is_clamped_by_max_width() {
        // A huge text-column hint must cap the row at MENU_MAX_WIDTH (the
        // hero's stacked text-group width) instead of spanning the area.
        let area = Rect::new(0, 0, 300, 4);
        let mut buf = Buffer::empty(area);
        let items = [("ctrl+w", "New worktree")];
        let rects = render_menu(area, &mut buf, &Theme::current(), &items, None, None, 200);
        assert_eq!(rects[0].width, MENU_MAX_WIDTH);
        assert_eq!(rects[0].x, area.x, "hinted rows left-align");
    }

    #[test]
    fn menu_aligns_left_with_hint_and_centers_without() {
        let items = [("l", "Login")];
        let area = Rect::new(10, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        let rects = render_menu(area, &mut buf, &Theme::current(), &items, None, None, 60);
        assert_eq!(
            rects[0].x, area.x,
            "hero rows must left-align with the text column"
        );
        let rects = render_menu(area, &mut buf, &Theme::current(), &items, None, None, 0);
        assert!(
            rects[0].x > area.x,
            "gate rows must stay centered when min_width_hint == 0"
        );
    }

    #[test]
    fn shortcut_keeps_right_pad_from_row_edge() {
        let items = [("ctrl+w", "New worktree")];
        let area = Rect::new(0, 0, 60, 2);
        let mut buf = Buffer::empty(area);
        let rects = render_menu(area, &mut buf, &Theme::current(), &items, None, None, 60);
        let row = rects[0];
        let row_right = row.x + row.width - 1;
        // The key's last cell must sit at least KEY_RIGHT_PAD cols from the
        // row's right edge, and the pad cells must be blank. (The label
        // "New worktree" also contains 'w', so take the LAST match.)
        let mut key_end = None;
        for x in row.x..=row_right {
            if buf[(x, row.y)].symbol() == "w" {
                key_end = Some(x);
            }
        }
        let key_end = key_end.expect("the ctrl+w key must render");
        assert!(
            row_right - key_end >= KEY_RIGHT_PAD,
            "key col {key_end} must be >= {KEY_RIGHT_PAD} cols from row edge {row_right}"
        );
        for x in key_end + 1..=row_right {
            assert_eq!(buf[(x, row.y)].symbol(), " ", "pad cell {x} must be blank");
        }
    }

    #[test]
    fn narrow_area_clamps_width_without_panicking() {
        // area.width < content_min: the row clamps to the area and the key
        // position math must saturate instead of underflowing.
        let items = [("ctrl+w", "New worktree")]; // content_min = 22
        for w in [20u16, 5u16] {
            let area = Rect::new(0, 0, w, 2);
            let mut buf = Buffer::empty(area);
            let rects = render_menu(area, &mut buf, &Theme::current(), &items, None, None, 0);
            assert!(rects[0].width <= w, "row must clamp to the area width");
            assert!(
                rects[0].width <= MENU_MAX_WIDTH,
                "row must never exceed MENU_MAX_WIDTH"
            );
        }
    }
}
