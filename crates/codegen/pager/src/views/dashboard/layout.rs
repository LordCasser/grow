//! Pure geometry for the dashboard header, list, composer, and footer.

use ratatui::layout::Rect;

/// Minimum width at which the dashboard can render meaningful rows.
/// Below this, the renderer falls back to a stripped, single-column
/// view; row labels are middle-truncated.
pub const MIN_DASHBOARD_WIDTH: u16 = 40;

/// Minimum height reserved for the scrollable list when a read-only preview
/// is shown separately beneath it.
pub const LIST_FLOOR_ROWS: u16 = 12;

/// Outer horizontal padding for the dispatch box (cols on each side).
///
/// Matches `LayoutConfig::outer_hpad_left/right = 2` from the agent
/// view's default appearance config.
pub const DISPATCH_OUTER_HPAD: u16 = 2;

/// Outer horizontal padding for the top page header (cols on each side).
/// Matches list/dispatch so the title aligns with content below.
pub const HEADER_OUTER_HPAD: u16 = 2;

/// Outer horizontal padding for the row list (cols on each side).
///
/// Gives the list (rows + group headers + scrollbar) breathing room so
/// selection markers, group header rules (`────`), and row text don't
/// sit flush against the terminal edges.
pub const LIST_OUTER_HPAD: u16 = 2;

/// Output of [`compute_layout`].
#[derive(Debug, Clone, Copy)]
pub struct DashboardLayout {
    /// Top margin row (blank space above the header). Height: 0 or 1.
    /// Matches the welcome view's `v_margin` so the
    /// dashboard's title row doesn't sit flush against the alt-screen
    /// top edge.
    pub top_margin: Rect,
    /// Header row (title + summary). Height: 0 or 1.
    pub header: Rect,
    /// Vertical breathing room between the header and the row list.
    /// Height: 0 or 1. Drops to 0 on short terminals (`area.height
    /// <= 10`, mirroring the dispatch/shortcuts gap threshold).
    /// No sub-renderer ever touches this rect — the area-wide
    /// `bg_base` fill paints it. Conceptually it's the same "blank
    /// breathing-room row" as the dispatch_gap / shortcuts_gap (it's
    /// kept as a named rect rather than an anonymous y-cursor bump
    /// only so tests can pin its position and threshold).
    pub header_gap: Rect,
    /// Scrollable list area (rows + group headers).
    pub list: Rect,
    /// Bottom dispatch input area.
    pub dispatch: Rect,
    /// Footer / shortcut hint row.
    pub footer: Rect,
    /// Bottom margin row (blank space below the shortcuts bar).
    /// Height: 0 or 1. Matches the agent view's
    /// `bottom_vpad` from `eff_outer_vpad` in `LayoutConfig::default`
    /// so the dashboard's shortcuts bar doesn't sit flush against the
    /// alt-screen's bottom edge. Drops to 0 on short terminals
    /// (`area.height <= 16`, same threshold as
    /// `views::agent::AgentViewLayout::compute`).
    pub bottom_margin: Rect,
}

/// Compute the dashboard layout for a given content area.
pub fn compute_layout(area: Rect) -> DashboardLayout {
    // Single text row is the default — callers that support a growing
    // multiline dispatch box (Shift+Enter newlines) use
    // [`compute_layout_with_dispatch`] to request more.
    compute_layout_with_dispatch(area, 1)
}

fn dashboard_chrome_heights(area: Rect) -> (u16, u16, u16, u16, u16, u16, u16, bool) {
    // Match welcome/agent top margin; drop on short terminals.
    let top_margin_h: u16 = if area.height > 6 { 1 } else { 0 };
    let header_h: u16 = if area.height > 4 { 1 } else { 0 };
    // Header↔list gap; collapses with dispatch/shortcuts gaps on short terms.
    let header_gap_h: u16 = if area.height > 10 { 1 } else { 0 };
    let footer_h: u16 = if area.height >= 2 { 1 } else { 0 };
    // Match agent prompt/shortcuts gaps; drop on short terminals.
    let dispatch_gap_h: u16 = if area.height > 10 { 1 } else { 0 };
    let shortcuts_gap_h: u16 = if area.height > 10 { 1 } else { 0 };
    // Match agent bottom_vpad; drop when height <= 16.
    let bottom_margin_h: u16 = if area.height > 16 { 1 } else { 0 };
    let short_terminal = area.height <= 8;
    (
        top_margin_h,
        header_h,
        header_gap_h,
        footer_h,
        dispatch_gap_h,
        shortcuts_gap_h,
        bottom_margin_h,
        short_terminal,
    )
}

/// Like [`compute_layout`] but lets the caller request a taller
/// dispatch box. `dispatch_text_rows` is the number of *text* rows the
/// dispatch input wants (≥1); the box adds 2 more for its top/bottom
/// border chrome. Used to grow the box as the user inserts newlines
/// (Shift+Enter) so multiline dispatch prompts are fully visible.
///
pub fn compute_layout_with_dispatch(area: Rect, dispatch_text_rows: u16) -> DashboardLayout {
    // When `area.height == 0`, every subrect collapses
    // to zero. A footer_h = 1 default would produce a non-zero
    // footer rect even on a 0-height area.
    if area.height == 0 {
        let z = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 0,
        };
        return DashboardLayout {
            top_margin: z,
            header: z,
            header_gap: z,
            list: z,
            dispatch: z,
            footer: z,
            bottom_margin: z,
        };
    }
    let (
        top_margin_h,
        header_h,
        header_gap_h,
        footer_h,
        dispatch_gap_h,
        shortcuts_gap_h,
        bottom_margin_h,
        short_terminal,
    ) = dashboard_chrome_heights(area);
    let dispatch_h: u16 = if !short_terminal {
        2 + dispatch_text_rows.max(1)
    } else {
        1
    };
    let remaining = area.height.saturating_sub(
        top_margin_h
            + header_h
            + header_gap_h
            + footer_h
            + dispatch_h
            + dispatch_gap_h
            + shortcuts_gap_h
            + bottom_margin_h,
    );

    let mut y = area.y;
    let top_margin = Rect {
        x: area.x,
        y,
        width: area.width,
        height: top_margin_h,
    };
    y += top_margin_h;
    // Inset the top page header to match list/dispatch content columns.
    let header_inner_pad = HEADER_OUTER_HPAD.saturating_mul(2);
    let header_width = area.width.saturating_sub(header_inner_pad);
    let header_x = if header_width > 0 {
        area.x.saturating_add(HEADER_OUTER_HPAD)
    } else {
        area.x
    };
    let header = Rect {
        x: header_x,
        y,
        width: if header_width > 0 {
            header_width
        } else {
            area.width
        },
        height: header_h,
    };
    y += header_h;

    // 1-row gap between header and list (collapsed on short
    // terminals). Painted by `render_dashboard`'s full-area fill —
    // no sub-renderer touches it.
    let header_gap = Rect {
        x: area.x,
        y,
        width: area.width,
        height: header_gap_h,
    };
    y += header_gap_h;

    // Polish — inset the list by LIST_OUTER_HPAD on each side so the
    // row content and group header rules have side breathing room.
    // The outer columns stay painted bg_base by the area-wide fill in
    // render_dashboard. Mirrors the dispatch inset pattern but with a
    // smaller pad (1 vs 2) because row text is long and dense.
    let list_inner_pad = LIST_OUTER_HPAD.saturating_mul(2);
    let list_width = area.width.saturating_sub(list_inner_pad);
    let list_x = if list_width > 0 {
        area.x.saturating_add(LIST_OUTER_HPAD)
    } else {
        area.x
    };
    let list = Rect {
        x: list_x,
        y,
        width: if list_width > 0 {
            list_width
        } else {
            area.width
        },
        height: remaining,
    };
    y += remaining;

    // 1-row gap between the list and the dispatch
    // box (mirrors `prompt_gap` in `views::agent::AgentViewLayout`).
    y += dispatch_gap_h;

    // Single-line dispatch input keeps its 2-col
    // outer padding so the `❯` prefix lines up with the row content
    // (rows are indented past the marker column too).
    let dispatch_inner_pad = DISPATCH_OUTER_HPAD.saturating_mul(2);
    let dispatch_width = area.width.saturating_sub(dispatch_inner_pad);
    let dispatch_x = if dispatch_width > 0 {
        area.x.saturating_add(DISPATCH_OUTER_HPAD)
    } else {
        area.x
    };
    let dispatch = Rect {
        x: dispatch_x,
        y,
        width: if dispatch_width > 0 {
            dispatch_width
        } else {
            area.width
        },
        height: dispatch_h,
    };
    y += dispatch_h;

    // 1-row gap between the dispatch box and the
    // shortcuts footer (mirrors `shortcuts_gap`).
    y += shortcuts_gap_h;

    let footer = Rect {
        x: area.x,
        y,
        width: area.width,
        height: footer_h,
    };
    y += footer_h;

    // Bottom margin row below the shortcuts bar
    // (matches the agent view's `bottom_vpad`). Painted with
    // `bg_base` by `render_dashboard`'s full-area fill — no
    // sub-renderer ever touches this rect.
    let bottom_margin = Rect {
        x: area.x,
        y,
        width: area.width,
        height: bottom_margin_h,
    };

    DashboardLayout {
        top_margin,
        header,
        header_gap,
        list,
        dispatch,
        footer,
        bottom_margin,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_assigns_disjoint_areas() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        // top_margin + header + list + dispatch + footer
        // + bottom_margin == total. Three blank rows sit between the
        // content rects: the header_gap (between header and list),
        // the dispatch_gap (between list and dispatch), and the
        // shortcuts_gap (between dispatch and footer). The
        // bottom_margin IS a rect with bg_base, so it stays inside
        // `total`. Same mental model as the dispatch/shortcuts
        // gaps — those are intentional blank breathing-room rows
        // that the area-wide bg fill paints without any dedicated
        // sub-renderer.
        let total = layout.top_margin.height
            + layout.header.height
            + layout.list.height
            + layout.dispatch.height
            + layout.footer.height
            + layout.bottom_margin.height;
        // 3 rows are absorbed by the gaps (header_gap +
        // dispatch_gap + shortcuts_gap).
        assert_eq!(total + 3, area.height);
    }

    /// Multiline dispatch: the box grows by exactly one row per extra
    /// text row (2 border rows + N text rows), and the row list gives up
    /// the space so the totals still tile the area.
    #[test]
    fn dispatch_box_grows_for_multiline_input() {
        let area = Rect::new(0, 0, 80, 30);
        let single = compute_layout(area);
        // Single-line default is 3 rows (top border + 1 text + bottom).
        assert_eq!(single.dispatch.height, 3);

        let three = compute_layout_with_dispatch(area, 3);
        assert_eq!(
            three.dispatch.height, 5,
            "3 text rows → 2 border + 3 text = 5 rows",
        );
        // The list absorbs the extra two rows the dispatch box took.
        assert_eq!(
            three.list.height + 2,
            single.list.height,
            "the row list must shrink by exactly the dispatch growth",
        );
        // Dispatch still sits above the footer with the same gap.
        assert_eq!(three.footer.y, three.dispatch.y + three.dispatch.height + 1);
    }

    /// A `dispatch_text_rows` of 0 is floored to a single text row so the
    /// box never collapses below its single-line chrome.
    #[test]
    fn dispatch_box_floors_at_single_text_row() {
        let area = Rect::new(0, 0, 80, 30);
        let zero = compute_layout_with_dispatch(area, 0);
        assert_eq!(zero.dispatch.height, 3, "0 text rows floors to 1 (3 total)");
    }

    /// The dashboard reserves 1 row of bottom margin
    /// below the shortcuts bar on tall enough terminals so the
    /// shortcuts don't sit flush against the alt-screen's bottom edge.
    /// Mirrors the agent view's `bottom_vpad` (`outer_vpad = 1`,
    /// dropped to 0 at `area.height <= 16`).
    #[test]
    fn layout_reserves_bottom_margin_on_tall_terminals() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.bottom_margin.height, 1,
            "tall terminal must reserve a bottom margin row",
        );
        // Bottom margin must sit directly below the footer.
        assert_eq!(
            layout.bottom_margin.y,
            layout.footer.y + layout.footer.height,
            "bottom_margin must sit directly below the footer",
        );
        // Bottom margin must end exactly at area.height — no slack
        // before the alt-screen's bottom edge.
        assert_eq!(
            layout.bottom_margin.y + layout.bottom_margin.height,
            area.y + area.height,
            "bottom_margin must extend to the bottom of `area`",
        );
    }

    /// Bottom margin collapses to 0 on short
    /// terminals (`area.height <= 16`, matching the agent view's
    /// threshold) so the row list isn't starved.
    #[test]
    fn layout_drops_bottom_margin_on_short_terminals() {
        let area = Rect::new(0, 0, 80, 16);
        let layout = compute_layout(area);
        assert_eq!(layout.bottom_margin.height, 0);
    }

    /// Dispatch box gets `DISPATCH_OUTER_HPAD` cols of
    /// outer padding on each side so its rounded border doesn't reach
    /// the terminal edge. Matches the agent view's `outer_hpad_left/right`.
    #[test]
    fn layout_applies_outer_hpad_to_dispatch_box() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.dispatch.x,
            area.x + DISPATCH_OUTER_HPAD,
            "dispatch must be inset by DISPATCH_OUTER_HPAD on the left",
        );
        assert_eq!(
            layout.dispatch.width,
            area.width - DISPATCH_OUTER_HPAD * 2,
            "dispatch width must lose DISPATCH_OUTER_HPAD on each side",
        );
    }

    /// Header is inset by HEADER_OUTER_HPAD; list by LIST_OUTER_HPAD.
    /// Footer remains full-width.
    #[test]
    fn layout_insets_header_and_list() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.header.width,
            area.width - HEADER_OUTER_HPAD * 2,
            "header must be inset by HEADER_OUTER_HPAD on each side",
        );
        assert_eq!(layout.footer.width, area.width);
        // List is intentionally inset by LIST_OUTER_HPAD on each side.
        assert_eq!(layout.list.width, area.width - LIST_OUTER_HPAD * 2);
    }

    /// Polish — the list rect is inset by LIST_OUTER_HPAD cols on each
    /// side so row content (markers, rules, text) has breathing room
    /// and doesn't touch the terminal edges. The outer columns remain
    /// bg_base (painted by the top-level area fill).
    #[test]
    fn layout_applies_outer_hpad_to_list() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.list.x,
            area.x + LIST_OUTER_HPAD,
            "list must be inset by LIST_OUTER_HPAD on the left",
        );
        assert_eq!(
            layout.list.width,
            area.width - LIST_OUTER_HPAD * 2,
            "list width must lose LIST_OUTER_HPAD on each side",
        );
    }

    /// Header h-pad matches list so title aligns with content columns.
    #[test]
    fn layout_applies_outer_hpad_to_header() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(HEADER_OUTER_HPAD, LIST_OUTER_HPAD);
        assert_eq!(
            layout.header.x,
            area.x + HEADER_OUTER_HPAD,
            "header must be inset by HEADER_OUTER_HPAD on the left",
        );
        assert_eq!(
            layout.header.width,
            area.width - HEADER_OUTER_HPAD * 2,
            "header width must lose HEADER_OUTER_HPAD on each side",
        );
        assert_eq!(layout.header.x, layout.list.x);
        assert_eq!(layout.header.width, layout.list.width);
    }

    /// A 1-row gap separates the list from the
    /// dispatch box, and another 1-row gap separates the dispatch box
    /// from the footer. Mirrors `prompt_gap` + `shortcuts_gap` in the
    /// agent view's layout.
    #[test]
    fn layout_reserves_dispatch_and_shortcuts_gaps() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        // 1-row gap before dispatch: dispatch.y - (list.y + list.height) == 1.
        let list_end = layout.list.y + layout.list.height;
        assert_eq!(
            layout.dispatch.y - list_end,
            1,
            "expected 1-row gap between list and dispatch, got {} (list_end={list_end}, dispatch.y={})",
            layout.dispatch.y - list_end,
            layout.dispatch.y,
        );
        // 1-row gap before footer: footer.y - (dispatch.y + dispatch.height) == 1.
        let dispatch_end = layout.dispatch.y + layout.dispatch.height;
        assert_eq!(
            layout.footer.y - dispatch_end,
            1,
            "expected 1-row gap between dispatch and footer, got {} (dispatch_end={dispatch_end}, footer.y={})",
            layout.footer.y - dispatch_end,
            layout.footer.y,
        );
    }

    /// Gaps collapse to 0 on short terminals so the
    /// row list isn't starved. Threshold mirrors the top-margin
    /// threshold pattern (`> 10`).
    #[test]
    fn layout_drops_gaps_on_short_terminals() {
        let area = Rect::new(0, 0, 80, 10);
        let layout = compute_layout(area);
        let list_end = layout.list.y + layout.list.height;
        let dispatch_end = layout.dispatch.y + layout.dispatch.height;
        assert_eq!(
            layout.dispatch.y - list_end,
            0,
            "short terminal must collapse dispatch gap",
        );
        assert_eq!(
            layout.footer.y - dispatch_end,
            0,
            "short terminal must collapse shortcuts gap",
        );
    }

    /// The dashboard reserves a 1-row gap between
    /// the header and the row list on tall enough terminals so the
    /// status chips / `Dashboard` label don't sit flush against the
    /// first group header or row. The gap sits immediately below the
    /// header and immediately above the list.
    #[test]
    fn layout_reserves_header_gap_on_tall_terminals() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.header_gap.height, 1,
            "tall terminal must reserve a header gap row",
        );
        // Header gap sits directly below the header.
        assert_eq!(
            layout.header_gap.y,
            layout.header.y + layout.header.height,
            "header_gap must sit directly below the header",
        );
        // List starts directly below the header gap.
        assert_eq!(
            layout.list.y,
            layout.header_gap.y + layout.header_gap.height,
            "list must start directly below the header_gap",
        );
    }

    /// The header gap collapses to 0 on short
    /// terminals (`area.height <= 10`, same threshold as the
    /// dispatch / shortcuts gaps) so the row list still gets visible
    /// space.
    #[test]
    fn layout_drops_header_gap_on_short_terminals() {
        let area = Rect::new(0, 0, 80, 10);
        let layout = compute_layout(area);
        assert_eq!(
            layout.header_gap.height, 0,
            "short terminal must collapse header_gap",
        );
        // And the list must start immediately below the header (no
        // implicit gap left behind).
        assert_eq!(
            layout.list.y,
            layout.header.y + layout.header.height,
            "list must start directly below the header when the gap collapses",
        );
    }

    /// The dashboard reserves one row of top margin
    /// on terminals tall enough to spare it, mirroring the welcome
    /// view's `v_margin`. Below the height threshold the margin
    /// collapses to 0 so the row list isn't starved.
    #[test]
    fn layout_reserves_top_margin_on_tall_terminals() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.top_margin.height, 1,
            "tall terminal must reserve a top margin row",
        );
        assert_eq!(
            layout.header.y,
            area.y + 1,
            "header must sit below the top margin",
        );
    }

    /// Short terminals collapse the top margin to 0
    /// so the row list still gets visible space. Threshold matches
    /// the dispatch chrome's threshold (`area.height > 6`).
    #[test]
    fn layout_drops_top_margin_on_short_terminals() {
        let area = Rect::new(0, 0, 80, 6);
        let layout = compute_layout(area);
        assert_eq!(layout.top_margin.height, 0);
    }

    #[test]
    fn layout_at_minimum_width_returns_valid_rect() {
        let area = Rect::new(0, 0, MIN_DASHBOARD_WIDTH, 30);
        let layout = compute_layout(area);
        // List is inset by LIST_OUTER_HPAD on each side even at the
        // minimum dashboard width (40 cols → 38 usable for content).
        assert_eq!(layout.list.width, MIN_DASHBOARD_WIDTH - LIST_OUTER_HPAD * 2);
    }

    // Boundary tests: the existing three tests cover the happy paths;
    // the following four close the boundary gaps explicitly.

    /// Zero-height area produces zero-height sub-rects
    /// (and doesn't panic on saturating subtraction).
    #[test]
    fn layout_height_zero_produces_zero_subrects() {
        let area = Rect::new(0, 0, 80, 0);
        let layout = compute_layout(area);
        assert_eq!(layout.header.height, 0);
        assert_eq!(layout.list.height, 0);
        assert_eq!(layout.dispatch.height, 0);
        assert_eq!(layout.footer.height, 0);
    }

    /// Zero-width area returns valid zero-width rects.
    #[test]
    fn layout_width_zero_returns_zero_width_subrects() {
        let area = Rect::new(0, 0, 0, 30);
        let layout = compute_layout(area);
        assert_eq!(layout.list.width, 0);
        assert_eq!(layout.dispatch.width, 0);
    }

    /// A 39-wide area (below `MIN_DASHBOARD_WIDTH=40`)
    /// returns valid sub-rects — the renderer will fall back to
    /// narrow mode. List still receives its outer hpad inset.
    #[test]
    fn layout_width_below_min_returns_valid_subrects() {
        let area = Rect::new(0, 0, MIN_DASHBOARD_WIDTH - 1, 30);
        let layout = compute_layout(area);
        assert_eq!(
            layout.list.width,
            MIN_DASHBOARD_WIDTH - 1 - LIST_OUTER_HPAD * 2
        );
        assert!(layout.list.height > 0);
    }

    #[test]
    fn layout_header_aligns_with_list_and_dispatch() {
        let area = Rect::new(0, 0, 80, 30);
        let layout = compute_layout(area);
        assert_eq!(layout.header.x, layout.list.x);
        assert_eq!(layout.header.x, layout.dispatch.x);
        assert_eq!(layout.header.width, layout.list.width);
        assert_eq!(layout.header.width, layout.dispatch.width);
    }
}
