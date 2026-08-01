//! Hero component — the borderless welcome content: logo + text group.
//!
//! Replaces the old bordered hero box. The component is **borderless** (no
//! `Block`/`Borders` anywhere) and separates layout computation
//! ([`compute_hero`]) from rendering ([`render_hero`]).
//!
//! Layout modes (picked by content-area width **and** height, calibrated to
//! the measured assets — big logo 80×35, small logo 30×15):
//!
//! - `w ≥ 143 && h ≥ 39` → side-by-side with the **big** logo.
//! - `w ≥ 93 && h ≥ 19` → side-by-side with the **small** logo.
//! - `w ≥ 34 && h ≥ 19` → stacked (logo above the text group).
//! - smaller → text-only (no logo — a logo that can't fit or would deform
//!   the content is never shown).
//!
//! Side-by-side: the left column is `max(50% of the content width, logo width
//! + padding)`, the logo centered horizontally and vertically inside it; the
//! right column holds the vertically-centered text group: version badge →
//! subtitle → info slot (announcement) → menu. Stacked: logo on top, text
//! group below, the whole block vertically centered.
//!
//! The component is parameterized for reuse by the agent empty-state (Task B):
//! `with_menu`, `with_info` and `interactive` (hover styling) can be toggled
//! off; the home page enables all of them. The logo render functions stay
//! standalone in [`super::logo`] so Task B can render a bare centered logo.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::theme::Theme;

use super::logo::{self, LogoSize};

/// Maximum width (cols) of the centered text group in stacked / text-only mode.
/// Matches the old stacked `MENU_MIN_WIDTH` so the menu row layout is stable.
const STACKED_TEXT_WIDTH: u16 = 51;

/// Rows the promo announcement CTA reserves in the info slot: a spacer row above the
/// `[label]` button row. Reserved on top of the announcement text rows so the
/// message never paints over the button.
const ANNOUNCEMENT_CTA_ROWS: u16 = 2;

const HERO_SUBTITLE: &str = "Thanks for trying Grow, give feedback with /feedback!";

/// Which logo the hero shows (both tiers are the same art family).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum HeroMode {
    /// Logo left, text group right.
    SideBySide(LogoSize),
    /// Logo above the text group.
    Stacked(LogoSize),
    /// No logo — the area is too small to fit one without deforming content.
    TextOnly,
}

/// Inputs to [`compute_hero`].
#[derive(Default)]
pub(super) struct HeroLayoutInput<'a> {
    pub(super) content_area: Rect,
    /// Error/warning row height; 0 when there's nothing to show.
    pub(super) error_height: u16,
    pub(super) menu_height: u16,
    pub(super) tip_height: u16,
    pub(super) announcement: Option<&'a announcements::Announcement>,
    /// Whether a long announcement is expanded inline (vs. collapsed to 2 lines).
    pub(super) expanded: bool,
    /// Whether the info slot reserves a promo announcement CTA (spacer + button).
    pub(super) has_promo_cta: bool,
    /// Hide the menu (agent empty-state reuse; the home page keeps it).
    pub(super) with_menu: bool,
    /// Hide the announcement info slot (agent empty-state reuse; home keeps it).
    pub(super) with_info: bool,
}

/// Computed areas for the borderless hero.
pub(super) struct HeroLayout {
    pub(super) mode: HeroMode,
    /// The whole hero block (logo + text group), vertically centered.
    ///
    /// `dead_code` (lib target): read by the geometry tests; the renderer
    /// only consumes the per-section rects below. Task B's agent empty-state
    /// layout may use the block rect directly.
    #[allow(dead_code)]
    pub(super) hero: Rect,
    /// Logo area (zero when the mode is [`HeroMode::TextOnly`]).
    pub(super) logo: Rect,
    /// Text group column (version … menu).
    ///
    /// `dead_code` (lib target): read by the geometry tests; the per-section
    /// rects below are all derived from this column.
    #[allow(dead_code)]
    pub(super) text: Rect,
    pub(super) version: Rect,
    /// "Thanks for trying Grow" line — hidden when the info slot is shown.
    pub(super) subtitle: Rect,
    /// Info slot (announcement), zero when no announcement fits.
    pub(super) info: Rect,
    pub(super) menu: Rect,
    /// Fixed tip row at the bottom of the content area (zero without a tip).
    pub(super) tip: Rect,
    /// Error/warning row above the hero (zero without one).
    pub(super) error: Rect,
}

/// Fixed rows below the hero: the tip + its gap (no bottom version row on the
/// home page — the badge moved into the hero's text group).
pub(super) fn fixed_below(tip_height: u16) -> u16 {
    let tip_gap = if tip_height > 0 { 1u16 } else { 0 };
    tip_height + tip_gap
}

/// Side-by-side gate for the big logo: logo width + padding + minimum text
/// column (143 cols) and logo height + vertical padding (39 rows).
fn big_gate() -> (u16, u16) {
    (
        logo::visual_width(logo::LOGO) + 2 * logo::H_PAD + logo::RIGHT_COL_MIN,
        logo::count_lines(logo::LOGO) + 2 * logo::V_PAD,
    )
}

/// Side-by-side gate for the small logo: 93 cols × 19 rows.
fn small_gate() -> (u16, u16) {
    (
        logo::visual_width(logo::LOGO_SMALL) + 2 * logo::H_PAD + logo::RIGHT_COL_MIN,
        logo::count_lines(logo::LOGO_SMALL) + 2 * logo::V_PAD,
    )
}

/// Stacked gate for the small logo: logo width + padding only (34 cols × 19 rows).
fn stacked_gate() -> (u16, u16) {
    (
        logo::visual_width(logo::LOGO_SMALL) + 2 * logo::H_PAD,
        logo::count_lines(logo::LOGO_SMALL) + 2 * logo::V_PAD,
    )
}

/// Width of the centered text group in stacked / text-only mode.
fn text_group_width(content_width: u16) -> u16 {
    content_width
        .saturating_sub(2 * logo::H_PAD)
        .min(STACKED_TEXT_WIDTH)
}

/// Text-group geometry for a mode: `(text_width, logo_height, base_block_height)`.
/// The base block height excludes the info slot (added by the caller once it
/// is clamped to fit).
fn mode_geometry(mode: HeroMode, content_width: u16, base_text_height: u16) -> (u16, u16, u16) {
    match mode {
        HeroMode::SideBySide(size) => {
            let left = (content_width / 2).max(size.width() + 2 * logo::H_PAD);
            (
                content_width - left,
                size.height(),
                (size.height() + 2 * logo::V_PAD).max(base_text_height + 2 * logo::V_PAD),
            )
        }
        HeroMode::Stacked(size) => (
            text_group_width(content_width),
            size.height(),
            size.height() + 1 + base_text_height,
        ),
        HeroMode::TextOnly => (text_group_width(content_width), 0, base_text_height),
    }
}

/// Height the info slot may reserve (announcement rows, clamped to fit).
fn info_slot_height(
    text_width: u16,
    base_block_height: u16,
    input: &HeroLayoutInput<'_>,
    gap_err: u16,
    fixed_below: u16,
) -> u16 {
    if !input.with_info {
        return 0;
    }
    let Some(ann) = input.announcement else {
        return 0;
    };
    let desired = announcement_desired_rows(ann, text_width, input.expanded, input.has_promo_cta);
    let slack = input
        .content_area
        .height
        .saturating_sub(gap_err + input.error_height + base_block_height + fixed_below);
    // The info slot needs a spacer row above it once it has content.
    if slack >= 1 {
        desired.min(slack - 1)
    } else {
        0
    }
}

/// Candidate modes in degradation order for the given content-area size.
fn candidates(w: u16, h: u16) -> &'static [HeroMode] {
    let (big_w, big_h) = big_gate();
    let (small_w, small_h) = small_gate();
    let (stacked_w, stacked_h) = stacked_gate();
    if w >= big_w && h >= big_h {
        &[
            HeroMode::SideBySide(LogoSize::Big),
            HeroMode::SideBySide(LogoSize::Small),
            HeroMode::Stacked(LogoSize::Small),
            HeroMode::TextOnly,
        ]
    } else if w >= small_w && h >= small_h {
        &[
            HeroMode::SideBySide(LogoSize::Small),
            HeroMode::Stacked(LogoSize::Small),
            HeroMode::TextOnly,
        ]
    } else if w >= stacked_w && h >= stacked_h {
        &[HeroMode::Stacked(LogoSize::Small), HeroMode::TextOnly]
    } else {
        &[HeroMode::TextOnly]
    }
}

/// Compute the hero layout. The mode resolves by trying the candidate tiers in
/// degradation order (big → small → stacked-small → text-only), clamping the
/// announcement slot to what actually fits — a logo is only shown when the
/// full block fits without deforming the content.
pub(super) fn compute_hero(input: HeroLayoutInput<'_>) -> HeroLayout {
    let zero = Rect::default();
    let content_area = input.content_area;
    let w = content_area.width;
    let h = content_area.height;
    let gap_err = if input.error_height > 0 { 1 } else { 0 };
    let fixed_below = fixed_below(input.tip_height);
    let menu_h = if input.with_menu {
        input.menu_height
    } else {
        0
    };
    // version(1) + subtitle(1) + gap-before-menu(1) + menu.
    let base_text_h = 1 + 1 + u16::from(menu_h > 0) + menu_h;

    // Resolve the mode + info slot: first candidate whose full block fits.
    let (mode, info_h) = candidates(w, h)
        .iter()
        .copied()
        .find_map(|mode| {
            let (text_w, _logo_h, base_h) = mode_geometry(mode, w, base_text_h);
            let info_h = info_slot_height(text_w, base_h, &input, gap_err, fixed_below);
            let block_h = base_h + if info_h > 0 { 1 + info_h } else { 0 };
            (h >= gap_err + input.error_height + block_h + fixed_below).then_some((mode, info_h))
        })
        .unwrap_or((HeroMode::TextOnly, 0));

    let (text_w, _logo_h, base_h) = mode_geometry(mode, w, base_text_h);
    let info_gap = if info_h > 0 { 1 } else { 0 };
    let subtitle_rows = if info_h > 0 { 0 } else { 1 };
    let menu_gap = if menu_h > 0 { 1 } else { 0 };
    let text_h = 1 + subtitle_rows + info_gap + info_h + menu_gap + menu_h;
    let block_h = base_h + info_gap + info_h;

    let slack = h.saturating_sub(gap_err + input.error_height + block_h + fixed_below);
    let top_pad = slack / 2;
    let tip_gap = if input.tip_height > 0 { 1 } else { 0 };
    let [_, _, error, hero_slot, _, tip, _] = Layout::vertical([
        Constraint::Length(top_pad),
        Constraint::Length(gap_err),
        Constraint::Length(input.error_height),
        Constraint::Length(block_h),
        Constraint::Min(0),
        Constraint::Length(input.tip_height),
        Constraint::Length(tip_gap),
    ])
    .areas(content_area);

    let (logo, text) = match mode {
        HeroMode::SideBySide(size) => {
            let left_w = (hero_slot.width / 2).max(size.width() + 2 * logo::H_PAD);
            let logo_rect = Rect {
                x: hero_slot.x + (left_w - size.width()) / 2,
                y: hero_slot.y + (block_h - size.height()) / 2,
                width: size.width(),
                height: size.height(),
            };
            let text_rect = Rect {
                x: hero_slot.x + left_w,
                y: hero_slot.y + (block_h - text_h) / 2,
                width: hero_slot.width - left_w,
                height: text_h,
            };
            (logo_rect, text_rect)
        }
        HeroMode::Stacked(size) => {
            let logo_rect = Rect {
                x: hero_slot.x + (hero_slot.width - size.width()) / 2,
                y: hero_slot.y,
                width: size.width(),
                height: size.height(),
            };
            let text_rect = Rect {
                x: hero_slot.x + (hero_slot.width - text_w) / 2,
                y: hero_slot.y + size.height() + 1,
                width: text_w,
                height: text_h,
            };
            (logo_rect, text_rect)
        }
        HeroMode::TextOnly => {
            let text_rect = Rect {
                x: hero_slot.x + (hero_slot.width - text_w) / 2,
                y: hero_slot.y + (block_h - text_h) / 2,
                width: text_w,
                height: text_h,
            };
            (zero, text_rect)
        }
    };

    let version = Rect {
        x: text.x,
        y: text.y,
        width: text.width,
        height: 1,
    };
    let subtitle = if subtitle_rows > 0 {
        Rect {
            x: text.x,
            y: text.y + 1,
            width: text.width,
            height: 1,
        }
    } else {
        zero
    };
    let info = if info_h > 0 {
        Rect {
            x: text.x,
            y: text.y + 1 + subtitle_rows + info_gap,
            width: text.width,
            height: info_h,
        }
    } else {
        zero
    };
    let menu = if menu_h > 0 {
        Rect {
            x: text.x,
            y: text.y + 1 + subtitle_rows + info_gap + info_h + menu_gap,
            width: text.width,
            height: menu_h,
        }
    } else {
        zero
    };

    HeroLayout {
        mode,
        hero: hero_slot,
        logo,
        text,
        version,
        subtitle,
        info,
        menu,
        tip,
        error,
    }
}

/// Hit-test rects produced by [`render_hero`].
pub(super) struct HeroRects {
    /// Hit-test rect per menu item row (for click/hover).
    pub(super) menu_rects: Vec<Rect>,
    /// Whether the announcement overflowed (the "expandable" signal).
    pub(super) announcement_truncated: bool,
    /// Full announcement block area (clickable anywhere to toggle), if shown.
    pub(super) announcement_rect: Option<Rect>,
    /// Promo announcement CTA `[label]` button rect (click → open), if drawn.
    pub(super) promo_cta_rect: Option<Rect>,
}

/// Render the borderless hero: logo (per the layout mode), then the text group
/// (version badge → subtitle → info slot → menu). No borders anywhere.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_hero(
    layout: &HeroLayout,
    buf: &mut Buffer,
    theme: &Theme,
    menu_items: &[(&str, &str)],
    selected: Option<usize>,
    mouse_pos: Option<(u16, u16)>,
    announcement: Option<&announcements::Announcement>,
    announcement_expanded: bool,
    promo_cta: Option<&str>,
    interactive: bool,
) -> HeroRects {
    let hover = interactive.then_some(mouse_pos).flatten();

    if layout.logo.height > 0 {
        let logo = match layout.mode {
            HeroMode::SideBySide(size) | HeroMode::Stacked(size) => size.art(),
            HeroMode::TextOnly => unreachable!("no logo rect in text-only mode"),
        };
        logo::render_logo_into(layout.logo, buf, theme, logo);
    }

    super::render_version_badge(
        layout.version,
        buf,
        theme,
        0,
        super::VersionBadgeMode::HeroInline,
    );

    if layout.subtitle.height > 0 {
        let subtitle_style = Style::default().fg(theme.gray);
        buf.set_span(
            layout.subtitle.x,
            layout.subtitle.y,
            &Span::styled(HERO_SUBTITLE, subtitle_style),
            layout.subtitle.width,
        );
    }

    let mut announcement_truncated = false;
    let mut announcement_rect = None;
    let mut promo_cta_rect = None;
    if layout.info.height > 0
        && let Some(ann) = announcement
    {
        let (text_area, truncated, cta_rect) = render_announcement_with_cta(
            buf,
            theme,
            layout.info,
            ann,
            announcement_expanded,
            hover,
            promo_cta,
        );
        announcement_rect = Some(text_area);
        announcement_truncated = truncated;
        promo_cta_rect = cta_rect;
    }

    let menu_rects = if layout.menu.height > 0 {
        super::menu::render_menu(
            layout.menu,
            buf,
            theme,
            menu_items,
            selected,
            hover,
            layout.menu.width,
        )
    } else {
        vec![]
    };

    HeroRects {
        menu_rects,
        announcement_truncated,
        announcement_rect,
        promo_cta_rect,
    }
}

/// Draw the announcement text + (optional) announcement CTA into `area`, reserving
/// the CTA rows at the bottom so a long/expanded message never overpaints the
/// button; the button is placed right after the drawn text + a spacer row.
/// Returns `(text_area, truncated, promo_cta_rect)`.
#[allow(clippy::too_many_arguments)]
fn render_announcement_with_cta(
    buf: &mut Buffer,
    theme: &Theme,
    area: Rect,
    ann: &announcements::Announcement,
    expanded: bool,
    mouse_pos: Option<(u16, u16)>,
    promo_cta: Option<&str>,
) -> (Rect, bool, Option<Rect>) {
    let cta_rows = if promo_cta.is_some() {
        ANNOUNCEMENT_CTA_ROWS
    } else {
        0
    };
    let text_area = Rect {
        height: area.height.saturating_sub(cta_rows),
        ..area
    };
    let truncated = render_announcement_block(buf, theme, text_area, ann, expanded, mouse_pos);
    let mut cta_rect = None;
    if let Some(label) = promo_cta {
        use unicode_width::UnicodeWidthStr;
        let text_rows =
            announcement_text_rows(ann, text_area.width, expanded).min(text_area.height);
        let cta_y = area.y + text_rows + 1;
        if cta_y < area.y + area.height {
            // Hover follows the button cells (mouse-pos driven, like the sibling
            // info blocks); the shared painter owns the styling + truncation.
            let btn_w =
                UnicodeWidthStr::width(format!("[{label}]").as_str()).min(area.width as usize);
            let hovered = mouse_pos.is_some_and(|(mx, my)| {
                my == cta_y && mx >= area.x && (mx as usize) < area.x as usize + btn_w
            });
            // Pinned (non-dismissible) promo shows its dim `cta.caption`; a
            // dismissible one stays bare. No permission prompt on the welcome
            // screen, so no gating; the painter drops it whole if too narrow.
            let caption = (!crate::views::announcements::is_dismissible(ann))
                .then(|| crate::views::announcements::usable_cta_caption(ann))
                .flatten();
            cta_rect = crate::views::announcements::render_cta_button(
                buf, theme, area.x, cta_y, area.width, label, caption, hovered,
            );
        }
    }
    (text_area, truncated, cta_rect)
}

/// Render the announcement (title + message) into `area`. Collapsed wraps to
/// 2 lines + a `…`; expanded shows what fits; the block brightens while
/// hovered, but only when it's interactive (overflowing or already expanded).
/// Returns whether the message was truncated (the "expandable" signal).
fn render_announcement_block(
    buf: &mut Buffer,
    theme: &Theme,
    area: Rect,
    ann: &announcements::Announcement,
    expanded: bool,
    mouse_pos: Option<(u16, u16)>,
) -> bool {
    let over = mouse_pos.is_some_and(|(mx, my)| area.contains(Position::new(mx, my)));
    let mut row = area.y;
    let max_w = area.width as usize;
    if let Some(title) = ann.title.as_deref() {
        let title_color = match ann.severity.as_deref() {
            Some("critical") => theme.accent_error,
            _ => theme.warning,
        };
        let title_style = Style::default()
            .fg(title_color)
            .add_modifier(Modifier::BOLD);
        let display = crate::render::line_utils::truncate_str(title, max_w);
        buf.set_span(area.x, row, &Span::styled(display, title_style), area.width);
        row += 1;
    }
    if let Some(msg) = ann.message.as_deref() {
        let remaining_rows = (area.y + area.height).saturating_sub(row) as usize;
        let max_lines = if expanded {
            remaining_rows
        } else {
            remaining_rows.min(2)
        };
        // Only brighten when there's something to toggle (an overflowing message
        // or the already-expanded state); a short message that fits isn't
        // clickable, so it must not look interactive.
        let interactive = expanded || wrapped_line_count(msg, area.width) as usize > max_lines;
        let hovered = over && interactive;
        let msg_style = super::hover_style(theme, hovered, Style::default().fg(theme.gray));
        // Dim `…` affordance unless hovered.
        let ell_style = super::hover_style(
            theme,
            hovered,
            Style::default()
                .fg(theme.gray_bright)
                .add_modifier(Modifier::DIM),
        );
        return render_wrapped_text(
            buf, area.x, row, area.width, msg, msg_style, ell_style, max_lines,
        );
    }
    false
}

/// Word-wrap `text` into lines no wider than `width` columns. A single word
/// longer than `width` becomes its own (over-wide) line; the renderer clips it.
fn wrap_lines(text: &str, width: u16) -> Vec<String> {
    use unicode_width::UnicodeWidthStr;

    let w = width as usize;
    let mut lines: Vec<String> = Vec::new();
    if w == 0 {
        return lines;
    }
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current = word.to_string();
        } else if current.width() + 1 + word.width() <= w {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Number of rows `text` occupies when word-wrapped to `width` columns. Shared
/// by the layout height pre-pass and the renderer so they can't drift.
fn wrapped_line_count(text: &str, width: u16) -> u16 {
    wrap_lines(text, width).len() as u16
}

/// Rows the announcement TEXT wants at `width`: title + message, the message
/// capped at 2 wrapped lines unless `expanded`. Shared with the renderer so the
/// announcement CTA is placed right after the drawn text (reserved == drawn).
fn announcement_text_rows(ann: &announcements::Announcement, width: u16, expanded: bool) -> u16 {
    let title_rows = if ann.title.is_some() { 1u16 } else { 0 };
    let msg_rows = ann.message.as_deref().map_or(0, |msg| {
        let wrapped = wrapped_line_count(msg, width);
        if expanded { wrapped } else { wrapped.min(2) }
    });
    title_rows + msg_rows
}

/// Rows the announcement info slot wants at `width`: the text rows plus, when a
/// promo announcement CTA is shown, a spacer row + the `[label]` button row
/// (`ANNOUNCEMENT_CTA_ROWS`). Shared with the renderer (reserved == drawn).
fn announcement_desired_rows(
    ann: &announcements::Announcement,
    width: u16,
    expanded: bool,
    has_promo_cta: bool,
) -> u16 {
    announcement_text_rows(ann, width, expanded)
        + if has_promo_cta {
            ANNOUNCEMENT_CTA_ROWS
        } else {
            0
        }
}

/// Word-wrap `text` into at most `max_lines` rows at (`x`, `y`). Overflow ends
/// the last row with a `…` painted in `ell_style`. Returns whether the text was
/// truncated.
#[allow(clippy::too_many_arguments)]
fn render_wrapped_text(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    width: u16,
    text: &str,
    style: Style,
    ell_style: Style,
    max_lines: usize,
) -> bool {
    use unicode_width::UnicodeWidthStr;

    if max_lines == 0 || width == 0 {
        return false;
    }
    let w = width as usize;
    let lines = wrap_lines(text, width);
    let truncated = lines.len() > max_lines;
    let visible = max_lines.min(lines.len());

    for (i, line) in lines.iter().take(visible).enumerate() {
        let row = y + i as u16;
        if i + 1 == visible && truncated {
            // Hard-cut the text and append our own styled `…` (no built-in one).
            let (head, ell_x) = if line.width() < w {
                (line.as_str(), x + line.width() as u16)
            } else {
                let cut =
                    crate::render::line_utils::byte_offset_at_width(line, w.saturating_sub(1));
                (&line[..cut], x + line[..cut].width() as u16)
            };
            buf.set_span(x, row, &Span::styled(head, style), width);
            buf.set_span(ell_x, row, &Span::styled("…", ell_style), 1);
        } else {
            buf.set_span(x, row, &Span::styled(line.as_str(), style), width);
        }
    }
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::style::Style;

    fn extract_text(buf: &Buffer, x: u16, y: u16, width: u16) -> String {
        (x..x + width)
            .map(|col| {
                buf.cell((col, y))
                    .map_or(' ', |c| c.symbol().chars().next().unwrap_or(' '))
            })
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn theme() -> crate::theme::Theme {
        crate::theme::Theme::current()
    }

    /// Distinctive long managed-config message whose tail ("incidents") only
    /// shows when expanded — mirrors the enterprise-policy case from the bug.
    const LONG_MSG: &str = "Enterprise security policy is now in effect for all \
managed devices and accounts. Report security incidents";

    fn ann(title: Option<&str>, message: Option<&str>) -> announcements::Announcement {
        announcements::Announcement {
            title: title.map(str::to_string),
            message: message.map(str::to_string),
            ..Default::default()
        }
    }

    fn all_text(buf: &Buffer, area: Rect) -> String {
        (area.y..area.y + area.height)
            .map(|r| extract_text(buf, area.x, r, area.width))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn layout(w: u16, h: u16) -> HeroLayout {
        compute_hero(HeroLayoutInput {
            content_area: Rect::new(0, 0, w, h),
            menu_height: 4,
            with_menu: true,
            ..Default::default()
        })
    }

    // ── Logo tiering (three levels, both dimensions) ──────────────────────

    #[test]
    fn hero_tiers_by_width_and_height() {
        // Big: ≥ 143 wide AND ≥ 39 tall.
        assert_eq!(layout(150, 45).mode, HeroMode::SideBySide(LogoSize::Big));
        assert_eq!(layout(143, 39).mode, HeroMode::SideBySide(LogoSize::Big));
        // Width matters: one column short of the big gate → small.
        assert_eq!(layout(142, 45).mode, HeroMode::SideBySide(LogoSize::Small));
        // Height matters: one row short of the big gate → small.
        assert_eq!(layout(150, 38).mode, HeroMode::SideBySide(LogoSize::Small));
        // Small: ≥ 93 wide AND ≥ 19 tall.
        assert_eq!(layout(93, 19).mode, HeroMode::SideBySide(LogoSize::Small));
        assert_eq!(layout(93, 40).mode, HeroMode::SideBySide(LogoSize::Small));
        // Stacked: small logo on top of the text group.
        assert_eq!(layout(92, 30).mode, HeroMode::Stacked(LogoSize::Small));
        assert_eq!(layout(80, 23).mode, HeroMode::Stacked(LogoSize::Small));
        // None: too narrow or too short for even the small logo.
        assert_eq!(layout(33, 30).mode, HeroMode::TextOnly);
        assert_eq!(layout(100, 18).mode, HeroMode::TextOnly);
    }

    #[test]
    fn hero_falls_back_when_fixed_rows_steal_space() {
        // A tip eats the rows the small logo needs — the mode degrades rather
        // than overflow (logo first, then the announcement slot).
        let with_tip = compute_hero(HeroLayoutInput {
            content_area: Rect::new(0, 0, 100, 21),
            menu_height: 4,
            tip_height: 2,
            with_menu: true,
            ..Default::default()
        });
        assert_eq!(with_tip.mode, HeroMode::TextOnly);

        let without_tip = compute_hero(HeroLayoutInput {
            content_area: Rect::new(0, 0, 100, 21),
            menu_height: 4,
            with_menu: true,
            ..Default::default()
        });
        assert_eq!(without_tip.mode, HeroMode::SideBySide(LogoSize::Small));
    }

    #[test]
    fn hero_announcement_clamped_to_fit() {
        let a = ann(Some("Heads up"), Some(LONG_MSG));
        let layout = compute_hero(HeroLayoutInput {
            content_area: Rect::new(0, 0, 120, 24),
            menu_height: 4,
            announcement: Some(&a),
            with_menu: true,
            with_info: true,
            ..Default::default()
        });
        // Side-by-side small: block = 19 rows; 5 remain for the info slot
        // (spacer + up to 4 announcement rows).
        assert_eq!(layout.mode, HeroMode::SideBySide(LogoSize::Small));
        assert!(layout.info.height > 0);
        assert!(layout.info.height <= 4);
        // The block still fits: hero + info + flex within the content area.
        let block_bottom = layout.hero.y + layout.hero.height;
        assert!(block_bottom <= 24, "hero overflows: {block_bottom}");
    }

    // ── Geometry ──────────────────────────────────────────────────────────

    #[test]
    fn hero_side_by_side_geometry() {
        let l = layout(200, 45);
        // Left column = max(50% of the content width, logo + padding) = 100.
        assert_eq!(l.hero.width, 200);
        assert_eq!(l.logo.width, 80);
        assert_eq!(l.logo.x, 10, "logo centered in the left column");
        assert_eq!(l.text.x, 100, "text group starts at the column split");
        assert!(l.logo.y >= l.hero.y);
        assert!(l.logo.y + l.logo.height <= l.hero.y + l.hero.height);
        assert_eq!(l.version.height, 1);
        assert!(l.menu.y > l.version.y, "menu sits below the version row");
    }

    #[test]
    fn hero_stacked_geometry() {
        let l = layout(80, 30);
        assert_eq!(l.mode, HeroMode::Stacked(LogoSize::Small));
        assert_eq!(l.logo.width, 30);
        assert_eq!(l.logo.x, 25, "stacked logo horizontally centered");
        assert_eq!(l.text.y, l.logo.y + l.logo.height + 1);
        assert_eq!(l.text.width, 51, "stacked text group uses the stable width");
    }

    #[test]
    fn hero_no_logo_when_too_small() {
        let l = layout(30, 20);
        assert_eq!(l.mode, HeroMode::TextOnly);
        assert_eq!(l.logo.height, 0);
        assert_eq!(
            l.text.width, 26,
            "text group shrinks to the available width"
        );
    }

    #[test]
    fn hero_keeps_version_subtitle_menu_order() {
        let l = layout(100, 40);
        assert_eq!(l.mode, HeroMode::SideBySide(LogoSize::Small));
        assert!(l.version.y == l.text.y);
        assert!(l.subtitle.y == l.text.y + 1);
        assert!(l.menu.y > l.subtitle.y);
        assert_eq!(l.menu.height, 4);
        // No announcement → subtitle shown, no info slot.
        assert_eq!(l.info.height, 0);
        assert!(l.subtitle.height > 0);
    }

    // ── Borderless ────────────────────────────────────────────────────────

    #[test]
    fn hero_renders_no_border_glyphs() {
        let area = Rect::new(0, 0, 150, 50);
        let mut buf = Buffer::empty(area);
        let items = [
            ("ctrl+w", "New worktree"),
            ("ctrl+s", "Resume session"),
            ("ctrl+q", "Quit"),
        ];
        let l = compute_hero(HeroLayoutInput {
            content_area: area,
            menu_height: 3,
            with_menu: true,
            ..Default::default()
        });
        render_hero(
            &l,
            &mut buf,
            &theme(),
            &items,
            None,
            None,
            None,
            false,
            None,
            true,
        );
        let border_chars = [
            '╭', '╮', '╰', '╯', '│', '─', '┌', '┐', '└', '┘', '┼', '├', '┤', '┬', '┴',
        ];
        for y in 0..area.height {
            for x in 0..area.width {
                let sym = buf.cell((x, y)).map_or("", |c| c.symbol());
                assert!(
                    !sym.chars().any(|ch| border_chars.contains(&ch)),
                    "border glyph `{sym}` at ({x}, {y}) — the hero must be borderless"
                );
            }
        }
    }

    #[test]
    fn hero_menu_rects_follow_rows() {
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let items = [
            ("ctrl+w", "New worktree"),
            ("ctrl+s", "Resume session"),
            ("ctrl+q", "Quit"),
        ];
        let l = compute_hero(HeroLayoutInput {
            content_area: area,
            menu_height: 3,
            with_menu: true,
            ..Default::default()
        });
        let rects = render_hero(
            &l,
            &mut buf,
            &theme(),
            &items,
            None,
            None,
            None,
            false,
            None,
            true,
        );
        assert_eq!(rects.menu_rects.len(), 3);
        assert!(rects.menu_rects[1].y > rects.menu_rects[0].y);
        assert!(rects.menu_rects[2].y > rects.menu_rects[1].y);
    }

    // ── Announcement / wrapping (moved from the old hero box) ─────────────

    #[test]
    fn wrap_short_text_single_line() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        render_wrapped_text(
            &mut buf,
            0,
            0,
            40,
            "hello world",
            Style::default(),
            Style::default(),
            2,
        );
        assert_eq!(extract_text(&buf, 0, 0, 40), "hello world");
        assert_eq!(extract_text(&buf, 0, 1, 40), "");
    }

    #[test]
    fn wrap_long_text_two_lines() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 3));
        render_wrapped_text(
            &mut buf,
            0,
            0,
            20,
            "one two three four five six",
            Style::default(),
            Style::default(),
            2,
        );
        let line0 = extract_text(&buf, 0, 0, 20);
        let line1 = extract_text(&buf, 0, 1, 20);
        assert!(!line0.is_empty());
        assert!(!line1.is_empty());
        assert_eq!(extract_text(&buf, 0, 2, 20), "");
    }

    #[test]
    fn wrap_empty_text() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        render_wrapped_text(
            &mut buf,
            0,
            0,
            20,
            "",
            Style::default(),
            Style::default(),
            2,
        );
        assert_eq!(extract_text(&buf, 0, 0, 20), "");
    }

    #[test]
    fn wrap_zero_max_lines() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        render_wrapped_text(
            &mut buf,
            0,
            0,
            20,
            "hello",
            Style::default(),
            Style::default(),
            0,
        );
        assert_eq!(extract_text(&buf, 0, 0, 20), "");
    }

    #[test]
    fn wrap_respects_max_lines() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        render_wrapped_text(
            &mut buf,
            0,
            0,
            10,
            "a b c d e f g h i j k l",
            Style::default(),
            Style::default(),
            1,
        );
        assert!(!extract_text(&buf, 0, 0, 10).is_empty());
        assert_eq!(extract_text(&buf, 0, 1, 10), "");
    }

    #[test]
    fn announcement_collapsed_long_shows_two_lines_and_ellipsis() {
        let area = Rect::new(0, 0, 28, 10);
        let mut buf = Buffer::empty(area);
        let a = ann(Some("Heads up"), Some(LONG_MSG));
        let truncated = render_announcement_block(&mut buf, &theme(), area, &a, false, None);
        // Title on row 0, exactly 2 wrapped message rows, then blank.
        assert_eq!(extract_text(&buf, 0, 0, area.width), "Heads up");
        assert!(!extract_text(&buf, 0, 1, area.width).is_empty());
        assert!(!extract_text(&buf, 0, 2, area.width).is_empty());
        assert_eq!(extract_text(&buf, 0, 3, area.width), "");
        // The 2nd message line ends with the `…` affordance, and a hit-rect.
        assert!(extract_text(&buf, 0, 2, area.width).contains('…'));
        assert!(truncated);
        // The tail of the message is hidden while collapsed.
        assert!(!all_text(&buf, area).contains("incidents"));
    }

    #[test]
    fn announcement_short_no_ellipsis_no_rect() {
        let area = Rect::new(0, 0, 40, 6);
        let mut buf = Buffer::empty(area);
        let a = ann(Some("FYI"), Some("All systems normal."));
        let truncated = render_announcement_block(&mut buf, &theme(), area, &a, false, None);
        assert!(!truncated);
        assert!(!all_text(&buf, area).contains('…'));
    }

    #[test]
    fn short_announcement_does_not_brighten_on_hover() {
        // A short message that fits isn't clickable, so hovering must not
        // brighten it (otherwise it looks interactive when it isn't).
        let area = Rect::new(0, 0, 40, 6);
        let theme = theme();
        let a = ann(Some("FYI"), Some("All systems normal."));
        let mut buf = Buffer::empty(area);
        // Mouse over the message row (row 1; the title is row 0).
        let truncated = render_announcement_block(&mut buf, &theme, area, &a, false, Some((1, 1)));
        assert!(!truncated);
        assert_eq!(
            buf.cell((0, 1)).unwrap().fg,
            theme.gray,
            "short announcement must stay dim on hover"
        );
    }

    #[test]
    fn overflowing_announcement_brightens_on_hover() {
        // A collapsible (overflowing) message is interactive, so hovering it
        // brightens the message to the primary color.
        let area = Rect::new(0, 0, 28, 10);
        let theme = theme();
        let a = ann(Some("Heads up"), Some(LONG_MSG));
        let mut buf = Buffer::empty(area);
        let truncated = render_announcement_block(&mut buf, &theme, area, &a, false, Some((1, 1)));
        assert!(truncated);
        assert_eq!(
            buf.cell((0, 1)).unwrap().fg,
            theme.text_primary,
            "overflowing announcement should brighten on hover"
        );
    }

    #[test]
    fn announcement_expanded_shows_full_message() {
        let area = Rect::new(0, 0, 28, 12);
        // Collapsed hides the tail; expanded reveals it.
        let mut collapsed = Buffer::empty(area);
        let a = ann(Some("Heads up"), Some(LONG_MSG));
        render_announcement_block(&mut collapsed, &theme(), area, &a, false, None);
        assert!(!all_text(&collapsed, area).contains("incidents"));

        let mut expanded = Buffer::empty(area);
        let truncated = render_announcement_block(&mut expanded, &theme(), area, &a, true, None);
        assert!(all_text(&expanded, area).contains("incidents"));
        // Fully shown → nothing truncated, so no `…` and no hit-rect.
        assert!(!all_text(&expanded, area).contains('…'));
        assert!(!truncated);
    }

    #[test]
    fn announcement_expanded_clamped_keeps_ellipsis() {
        // Too few rows for the full message even when expanded: still graceful
        // (renders what fits + keeps the `…`), never overflows the area.
        let area = Rect::new(0, 0, 28, 4);
        let mut buf = Buffer::empty(area);
        let a = ann(Some("Heads up"), Some(LONG_MSG));
        let truncated = render_announcement_block(&mut buf, &theme(), area, &a, true, None);
        assert!(truncated);
        assert!(all_text(&buf, area).contains('…'));
        // Nothing drawn past the area's last row.
        assert_eq!(extract_text(&buf, 0, area.height, area.width), "");
    }

    /// The announcement CTA reserves `ANNOUNCEMENT_CTA_ROWS` on top of the text rows;
    /// `render_announcement_with_cta` paints `[label]` below the message
    /// — plus the dim `cta.caption` for a pinned promo that configures one; bare
    /// for a caption-less pinned promo or a dismissible one — and returns the
    /// button rect (button only, caption excluded).
    #[test]
    fn promo_cta_reserves_rows_and_returns_button_rect() {
        let area = Rect::new(0, 0, 40, 8);
        let a = ann(None, Some("Local maintenance notice. Open documentation."));
        let text_rows = announcement_text_rows(&a, area.width, false);
        assert_eq!(
            announcement_desired_rows(&a, area.width, false, true),
            text_rows + ANNOUNCEMENT_CTA_ROWS,
            "a CTA reserves the spacer + button rows"
        );
        assert_eq!(
            announcement_desired_rows(&a, area.width, false, false),
            text_rows
        );

        // Pinned promo with a configured caption: button + dim caption below.
        let mut pinned = ann(None, Some("Local maintenance notice. Open documentation."));
        pinned.dismissible = Some(false);
        pinned.cta = Some(announcements::AnnouncementCta {
            label: Some("Open Docs".into()),
            url: Some("https://example.com/grow".into()),
            caption: Some("or use Ctrl+O".into()),
        });
        let mut buf = Buffer::empty(area);
        let (text_area, _truncated, cta_rect) = render_announcement_with_cta(
            &mut buf,
            &theme(),
            area,
            &pinned,
            false,
            None,
            Some("Open Docs"),
        );
        let rect = cta_rect.expect("CTA returns a button rect");
        assert_eq!(
            text_area.height,
            area.height - ANNOUNCEMENT_CTA_ROWS,
            "text area shrinks by the reserved CTA rows"
        );
        assert!(
            rect.y >= text_area.y + text_rows,
            "button sits below the text"
        );
        assert_eq!(rect.width, 11, "rect is the [Open Docs] button only");
        let row = extract_text(&buf, area.x, rect.y, area.width);
        assert_eq!(
            row, "[Open Docs] or use Ctrl+O",
            "pinned promo hero shows the configured caption; row={row:?}"
        );

        // Caption-less pinned promo: bare button (nothing hardcoded fills in).
        pinned.cta.as_mut().unwrap().caption = None;
        let mut buf = Buffer::empty(area);
        let (_ta, _t, cta_rect) = render_announcement_with_cta(
            &mut buf,
            &theme(),
            area,
            &pinned,
            false,
            None,
            Some("Open Docs"),
        );
        let rect = cta_rect.expect("caption-less pinned promo still shows the button");
        let row = extract_text(&buf, area.x, rect.y, area.width);
        assert_eq!(row, "[Open Docs]", "absent caption stays bare");

        // Dismissible promo: bare button even with a configured caption.
        let mut dismissible = ann(None, Some("Local maintenance notice. Open documentation."));
        dismissible.cta = Some(announcements::AnnouncementCta {
            label: Some("Open Docs".into()),
            url: Some("https://example.com/grow".into()),
            caption: Some("or use Ctrl+O".into()),
        });
        let mut buf = Buffer::empty(area);
        let (_ta, _t, cta_rect) = render_announcement_with_cta(
            &mut buf,
            &theme(),
            area,
            &dismissible,
            false,
            None,
            Some("Open Docs"),
        );
        let rect = cta_rect.expect("dismissible promo still shows the button");
        let row = extract_text(&buf, area.x, rect.y, area.width);
        assert!(row.contains("[Open Docs]"), "row={row:?}");
        assert!(
            !row.contains("Ctrl+O"),
            "dismissible hero ignores the configured caption; row={row:?}"
        );

        // No CTA: no rect, full-height text area.
        let mut buf = Buffer::empty(area);
        let (text_area, _t, cta_rect) =
            render_announcement_with_cta(&mut buf, &theme(), area, &a, false, None, None);
        assert!(cta_rect.is_none());
        assert_eq!(text_area.height, area.height);
    }
}
