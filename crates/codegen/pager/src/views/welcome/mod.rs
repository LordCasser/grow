//! Welcome screen — the first thing users see.
//!
//! Layout (top to bottom):
//! - Top margin row (always preserved)
//! - Top bar: repo_root:branch (left), version (right)
//! - Vertically centered borderless hero (logo + text group, see [`hero`])
//! - Tip (optional) at the bottom
//! - Bottom margin
//!
//! The home page has **no input box**: any key the input model doesn't capture
//! starts a new session and forwards the key into its prompt. The trust gate
//! renders through [`WelcomeLayout`] (stacked layout
//! with the prompt + bottom version row preserved).

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::app_view::{SessionPickerEntry, TrustState};
use crate::startup::StartupWarning;
use crate::theme::Theme;
use crate::views::prompt_widget::PromptFlag;
mod hero;
pub(crate) mod logo;
mod menu;
mod top_bar;

use logo::{logo_line_count, render_logo};
use menu::render_menu;
pub(crate) use top_bar::location_line_at;
use top_bar::render_top_bar;

use hero::{HeroLayoutInput, compute_hero, render_hero};

/// True for VS Code and xterm.js embeds (VS Code-family IDEs and Zed) where
/// quit is `Ctrl+D` (canonical: [`TerminalName::is_vscode_family`]).
fn welcome_in_vscode_family() -> bool {
    crate::terminal::terminal_context().brand.is_vscode_family()
}

/// Style for a clickable welcome block: bright primary while `hovered`, else
/// `base`. Shared by the announcement renderers.
pub(super) fn hover_style(theme: &Theme, hovered: bool, base: Style) -> Style {
    if hovered {
        Style::default().fg(theme.text_primary)
    } else {
        base
    }
}

/// Horizontal margin (left and right) in normal mode.
const H_MARGIN: u16 = 2;
/// Horizontal margin in compact mode.
const H_MARGIN_COMPACT: u16 = 1;

fn prompt_inset(compact: bool) -> u16 {
    if compact { 0 } else { 2 }
}

/// Result of rendering the welcome screen.
#[derive(Default)]
pub struct WelcomeRenderResult {
    /// Terminal image/cursor escapes paired with their ownership transition.
    pub post_flush_escapes: Option<crate::terminal::overlay::PostFlush>,
    /// Hit-test rects for each menu item (for click/hover).
    pub menu_rects: Vec<Rect>,
    /// Hit-test rect for the import-claude banner (for click to open import modal).
    pub import_banner_rect: Option<Rect>,
    /// Hit areas from the session picker (for mouse hit-testing).
    pub session_picker_hit_areas: Option<crate::views::picker::PickerHitAreas>,
    /// Whether the announcement overflowed (the "expandable" signal).
    pub announcement_truncated: bool,
    /// Hit-test rect for the full announcement block (click anywhere to toggle).
    pub announcement_rect: Option<Rect>,
    /// Hit-test rect for the promo announcement CTA `[label]` button (click → open).
    pub promo_cta_rect: Option<Rect>,
}

/// Gap between prompt and version line.
const VERSION_GAP: u16 = 1;

/// Computed areas for the stacked trust gate: logo + warning + menu + version
/// row. The home page uses
/// [`hero::compute_hero`] instead.
pub(super) struct WelcomeLayout {
    pub(super) logo: Rect,
    pub(super) error: Rect,
    pub(super) menu: Rect,
    pub(super) version: Rect,
}

/// Inputs to [`WelcomeLayout::compute_stacked`].
///
/// Bundled (and `Default`-able) so call sites name each field.
#[derive(Default)]
struct WelcomeLayoutInput {
    content_area: Rect,
    /// Error/warning row height; 0 when there's nothing to show.
    error_height: u16,
    menu_height: u16,
    tip_height: u16,
    /// Vertical compaction (session picker visible): skip the logo.
    compact: bool,
}

impl WelcomeLayout {
    pub(super) fn fixed_below(tip_height: u16) -> u16 {
        let tip_gap = if tip_height > 0 { 1u16 } else { 0 };
        tip_height + tip_gap + VERSION_GAP + 1
    }

    /// Compute the stacked gate-screen layout. Drops the logo if it would be
    /// truncated or the version row would overflow the content area (the logo
    /// is the first thing to sacrifice so the chrome never deforms).
    fn compute_stacked(input: WelcomeLayoutInput) -> Self {
        let with_logo = Self::compute_stacked_inner(&input, true);
        let bottom = input.content_area.bottom();
        // The logo must render at its full line count: ratatui truncates an
        // over-tall fixed row from the *head* (a 15-row logo becomes a 5-row
        // stub), so a merely non-overflowing version row is not enough — a
        // clipped logo is a deformed logo and must be dropped instead.
        let expected_logo_rows = if input.compact {
            0
        } else {
            logo_line_count(input.content_area.width, input.content_area.height)
        };
        let logo_intact = with_logo.logo.height == expected_logo_rows
            && with_logo.logo.y + with_logo.logo.height <= bottom;
        if logo_intact && with_logo.version.y + with_logo.version.height <= bottom {
            return with_logo;
        }
        Self::compute_stacked_inner(&input, false)
    }

    fn compute_stacked_inner(input: &WelcomeLayoutInput, with_logo: bool) -> Self {
        let WelcomeLayoutInput {
            content_area,
            error_height,
            menu_height,
            tip_height,
            compact,
            ..
        } = input;
        let logo_rows = if *compact || !with_logo {
            0
        } else {
            logo_line_count(content_area.width, content_area.height)
        };
        let gap_after_logo = if *error_height > 0 { 1 } else { 0 };
        let fixed_above = logo_rows + 1 + gap_after_logo + error_height;
        let tip_gap = if *tip_height > 0 { 1u16 } else { 0 };
        let fixed_below = Self::fixed_below(*tip_height);
        let top_pad = if *compact {
            0
        } else {
            let default_menu_height = 4u16;
            content_area
                .height
                .saturating_sub(fixed_above)
                .saturating_sub(default_menu_height)
                .saturating_sub(fixed_below)
                / 3
        };
        let logo_gap = 1u16;
        let flex_gap = 1u16;
        let [_, logo, _, _, error, menu, _, _, _, _, version] = Layout::vertical([
            Constraint::Length(top_pad),
            Constraint::Length(logo_rows),
            Constraint::Length(logo_gap),
            Constraint::Length(gap_after_logo),
            Constraint::Length(*error_height),
            Constraint::Length(*menu_height),
            Constraint::Min(flex_gap),
            Constraint::Length(*tip_height),
            Constraint::Length(tip_gap),
            Constraint::Length(VERSION_GAP),
            Constraint::Length(1), // version
        ])
        .areas(*content_area);
        Self {
            logo,
            error,
            menu,
            version,
        }
    }
}

/// Controls what the version badge renders. None of the modes show a "Beta"
/// marker — "Grow" ships without one.
pub(super) enum VersionBadgeMode {
    /// Full badge: **Grow** VERSION+channel (right-aligned). Used by the trust gate.
    Full,
    /// Hero inline: **Grow**  VERSION (left-aligned). Used by the hero text
    /// group.
    HeroInline,
}

pub(super) fn render_version_badge(
    version_rect: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    h_margin: u16,
    mode: VersionBadgeMode,
) {
    let version_area = Rect {
        width: version_rect.width.saturating_sub(h_margin),
        ..version_rect
    };
    let mut spans = Vec::new();

    let align = match &mode {
        VersionBadgeMode::Full => Alignment::Right,
        VersionBadgeMode::HeroInline => Alignment::Left,
    };
    let channel = update::channel_label();
    match &mode {
        VersionBadgeMode::Full => {
            spans.push(Span::styled(
                "Grow  ",
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("{}{}", version::VERSION, channel),
                Style::default().fg(theme.gray),
            ));
        }
        VersionBadgeMode::HeroInline => {
            spans.push(Span::styled(
                "Grow  ",
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                version::VERSION,
                Style::default().fg(theme.gray),
            ));
        }
    }

    let version_line = Line::from(spans).alignment(align);
    Paragraph::new(version_line).render(version_area, buf);
}

/// All display state for rendering the welcome screen.
pub struct WelcomeRenderParams<'a> {
    /// Folder-trust state. When `Pending`, the welcome screen renders the
    /// trust question instead of the normal prompt.
    pub trust_state: &'a TrustState,
    pub announcement: Option<&'a announcements::Announcement>,
    pub tip: Option<&'a str>,
    pub model_name: &'a str,
    pub flags: &'a [PromptFlag<'a>],
    pub selected: Option<usize>,
    pub has_claude_import: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub session_picker: Option<&'a [SessionPickerEntry]>,
    pub session_picker_loading: bool,
    pub compact: bool,
    pub pending_hint: Option<crate::views::shortcuts_bar::PendingHint>,
    pub startup_warnings: &'a [StartupWarning],
    pub pending_update_version: Option<&'a str>,
    pub session_picker_content_results:
        Option<&'a [shell::extensions::session_search::SearchSessionHit]>,
    pub session_picker_content_loading: bool,
    /// The query the picker entries were server-fetched with (see
    /// [`crate::views::session_picker::effective_filter_query`]).
    pub session_picker_entries_query: Option<&'a str>,
    pub frame: crate::motion::FrameStamp,
    pub session_picker_grouped: bool,
    /// Live working directory (tracks `Effect::SetWorkingDir`), used to pin
    /// the current repo's session group to the top of the picker.
    pub cwd: &'a std::path::Path,
    /// Whether a long managed-config announcement is expanded inline (vs the
    /// default 2-line collapsed view with a trailing `…`).
    pub welcome_announcement_expanded: bool,
    /// Promo announcement CTA `[label]` to paint below the hero announcement: `Some`
    /// drives both the reserved row height and the `[label]` button. `None` = no
    /// CTA on the welcome screen.
    pub promo_cta: Option<&'a str>,
}

/// Render the welcome screen.
pub fn render_welcome(
    area: Rect,
    buf: &mut Buffer,
    params: &WelcomeRenderParams<'_>,
    session_picker_state: &mut crate::views::picker::PickerState,
) -> WelcomeRenderResult {
    let theme = Theme::current();
    let h_margin = if params.compact {
        H_MARGIN_COMPACT
    } else {
        H_MARGIN
    };
    let v_margin = 1u16;

    buf.set_style(area, Style::default().bg(theme.bg_base));

    // Announcements only render inside the hero box. Top bar is always 1 row.
    let [_, top_bar_area, content_area, _] = Layout::vertical([
        Constraint::Length(v_margin),
        Constraint::Length(1),
        Constraint::Min(10),
        Constraint::Length(v_margin),
    ])
    .areas(area);

    let top_bar_inner = Rect {
        x: top_bar_area.x + h_margin,
        y: top_bar_area.y,
        width: top_bar_area.width.saturating_sub(h_margin * 2),
        height: 1,
    };
    render_top_bar(top_bar_inner, buf, &theme, None);

    let mut result = if let TrustState::Pending { workspace } = params.trust_state {
        render_welcome_trust(
            content_area,
            buf,
            &theme,
            workspace,
            params.selected,
            h_margin,
            params.compact,
            params.frame,
        )
    } else {
        render_welcome_done(content_area, buf, &theme, params, session_picker_state)
    };
    if result.post_flush_escapes.is_none() {
        result.post_flush_escapes = crate::terminal::overlay::clear().map(Into::into);
    }
    result
}

/// Render the folder-trust question. Mirrors [`render_welcome_blocked`]'s
/// stacked layout (logo + message + menu + version badge), but the message is a
/// multi-line block showing the workspace path and the warning that Grow
/// may run or modify contents in this directory (a security risk). The y/N
/// answer is handled by the welcome input interceptor, so this only paints;
/// `menu_rects` are returned for parity with the other welcome arms.
fn render_welcome_trust(
    content_area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    workspace: &std::path::Path,
    selected: Option<usize>,
    h_margin: u16,
    compact: bool,
    frame: crate::motion::FrameStamp,
) -> WelcomeRenderResult {
    let menu_items = [("y", "Yes, proceed"), ("n", "No, quit")];
    let lines = vec![
        Line::from(Span::styled(
            "Do you trust the contents of this directory?",
            Style::default().fg(theme.gray_bright),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            workspace.display().to_string(),
            Style::default().fg(theme.accent_user),
        ))
        .alignment(Alignment::Center),
        Line::default(),
        // Two lines so the warning never clips at narrow / compact widths
        // (a single ~78-char line would truncate "...posing security risks").
        Line::from(Span::styled(
            "Grow may run or modify contents in this directory,",
            Style::default().fg(theme.gray),
        ))
        .alignment(Alignment::Center),
        Line::from(Span::styled(
            "posing security risks.",
            Style::default().fg(theme.gray),
        ))
        .alignment(Alignment::Center),
        // Spacer between the warning and the y/n menu.
        Line::default(),
    ];

    let msg_height = lines.len() as u16;
    let menu_height = menu_items.len() as u16;
    let layout = WelcomeLayout::compute_stacked(WelcomeLayoutInput {
        content_area,
        error_height: msg_height,
        menu_height,
        compact,
        ..Default::default()
    });

    render_logo(
        layout.logo,
        buf,
        theme,
        content_area.width,
        content_area.height,
        frame,
    );
    Paragraph::new(lines).render(layout.error, buf);

    let menu_area = inset_horizontal(layout.menu, prompt_inset(compact));
    let menu_rects = render_menu(menu_area, buf, theme, &menu_items, selected, None, 0);

    render_version_badge(layout.version, buf, theme, h_margin, VersionBadgeMode::Full);

    // Only `menu_rects` are meaningful here; the rest are absent (no prompt,
    // picker, auth/gate links) -- `Default` keeps this honest without an
    // all-`None` literal.
    WelcomeRenderResult {
        menu_rects,
        ..Default::default()
    }
}

/// Header text shared by Loopback and Command auth modes.
/// Shrink a rect by `inset` columns on the left and right (clamped at 0).
fn inset_horizontal(rect: Rect, inset: u16) -> Rect {
    Rect {
        x: rect.x + inset,
        width: rect.width.saturating_sub(inset * 2),
        ..rect
    }
}

/// Render the normal welcome screen (Done state -- already authenticated).
///
/// The home page renders the borderless hero (logo + version badge + subtitle
/// + announcement + menu) with the tip at the bottom. There is **no input
/// box** and **no bottom version row** — the badge lives in the hero's text
/// group. The changelog is gone from the welcome screen (release notes stay
/// available via `/release-notes` inside a session).
fn render_welcome_done(
    content_area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    p: &WelcomeRenderParams<'_>,
    session_picker_state: &mut crate::views::picker::PickerState,
) -> WelcomeRenderResult {
    let show_picker = p.session_picker.is_some() || p.session_picker_loading;
    // Only use compact layout when the session picker is visible — it needs
    // the space for its list. Plain compact mode keeps the normal welcome.
    let welcome_compact = show_picker;

    let in_vscode_family = welcome_in_vscode_family();
    // Startup-warning hint height (multi-line aware). Must pick the same
    // entry `render_startup_warnings` draws — see `startup::banner_warning`.
    let hint_height = crate::startup::banner_warning(p.startup_warnings).map_or(0u16, |w| {
        let msg_lines = w.message.lines().count() as u16;
        let action_line = if w.action.is_some() { 1 } else { 0 };
        msg_lines + action_line + 1 // +1 for buffer spacing
    });
    let has_update_tip = p.pending_update_version.is_some();
    // Tip slot precedence: pending update > random tip.
    let tip_height = if !show_picker {
        if has_update_tip {
            1u16
        } else if let Some(tip_text) = p.tip {
            let inset = prompt_inset(welcome_compact);
            let tip_width = content_area.width.saturating_sub(inset * 2);
            crate::tips::render::tip_height(tip_width, tip_text)
        } else {
            0
        }
    } else {
        0
    };

    // Menu order: `[Import Claude settings]`, New worktree, Resume session,
    // Quit — no Changelog row.
    let (key_w, key_s, key_q, key_i_with_x) = (
        "ctrl+w",
        "ctrl+s",
        if in_vscode_family { "ctrl+d" } else { "ctrl+q" },
        "ctrl+i  [x]",
    );
    let mut menu_items: Vec<(&str, &str)> = Vec::with_capacity(4);
    if p.has_claude_import {
        menu_items.push((key_i_with_x, "Import Claude settings"));
    }
    menu_items.push((key_w, "New worktree"));
    menu_items.push((key_s, "Resume session"));
    menu_items.push((key_q, "Quit"));
    let menu_height = if show_picker {
        0
    } else {
        menu_items.len() as u16
    };

    // Session picker height: 1 row per entry (no dividers), scrollable.
    let picker_count = p.session_picker.map_or(0, |s| s.len());
    let picker_height = if show_picker {
        if p.session_picker_loading {
            1
        } else {
            (picker_count as u16).min(15) + 3 // +3 for title + search + gap
        }
    } else {
        0
    };
    let content_height = menu_height + picker_height;

    // The hero measures + clamps the announcement slot itself.
    let hero = compute_hero(HeroLayoutInput {
        content_area,
        error_height: hint_height,
        menu_height: content_height,
        tip_height,
        announcement: p.announcement,
        expanded: p.welcome_announcement_expanded,
        has_promo_cta: p.promo_cta.is_some(),
        with_menu: true,
        with_info: true,
    });

    // Render startup warning in the error area (same slot as auth errors).
    let import_banner_rect = render_startup_warnings(hero.error, buf, theme, p.startup_warnings);

    let mut announcement_truncated = false;
    let mut announcement_rect: Option<Rect> = None;
    let mut promo_cta_rect: Option<Rect> = None;

    let (menu_rects, picker_close_button) = if show_picker {
        // Use the full area since logo/menu are hidden and shortcuts
        // are now rendered inside the picker content area.
        let picker_area = Rect {
            x: content_area.x,
            y: content_area.y,
            width: content_area.width,
            height: content_area.height,
        };
        let hit_areas = render_session_picker(
            picker_area,
            buf,
            theme,
            &mut SessionPickerRenderCtx {
                state: session_picker_state,
                sessions: p.session_picker,
                loading: p.session_picker_loading,
                pending_hint: p.pending_hint,
                shortcuts_area: None,
                content_results: p.session_picker_content_results,
                content_loading: p.session_picker_content_loading,
                entries_query: p.session_picker_entries_query,
                frame: p.frame,
                grouped: p.session_picker_grouped,
                cwd: p.cwd,
            },
        );
        (vec![], Some(hit_areas))
    } else {
        // Borderless hero: logo + version badge + subtitle + announcement +
        // menu. The home page is fully interactive (hover brightening on).
        let rects = render_hero(
            &hero,
            buf,
            theme,
            &menu_items,
            p.selected,
            p.mouse_pos,
            p.announcement,
            p.welcome_announcement_expanded,
            p.promo_cta,
            true,
            p.frame,
        );
        announcement_truncated = rects.announcement_truncated;
        announcement_rect = rects.announcement_rect;
        promo_cta_rect = rects.promo_cta_rect;
        (rects.menu_rects, None)
    };

    // Bottom tip slot: update banner > random tip.
    if hero.tip.height > 0 {
        let [_, tip_centered, _] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(content_area.width),
            Constraint::Min(0),
        ])
        .flex(Flex::Center)
        .areas(hero.tip);
        let inset = prompt_inset(p.compact);
        let tip_inset = Rect {
            x: tip_centered.x + inset,
            y: tip_centered.y,
            width: tip_centered.width.saturating_sub(inset * 2),
            height: tip_centered.height,
        };

        if let Some(ver) = p.pending_update_version {
            // Background update notification in the tip area.
            let key_name = "ctrl+u";
            let line = Line::from(vec![
                Span::styled(
                    "Update: ",
                    Style::default()
                        .fg(theme.accent_user)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("v{ver} available \u{2014} press {key_name} to restart"),
                    Style::default().fg(theme.accent_user),
                ),
            ]);
            Paragraph::new(line)
                .style(Style::default().bg(theme.bg_base))
                .render(tip_inset, buf);
        } else if let Some(tip_text) = p.tip {
            crate::tips::render::render_tip(tip_inset, buf, tip_text);
        }
    }

    WelcomeRenderResult {
        post_flush_escapes: None,
        menu_rects,
        import_banner_rect,
        session_picker_hit_areas: picker_close_button,
        announcement_truncated,
        announcement_rect,
        promo_cta_rect,
    }
}

/// Context for session picker rendering.
pub(crate) struct SessionPickerRenderCtx<'a> {
    pub(crate) state: &'a mut crate::views::picker::PickerState,
    pub(crate) sessions: Option<&'a [SessionPickerEntry]>,
    /// Live working directory (tracks `Effect::SetWorkingDir`), used to pin
    /// the current repo's group to the top.
    pub(crate) cwd: &'a std::path::Path,
    pub(crate) loading: bool,
    pub(crate) pending_hint: Option<crate::views::shortcuts_bar::PendingHint>,
    pub(crate) shortcuts_area: Option<Rect>,
    pub(crate) content_results: Option<&'a [shell::extensions::session_search::SearchSessionHit]>,
    pub(crate) content_loading: bool,
    /// The query `sessions` were server-fetched with (see
    /// [`crate::views::session_picker::effective_filter_query`]).
    pub(crate) entries_query: Option<&'a str>,
    pub(crate) frame: crate::motion::FrameStamp,
    /// When true, entries are grouped by `repo_name` with non-selectable headers.
    pub(crate) grouped: bool,
}

/// Render the session picker list on the welcome screen.
///
/// Builds `PickerEntry` items from `SessionPickerEntry` data and delegates to
/// `render_picker`. Returns `PickerHitAreas` for mouse hit-testing.
pub(crate) fn render_session_picker(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    ctx: &mut SessionPickerRenderCtx<'_>,
) -> crate::views::picker::PickerHitAreas {
    use crate::views::picker::{self, PickerConfig, PickerEntry, PickerField, PickerRow};
    use crate::views::session_picker::{
        SessionEntryData, build_grouped_picker_entries, build_session_entry_data,
    };

    let entries_data = match ctx.sessions {
        Some(s) => s,
        None => &[],
    };

    // Filter entries by query (shared helper). The same effective
    // query must drive filtering AND the content header/rows gates below, or
    // this render disagrees with `handle_welcome_input`'s `build_entry_map`
    // (which receives the effective query) on row indices.
    let filter_query =
        crate::views::session_picker::effective_filter_query(ctx.state.query(), ctx.entries_query);
    let filtered_indices = crate::app::app_view::filter_session_entries(ctx.sessions, filter_query);

    let content_width = area.width; // approximate for truncation
    let built = build_session_entry_data(entries_data, &filtered_indices, ctx.state, content_width);

    // Build PickerEntry refs that borrow from `built`.
    let fields_vecs: Vec<Vec<PickerField>> = built
        .iter()
        .map(|b| {
            b.field_data
                .iter()
                .map(|(l, v)| PickerField { label: l, value: v })
                .collect()
        })
        .collect();

    // Build picker entries, optionally grouped by repo_name.
    let (mut picker_entries, non_selectable_indices) = if ctx.grouped {
        let current_repo =
            crate::views::session_picker::repo_name_from_cwd(&ctx.cwd.to_string_lossy());
        build_grouped_picker_entries(
            entries_data,
            &filtered_indices,
            &built,
            &fields_vecs,
            ctx.state,
            Some(current_repo.as_str()),
        )
    } else {
        let entries: Vec<PickerEntry> = built
            .iter()
            .zip(fields_vecs.iter())
            .map(|(b, fields)| {
                PickerEntry::Row(PickerRow {
                    label: &b.summary,
                    right_label: &b.right_text,
                    selected: b.is_selected,
                    expanded: b.is_expanded,
                    fields,
                    description_lines: &[],
                    summary_lines: &[],
                    dimmed: false,
                    indent: 0,
                    badge: b.badge,
                    badge_color: None,
                    collapsible: b.collapsible,
                    underline_last_desc: false,
                })
            })
            .collect();
        (entries, Vec::new())
    };

    // Append content search result rows (shared helper handles dedup).
    use crate::views::session_picker::{build_content_entry_data, build_content_header_label};
    // Content rows will start after fuzzy rows + 1 header row.
    let content_start = picker_entries.len() + 1;
    let content_entry_data: Vec<SessionEntryData> = if let Some(hits) = ctx.content_results
        && !filter_query.is_empty()
    {
        build_content_entry_data(
            hits,
            entries_data,
            &filtered_indices,
            ctx.state,
            content_start,
        )
    } else {
        Vec::new()
    };

    // Show header only if there are actual deduped content rows to display.
    let has_content_rows = !content_entry_data.is_empty();
    let content_loading = ctx.content_loading;
    let spinner_label = build_content_header_label(content_loading, has_content_rows, ctx.frame);
    // Only show the header when content results exist or when content
    // search is in progress with a non-empty query.  This must match the
    // header condition inside `build_entry_map` as called from
    // `handle_welcome_input` (app_view.rs) so the input handler's
    // `entry_count` agrees with the rendered entry list — a mismatch causes
    // arrow-key selection to target the wrong row. Both sides therefore gate
    // on the same EFFECTIVE query (`filter_query`), not the live one.
    let show_content_header =
        has_content_rows || (content_loading && !filter_query.trim().is_empty());
    if show_content_header {
        picker_entries.push(PickerEntry::Header {
            label: &spinner_label,
        });
    }

    let content_fields: Vec<Vec<PickerField>> = content_entry_data
        .iter()
        .map(|b| {
            b.field_data
                .iter()
                .map(|(l, v)| PickerField { label: l, value: v })
                .collect()
        })
        .collect();

    let content_snippets: Vec<[&str; 1]> = content_entry_data
        .iter()
        .map(|b| [b.snippet_preview.as_deref().unwrap_or("")])
        .collect();

    for (i, (b, fields)) in content_entry_data
        .iter()
        .zip(content_fields.iter())
        .enumerate()
    {
        let has_snippet = b.snippet_preview.is_some();
        picker_entries.push(PickerEntry::Row(PickerRow {
            label: &b.summary,
            right_label: &b.right_text,
            selected: b.is_selected,
            expanded: b.is_expanded,
            fields,
            description_lines: if has_snippet {
                &content_snippets[i]
            } else {
                &[]
            },
            summary_lines: &[],
            dimmed: false,
            indent: 1,
            badge: if has_snippet { "match" } else { "" },
            badge_color: Some(theme.accent_user),
            collapsible: true,
            underline_last_desc: false,
        }));
    }

    // Build shortcuts for fullscreen mode.
    let worktree_shortcut: &'static str = "ctrl+w";
    use crate::views::shortcuts_bar::HintItem;
    let mut default_shortcuts: Vec<HintItem> = vec![
        HintItem::new(crate::key!(Esc), "back"),
        HintItem::new(crate::key!(Enter), "select"),
    ];
    default_shortcuts.push(HintItem {
        keys: vec![],
        label: "worktree".into(),
        custom_display: Some(worktree_shortcut),
        description: None,
        pinned: false,
    });
    default_shortcuts.push(HintItem {
        keys: vec![],
        label: "navigate".into(),
        custom_display: Some("\u{2191}\u{2193}"),
        description: None,
        pinned: false,
    });
    default_shortcuts.push(HintItem {
        keys: vec![],
        label: "filter".into(),
        custom_display: Some("f"),
        description: None,
        pinned: false,
    });

    let config = PickerConfig {
        title: Some("Resume session"),
        show_search_hint: true,
        expandable: true,
        esc_clears_query: true,
        shortcuts: Some(&default_shortcuts),
        pending_hint: ctx.pending_hint,
        non_selectable: &non_selectable_indices,
        non_selectable_clickable: &[],
        shortcuts_area: ctx.shortcuts_area,
        tabs: None,
        active_tab: 0,
        filter_label: None,
        filter_key_hint: None,
        filter_active: false,
        header_note: None,
        action_keys: &[],
        disable_search: false,
        compact_bottom_bar: false,
        search_only_on_slash: false,
        vim_normal_first: crate::appearance::cache::load_vim_mode(),
    };

    picker::render_picker(
        buf,
        area,
        theme,
        ctx.state,
        &picker_entries,
        &config,
        ctx.loading,
        ctx.frame,
    )
}

/// Render one startup warning centered in the given area.
///
/// `startup_warnings` can hold more than one entry (the WezTerm
/// kitty-keyboard banner is prepended ahead of `summarize_warnings()`
/// output — see `diagnostics::assemble_startup_warnings`), but only one is
/// rendered — the severity-aware pick from `startup::banner_warning`, so a
/// runtime-pushed Warning displaces an earlier Info entry. One message line,
/// one optional action line, plus a buffer row for spacing.
/// Severity controls color (yellow for `Warning`, dim for `Info`).
fn render_startup_warnings(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    warnings: &[StartupWarning],
) -> Option<Rect> {
    let w = crate::startup::banner_warning(warnings)?;

    // Skip the import-claude startup warning entirely — the import row in the
    // menu now carries the call-to-action with the same visual weight as
    // every other welcome menu item. Showing the warning text in addition to
    // the menu row would be redundant noise.
    if w.message.starts_with("Import Claude settings")
        || w.message.starts_with("Claude settings detected")
    {
        return None;
    }
    let color = match w.severity {
        crate::startup::WarningSeverity::Warning => theme.warning,
        crate::startup::WarningSeverity::Info => theme.gray_dim,
    };
    let style = Style::default().fg(color);

    let mut lines: Vec<Line<'_>> = w
        .message
        .lines()
        .map(|l| Line::from(Span::styled(l, style)).alignment(Alignment::Center))
        .collect();
    if let Some(ref action) = w.action {
        lines.push(Line::from(Span::styled(action.as_str(), style)).alignment(Alignment::Center));
    }

    Paragraph::new(lines).render(area, buf);
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::app_view::SessionPickerEntry;
    use crate::views::picker::PickerState;
    use crate::views::session_picker::{build_grouped_picker_entries, build_session_entry_data};

    fn make_entry(id: &str, summary: &str, repo_name: &str) -> SessionPickerEntry {
        SessionPickerEntry {
            id: id.into(),
            summary: summary.into(),
            updated_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            cwd: format!("/home/user/{repo_name}"),
            hostname: None,
            model_id: None,
            num_messages: 1,
            last_active_at: None,
            branch: None,
            repo_name: repo_name.into(),
            worktree_label: None,
            card_detail: None,
        }
    }

    fn render_params<'a>(
        trust_state: &'a TrustState,
        session_picker: Option<&'a [SessionPickerEntry]>,
    ) -> WelcomeRenderParams<'a> {
        WelcomeRenderParams {
            trust_state,
            announcement: None,
            tip: None,
            model_name: "test",
            flags: &[],
            selected: None,
            has_claude_import: false,
            mouse_pos: None,
            session_picker,
            session_picker_loading: false,
            compact: false,
            pending_hint: None,
            startup_warnings: &[],
            pending_update_version: None,
            session_picker_content_results: None,
            session_picker_content_loading: false,
            session_picker_entries_query: None,
            frame: crate::motion::FrameStamp::default(),
            session_picker_grouped: false,
            cwd: std::path::Path::new("/repo"),
            welcome_announcement_expanded: false,
            promo_cta: None,
        }
    }

    fn render_done_text(params: &WelcomeRenderParams<'_>) -> String {
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut picker = PickerState::default();
        render_welcome(area, &mut buf, params, &mut picker);
        buffer_text(&buf)
    }

    fn png() -> [u8; 8] {
        [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']
    }

    fn seed_static_owner(owner_id: u64) {
        let _ = crate::terminal::overlay::static_image(&png(), 20, 10, 0, 0, owner_id)
            .unwrap()
            .commit();
    }

    fn assert_promptless_clear(result: WelcomeRenderResult, owner_id: u64) {
        let post_flush = result
            .post_flush_escapes
            .expect("promptless welcome must clear ID 1");
        assert!(post_flush.as_str().contains("a=d"));
        let before_write =
            crate::terminal::overlay::static_image(&png(), 20, 10, 0, 0, owner_id).unwrap();
        assert!(
            !before_write.as_str().contains("a=t"),
            "constructing the clear must not commit ownership"
        );
        post_flush.write_to(&mut Vec::new()).unwrap();
        let after_write =
            crate::terminal::overlay::static_image(&png(), 20, 10, 0, 0, owner_id).unwrap();
        assert!(
            after_write.as_str().contains("a=t"),
            "writing the clear must commit ownership"
        );
    }

    #[test]
    fn picker_welcome_returns_paired_overlay_clear() {
        let _guard = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        crate::terminal::overlay::reset_owner();
        seed_static_owner(82);
        let trust_state = TrustState::Done;
        let sessions = [make_entry("session-1", "summary", "repo")];
        let params = render_params(&trust_state, Some(&sessions));
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut picker = PickerState::default();

        let result = render_welcome(area, &mut buf, &params, &mut picker);
        assert_promptless_clear(result, 82);
    }

    /// RENDER half of the header-gate invariant (input half:
    /// `session_picker::tests::grouped_entry_map_empty_query_with_loading_has_no_header`):
    /// with stamp==live and a re-search in flight, the "Searching…" header
    /// must NOT render — a render-only header row shifts arrow-key row
    /// indices. Control leg: the same search WITHOUT the stamp keeps it.
    #[test]
    fn render_header_gate_uses_effective_query() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;

        let theme = crate::theme::Theme::default();
        let area = Rect::new(0, 0, 80, 20);
        // Content-only hit: title shares nothing with the query "hit".
        let entries = vec![make_entry("conv-1", "Quarterly roadmap notes", "repo")];

        let render = |entries_query: Option<&str>| -> String {
            let mut buf = Buffer::empty(area);
            let mut state = PickerState::default();
            state.set_query("hit");
            render_session_picker(
                area,
                &mut buf,
                &theme,
                &mut SessionPickerRenderCtx {
                    state: &mut state,
                    sessions: Some(&entries),
                    cwd: std::path::Path::new("/repo"),
                    loading: false,
                    pending_hint: None,
                    shortcuts_area: None,
                    content_results: None,
                    content_loading: true,
                    entries_query,
                    frame: crate::motion::FrameStamp::default(),
                    grouped: false,
                },
            );
            (0..area.height)
                .map(|y| {
                    (0..area.width)
                        .map(|x| {
                            buf.cell((x, y))
                                .map_or(' ', |c| c.symbol().chars().next().unwrap_or(' '))
                        })
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let stamped = render(Some("hit"));
        assert!(
            !stamped.contains("Searching session content"),
            "stamp==live must not render the search header:\n{stamped}"
        );
        assert!(
            stamped.contains("Quarterly roadmap notes"),
            "stamped server hit must render:\n{stamped}"
        );

        // Control: unstamped in-flight search keeps the header, proving the
        // negative assertion above exercises the gate.
        let unstamped = render(None);
        assert!(
            unstamped.contains("Searching session content"),
            "in-flight search without the stamp must render the header:\n{unstamped}"
        );
    }

    #[test]
    fn grouped_entries_insert_headers() {
        let entries = vec![
            make_entry("s1", "Fix auth", "grow"),
            make_entry("s2", "Add streaming", "grow"),
            make_entry("s3", "Nuke tables", "fw-1"),
        ];
        let indices: Vec<usize> = (0..entries.len()).collect();
        let state = PickerState::default();
        let built = build_session_entry_data(&entries, &indices, &state, 80);
        let fields_vecs: Vec<Vec<crate::views::picker::PickerField>> =
            built.iter().map(|_| Vec::new()).collect();

        let (result, non_sel) =
            build_grouped_picker_entries(&entries, &indices, &built, &fields_vecs, &state, None);

        // 2 headers + 3 rows = 5 entries
        assert_eq!(result.len(), 5);
        // Groups are sorted alphabetically: fw-1 before grow.
        // Header positions: 0 (fw-1), 2 (grow)
        assert_eq!(non_sel.len(), 5);
        assert!(non_sel[0], "first entry should be header (non-selectable)");
        assert!(!non_sel[1], "second entry should be selectable row");
        assert!(non_sel[2], "third entry should be header (non-selectable)");
        assert!(!non_sel[3], "fourth entry should be selectable row");
        assert!(!non_sel[4], "fifth entry should be selectable row");

        // Verify headers
        assert!(
            matches!(&result[0], crate::views::picker::PickerEntry::Header { label } if label == &"fw-1")
        );
        assert!(
            matches!(&result[2], crate::views::picker::PickerEntry::Header { label } if label == &"grow")
        );
    }

    #[test]
    fn grouped_entries_pin_current_repo_first() {
        // Render path (build_grouped_picker_entries) must pin the current
        // working directory's repo group ahead of the alphabetical rest,
        // matching build_entry_map's index ordering.
        let entries = vec![
            make_entry("s1", "Fix auth", "aaa"),
            make_entry("s2", "Add streaming", "zzz"),
        ];
        let indices: Vec<usize> = (0..entries.len()).collect();
        let state = PickerState::default();
        let built = build_session_entry_data(&entries, &indices, &state, 80);
        let fields_vecs: Vec<Vec<crate::views::picker::PickerField>> =
            built.iter().map(|_| Vec::new()).collect();

        // Pin "zzz": it leads despite sorting last alphabetically.
        let (result, _) = build_grouped_picker_entries(
            &entries,
            &indices,
            &built,
            &fields_vecs,
            &state,
            Some("zzz"),
        );
        assert!(
            matches!(&result[0], crate::views::picker::PickerEntry::Header { label } if label == &"zzz"),
            "current repo group pinned first"
        );
        assert!(
            matches!(&result[2], crate::views::picker::PickerEntry::Header { label } if label == &"aaa"),
            "remaining group follows alphabetically"
        );
    }

    #[test]
    fn grouped_entries_single_group_has_one_header() {
        let entries = vec![
            make_entry("s1", "Fix auth", "grow"),
            make_entry("s2", "Add streaming", "grow"),
        ];
        let indices: Vec<usize> = (0..entries.len()).collect();
        let state = PickerState::default();
        let built = build_session_entry_data(&entries, &indices, &state, 80);
        let fields_vecs: Vec<Vec<crate::views::picker::PickerField>> =
            built.iter().map(|_| Vec::new()).collect();

        let (result, non_sel) =
            build_grouped_picker_entries(&entries, &indices, &built, &fields_vecs, &state, None);

        assert_eq!(result.len(), 3); // 1 header + 2 rows
        assert!(non_sel[0]);
        assert!(!non_sel[1]);
        assert!(!non_sel[2]);
    }

    #[test]
    fn grouped_entries_empty_input() {
        let entries: Vec<SessionPickerEntry> = vec![];
        let indices: Vec<usize> = vec![];
        let state = PickerState::default();
        let built = build_session_entry_data(&entries, &indices, &state, 80);
        let fields_vecs: Vec<Vec<crate::views::picker::PickerField>> = vec![];

        let (result, non_sel) =
            build_grouped_picker_entries(&entries, &indices, &built, &fields_vecs, &state, None);

        assert!(result.is_empty());
        assert!(non_sel.is_empty());
    }

    #[test]
    fn grouped_entries_rows_are_indented() {
        let entries = vec![make_entry("s1", "Fix auth", "grow")];
        let indices: Vec<usize> = vec![0];
        let state = PickerState::default();
        let built = build_session_entry_data(&entries, &indices, &state, 80);
        let fields_vecs: Vec<Vec<crate::views::picker::PickerField>> =
            built.iter().map(|_| Vec::new()).collect();

        let (result, _) =
            build_grouped_picker_entries(&entries, &indices, &built, &fields_vecs, &state, None);

        // The row (second entry) should have indent=1
        if let crate::views::picker::PickerEntry::Row(row) = &result[1] {
            assert_eq!(row.indent, 1);
        } else {
            panic!("expected Row, got Header");
        }
    }

    fn resume_picker_config() -> crate::views::picker::PickerConfig<'static> {
        crate::views::picker::PickerConfig {
            title: Some("Resume session"),
            show_search_hint: true,
            expandable: true,
            esc_clears_query: true,
            shortcuts: None,
            pending_hint: None,
            non_selectable: &[],
            non_selectable_clickable: &[],
            shortcuts_area: None,
            tabs: None,
            active_tab: 0,
            filter_label: None,
            filter_key_hint: None,
            filter_active: false,
            header_note: None,
            action_keys: &[],
            disable_search: false,
            compact_bottom_bar: false,
            search_only_on_slash: false,
            vim_normal_first: false,
        }
    }

    #[test]
    fn e_key_expands_selected_entry_in_resume_picker() {
        use crate::views::picker::{PickerOutcome, handle_picker_input};
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        let mut state = PickerState::default();
        let config = resume_picker_config();
        let ev = Event::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        let outcome = handle_picker_input(&ev, &mut state, 3, &config);
        assert!(matches!(outcome, PickerOutcome::Expand(0)));
    }

    #[test]
    fn e_key_routes_to_search_when_active() {
        use crate::views::picker::{PickerOutcome, handle_picker_input};
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

        let mut state = PickerState::input_active();
        let config = resume_picker_config();
        let ev = Event::Key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        let outcome = handle_picker_input(&ev, &mut state, 3, &config);
        assert!(matches!(outcome, PickerOutcome::QueryChanged));
        assert_eq!(state.query(), "e");
    }

    #[test]
    fn home_page_has_no_prompt_or_version_footer() {
        // The home page must not render an input box, the bottom version row,
        // "Beta", or anything changelog-related.
        let trust = TrustState::Done;
        let text = render_done_text(&render_params(&trust, None));
        assert!(
            !text.contains("Type a message"),
            "home must not render the prompt placeholder:\n{text}"
        );
        assert!(!text.contains("Beta"), "no Beta anywhere:\n{text}");
        assert!(
            !text.contains("Changelog"),
            "no changelog on the home page:\n{text}"
        );
        // The menu carries the four (or three) home actions instead.
        assert!(text.contains("New worktree"), "{text}");
        assert!(text.contains("Resume session"), "{text}");
        assert!(text.contains("Quit"), "{text}");
    }

    #[test]
    fn home_page_menu_includes_import_row_when_detected() {
        let trust = TrustState::Done;
        let mut params = render_params(&trust, None);
        params.has_claude_import = true;
        let text = render_done_text(&params);
        assert!(
            text.contains("Import Claude settings"),
            "import row must render when Claude settings were detected:\n{text}"
        );
        assert!(text.contains("ctrl+i"), "{text}");
    }

    #[test]
    fn home_page_renders_menu_keys_without_changelog_row() {
        // menu_count = 3 without import: New worktree, Resume session, Quit.
        let trust = TrustState::Done;
        let text = render_done_text(&render_params(&trust, None));
        assert!(!text.contains("Changelog"), "{text}");
    }

    #[test]
    fn blocked_layout_keeps_logo_menu_and_version() {
        // The trust screen keeps its stacked layout on wide terminals.
        let area = Rect::new(0, 0, 120, 40);
        let layout = WelcomeLayout::compute_stacked(WelcomeLayoutInput {
            content_area: area,
            menu_height: 2,
            ..Default::default()
        });
        assert!(layout.logo.height > 0, "gate screens keep the logo");
        assert!(layout.menu.height > 0);
        assert!(
            layout.version.y + layout.version.height <= area.bottom(),
            "version row must stay inside the content area"
        );
    }

    #[test]
    fn blocked_layout_drops_logo_when_chrome_overflows() {
        // A message-heavy gate screen (e.g. the trust question) that cannot
        // fit the logo AND the chrome must drop the logo rather than clip the
        // version row.
        let area = Rect::new(0, 0, 40, 25);
        let with_logo = WelcomeLayout::compute_stacked(WelcomeLayoutInput {
            content_area: area,
            error_height: 10,
            menu_height: 2,
            ..Default::default()
        });
        assert_eq!(
            with_logo.logo.height, 0,
            "the logo must be dropped when the chrome would overflow"
        );
        assert!(
            with_logo.version.y + with_logo.version.height <= area.bottom(),
            "version row must still fit"
        );
    }

    #[test]
    fn blocked_layout_is_compact_without_logo() {
        let area = Rect::new(0, 0, 120, 40);
        let layout = WelcomeLayout::compute_stacked(WelcomeLayoutInput {
            content_area: area,
            menu_height: 2,
            compact: true,
            ..Default::default()
        });
        assert_eq!(layout.logo.height, 0);
        assert!(layout.menu.height > 0);
    }

    /// Flatten a rendered buffer into one string for substring assertions.
    fn buffer_text(buf: &Buffer) -> String {
        let area = *buf.area();
        let mut out = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn home_page_renders_announcement_in_hero_info_slot() {
        // A wide terminal shows the announcement inside the hero text group
        // (the info slot), and the clickable block rect is reported.
        let trust = TrustState::Done;
        let mut params = render_params(&trust, None);
        let a = announcements::Announcement {
            title: Some("Security policy".into()),
            message: Some("Report security incidents to the security team.".into()),
            ..Default::default()
        };
        params.announcement = Some(&a);
        let area = Rect::new(0, 0, 150, 50);
        let mut buf = Buffer::empty(area);
        let mut picker = PickerState::default();
        let result = render_welcome(area, &mut buf, &params, &mut picker);
        let text = buffer_text(&buf);
        assert!(text.contains("Security policy"), "{text}");
        assert!(
            result.announcement_rect.is_some(),
            "announcement click rect must be reported"
        );
        assert!(
            !text.contains("Changelog"),
            "the info slot must never fall back to a changelog:\n{text}"
        );
    }

    #[test]
    fn home_menu_keys_stay_clear_of_screen_right_edge_on_wide_terminals() {
        // Regression: at 120×50 the hero renders side-by-side and the menu
        // row used to span the whole text column, flush-righting ctrl+w to
        // ~3 cols from the screen edge. The row must cap its width and pad
        // the shortcuts so the keys keep ≥5 cols of clearance.
        let trust = TrustState::Done;
        let params = render_params(&trust, None);
        let area = Rect::new(0, 0, 120, 50);
        let mut buf = Buffer::empty(area);
        let mut picker = PickerState::default();
        render_welcome(area, &mut buf, &params, &mut picker);
        // Scan cell-by-cell: each cell is exactly one column, so the x where
        // the accumulated row first ends with "ctrl+w" is the trailing 'w'
        // column. (Byte offsets would be skewed by multi-byte symbols such as
        // the logo's braille glyphs sharing the menu row in side-by-side mode.)
        let mut key_col = None;
        'outer: for y in 0..area.height {
            let mut row = String::new();
            for x in 0..area.width {
                row.push_str(buf[(x, y)].symbol());
                if row.ends_with("ctrl+w") {
                    // "ctrl+w" is 6 cells wide; its first cell sits 5 cells
                    // before the 'w' cell at x.
                    key_col = Some(x - ("ctrl+w".len() as u16 - 1));
                    break 'outer;
                }
            }
        }
        let key_col = key_col.unwrap_or_else(|| {
            panic!(
                "home must render the ctrl+w menu row:\n{}",
                buffer_text(&buf)
            )
        });
        let last_key_col = key_col + "ctrl+w".len() as u16 - 1;
        let right_gap = area.width - 1 - last_key_col;
        assert!(
            right_gap >= 5,
            "ctrl+w must stay ≥5 cols from the screen right edge (got {right_gap}):\n{}",
            buffer_text(&buf)
        );
    }
}
