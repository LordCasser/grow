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
//! starts a new session and forwards the key into its prompt. The login /
//! ZDR / trust gate screens render through [`WelcomeLayout`] (stacked layout
//! with the prompt + bottom version row preserved).

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding, Paragraph, Widget, Wrap};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::app_view::{AuthMode, AuthState, SessionPickerEntry, TrustState};
use crate::startup::StartupWarning;
use crate::theme::Theme;
use crate::views::prompt_widget::{PromptFlag, PromptInfo, PromptWidget};
mod hero;
pub(crate) mod logo;
mod menu;
mod prompt;
mod top_bar;

pub(crate) use logo::shimmer_frame;
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

/// Build the quit hint spans used in Authenticating sub-screens.
fn quit_hint_spans(theme: &Theme) -> Vec<Span<'static>> {
    let key = if welcome_in_vscode_family() {
        "ctrl+d"
    } else {
        "ctrl+q"
    };
    vec![
        Span::styled(
            key,
            Style::default()
                .fg(theme.accent_user)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  quit", Style::default().fg(theme.gray)),
    ]
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

/// Whether the welcome prompt is currently focused (accepting text input).
/// Only the login gate screen (AuthState::Pending) renders the prompt; the
/// home page has no input box, so the focus state is always `Unfocused` there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WelcomePromptFocus {
    #[default]
    Unfocused,
    Focused,
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
    /// Hit-test rect for the auth copy line (click-to-copy during Authenticating).
    pub auth_url_rect: Option<Rect>,
    /// Hit-test rect for the "show full URL" fallback link.
    pub auth_fallback_rect: Option<Rect>,
    /// Whether the announcement overflowed (the "expandable" signal).
    pub announcement_truncated: bool,
    /// Hit-test rect for the full announcement block (click anywhere to toggle).
    pub announcement_rect: Option<Rect>,
    /// Hit-test rect for the promo announcement CTA `[label]` button (click → open).
    pub promo_cta_rect: Option<Rect>,
}

/// Prompt input height (shared across welcome states).
const PROMPT_HEIGHT: u16 = 3;
/// Gap between prompt and version line.
const VERSION_GAP: u16 = 1;

/// Computed areas for the stacked gate screens (login / ZDR / trust): logo +
/// optional message + menu + optional prompt + version row. The home page uses
/// [`hero::compute_hero`] instead.
pub(super) struct WelcomeLayout {
    pub(super) logo: Rect,
    pub(super) error: Rect,
    pub(super) menu: Rect,
    pub(super) prompt: Rect,
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
        tip_height + tip_gap + PROMPT_HEIGHT + VERSION_GAP + 1
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
        let [_, logo, _, _, error, menu, _, _, _, prompt, _, version] = Layout::vertical([
            Constraint::Length(top_pad),
            Constraint::Length(logo_rows),
            Constraint::Length(logo_gap),
            Constraint::Length(gap_after_logo),
            Constraint::Length(*error_height),
            Constraint::Length(*menu_height),
            Constraint::Min(flex_gap),
            Constraint::Length(*tip_height),
            Constraint::Length(tip_gap),
            Constraint::Length(PROMPT_HEIGHT),
            Constraint::Length(VERSION_GAP),
            Constraint::Length(1), // version
        ])
        .areas(*content_area);
        Self {
            logo,
            error,
            menu,
            prompt,
            version,
        }
    }
}

/// Controls what the version badge renders. None of the modes show a "Beta"
/// marker — "Grow" ships without one.
pub(super) enum VersionBadgeMode {
    /// Full badge: team | **Grow** VERSION+channel (right-aligned). Used by
    /// the stacked gate screens (login / ZDR / trust).
    Full,
    /// Hero inline: **Grow**  VERSION (left-aligned). Used by the hero text
    /// group.
    HeroInline,
}

pub(super) fn render_version_badge(
    version_rect: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    team_name: Option<&str>,
    h_margin: u16,
    mode: VersionBadgeMode,
) {
    let version_area = Rect {
        width: version_rect.width.saturating_sub(h_margin),
        ..version_rect
    };
    let sep = Span::styled(
        "  \u{2502}  ",
        Style::default().fg(theme.gray).add_modifier(Modifier::DIM),
    );
    let mut spans = Vec::new();

    let (show_team, align) = match &mode {
        VersionBadgeMode::Full => (true, Alignment::Right),
        VersionBadgeMode::HeroInline => (false, Alignment::Left),
    };

    if show_team && let Some(team) = team_name {
        spans.push(Span::styled(team, Style::default().fg(theme.gray)));
        spans.push(sep);
    }
    let channel = grow_update::channel_label();
    match &mode {
        VersionBadgeMode::Full => {
            spans.push(Span::styled(
                "Grow  ",
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                format!("{}{}", grow_version::VERSION, channel),
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
                grow_version::VERSION,
                Style::default().fg(theme.gray),
            ));
        }
    }

    let version_line = Line::from(spans).alignment(align);
    Paragraph::new(version_line).render(version_area, buf);
}

/// All display state for rendering the welcome screen.
pub struct WelcomeRenderParams<'a> {
    pub auth_state: &'a AuthState,
    /// Folder-trust state. When `Pending` (auth done, access granted), the
    /// welcome screen renders the trust question instead of the normal prompt.
    pub trust_state: &'a TrustState,
    pub login_label: Option<&'a str>,
    pub auth_code_input: &'a str,
    pub auth_code_cursor_byte: usize,
    pub clipboard_delivery: Option<crate::clipboard::ClipboardDelivery>,
    pub show_raw_url: bool,
    pub announcement: Option<&'a grow_announcements::Announcement>,
    pub tip: Option<&'a str>,
    pub model_name: &'a str,
    pub flags: &'a [PromptFlag<'a>],
    pub selected: Option<usize>,
    pub team_name: Option<&'a str>,
    pub has_claude_import: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub is_zdr_blocked: bool,
    pub session_picker: Option<&'a [SessionPickerEntry]>,
    pub session_picker_loading: bool,
    pub compact: bool,
    pub pending_hint: Option<crate::views::shortcuts_bar::PendingHint>,
    pub startup_warnings: &'a [StartupWarning],
    pub pending_update_version: Option<&'a str>,
    /// Recent foreign session offered on ctrl+u, suppressed by a pending update.
    pub foreign_resume_hint: Option<&'a grow_workspace::foreign_sessions::RecentForeignSession>,
    pub session_picker_content_results:
        Option<&'a [grow_shell::extensions::session_search::SearchSessionHit]>,
    pub session_picker_content_loading: bool,
    /// The query the picker entries were server-fetched with (see
    /// [`crate::views::session_picker::effective_filter_query`]).
    pub session_picker_entries_query: Option<&'a str>,
    pub welcome_tick: u64,
    pub session_picker_grouped: bool,
    /// Source filter for the session picker.
    pub session_picker_source_filter: crate::views::session_picker::SourceFilter,
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
    prompt: &mut PromptWidget,
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

    let mut result = match params.auth_state {
        AuthState::Pending { error } => {
            let label = params.login_label.unwrap_or("service.example.com");
            let login_text = format!("Login with {}", label);
            let menu = [("l", login_text.as_str()), ("q", "Quit")];
            let msg = error.as_deref().map(|e| (e, theme.accent_error));
            let info = PromptInfo {
                agent_name: "grow",
                model_name: params.model_name,
                flags: params.flags,
                multiline: false,
                usage_warning: None,
                usage_warning_critical: false,
            };
            let (menu_rects, post_flush_escapes) = render_welcome_blocked(
                content_area,
                buf,
                msg,
                &menu,
                params.selected,
                Some((prompt, &info)),
                h_margin,
                params.compact,
            );
            WelcomeRenderResult {
                post_flush_escapes,
                menu_rects,
                session_picker_hit_areas: None,
                import_banner_rect: None,
                auth_url_rect: None,
                auth_fallback_rect: None,
                announcement_truncated: false,
                announcement_rect: None,
                promo_cta_rect: None,
            }
        }
        AuthState::Authenticating { auth_url, mode, .. } => {
            let llc = logo_line_count(content_area.width, content_area.height);
            let (url_rect, fallback_rect) = render_welcome_authenticating(
                content_area,
                buf,
                &theme,
                llc,
                auth_url.as_deref(),
                *mode,
                params.auth_code_input,
                params.auth_code_cursor_byte,
                params.clipboard_delivery,
                params.show_raw_url,
            );
            WelcomeRenderResult {
                post_flush_escapes: None,
                menu_rects: vec![],
                session_picker_hit_areas: None,
                import_banner_rect: None,
                auth_url_rect: url_rect,
                auth_fallback_rect: fallback_rect,
                announcement_truncated: false,
                announcement_rect: None,
                promo_cta_rect: None,
            }
        }
        AuthState::Done if params.is_zdr_blocked => {
            let menu = [("l", "Switch account"), ("q", "Quit")];
            let (menu_rects, post_flush_escapes) = render_welcome_blocked(
                content_area,
                buf,
                Some((
                    "Grow is not yet available for this account.",
                    theme.gray_bright,
                )),
                &menu,
                params.selected,
                None,
                h_margin,
                params.compact,
            );
            WelcomeRenderResult {
                post_flush_escapes,
                menu_rects,
                session_picker_hit_areas: None,
                import_banner_rect: None,
                auth_url_rect: None,
                auth_fallback_rect: None,
                announcement_truncated: false,
                announcement_rect: None,
                promo_cta_rect: None,
            }
        }
        // Folder-trust question: shown after auth, before any session is
        // created, when the cwd has untrusted repo-local config. Mirrors the
        // Pending login screen. Skipped under the ZDR gate above.
        AuthState::Done => {
            if let TrustState::Pending { workspace } = params.trust_state {
                render_welcome_trust(
                    content_area,
                    buf,
                    &theme,
                    workspace,
                    params.selected,
                    h_margin,
                    params.compact,
                )
            } else {
                render_welcome_done(
                    content_area,
                    buf,
                    &theme,
                    params,
                    session_picker_state,
                )
            }
        }
    };
    if result.post_flush_escapes.is_none() {
        result.post_flush_escapes = crate::terminal::overlay::clear().map(Into::into);
    }
    result
}

/// Render a blocked welcome screen: logo + optional message + menu + version.
///
/// Used for both the login screen (Pending) and the ZDR gate. The layout is:
///   Logo
///   {message}
///   Menu items
///   {prompt}      (optional)
///   Version badge
#[allow(clippy::too_many_arguments)]
fn render_welcome_blocked(
    content_area: Rect,
    buf: &mut Buffer,
    message: Option<(&str, ratatui::style::Color)>,
    menu_items: &[(&str, &str)],
    selected: Option<usize>,
    prompt: Option<(&mut PromptWidget, &PromptInfo<'_>)>,
    h_margin: u16,
    compact: bool,
) -> (Vec<Rect>, Option<crate::terminal::overlay::PostFlush>) {
    let theme = Theme::current();

    let msg_height = if message.is_some() { 2u16 } else { 0u16 };
    let menu_height = menu_items.len() as u16;
    // Force the stacked layout: this renderer only paints the stacked
    // logo/menu rects.
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
        &theme,
        content_area.width,
        content_area.height,
    );

    if let Some((text, color)) = message {
        let line =
            Line::from(Span::styled(text, Style::default().fg(color))).alignment(Alignment::Center);
        Paragraph::new(line).render(layout.error, buf);
    }

    // Inset the menu the same as the input bar / post-auth menu so the actions
    // keep side spacing instead of touching the window edge on narrow terminals.
    let menu_area = inset_horizontal(layout.menu, prompt::prompt_inset(compact));
    let menu_rects = render_menu(menu_area, buf, &theme, menu_items, selected, None, 0);

    let post_flush_escapes = if let Some((prompt_widget, info)) = prompt {
        let [_, prompt_centered, _] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(content_area.width),
            Constraint::Min(0),
        ])
        .flex(Flex::Center)
        .areas(layout.prompt);
        prompt::render_prompt(
            prompt_centered,
            buf,
            WelcomePromptFocus::Unfocused,
            prompt_widget,
            info,
            2,
            2,
            compact,
        )
        .1
    } else {
        None
    };

    render_version_badge(
        layout.version,
        buf,
        &theme,
        None,
        h_margin,
        VersionBadgeMode::Full,
    );
    (menu_rects, post_flush_escapes)
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
    );
    Paragraph::new(lines).render(layout.error, buf);

    let menu_area = inset_horizontal(layout.menu, prompt::prompt_inset(compact));
    let menu_rects = render_menu(menu_area, buf, theme, &menu_items, selected, None, 0);

    render_version_badge(
        layout.version,
        buf,
        theme,
        None,
        h_margin,
        VersionBadgeMode::Full,
    );

    // Only `menu_rects` are meaningful here; the rest are absent (no prompt,
    // picker, auth/gate links) -- `Default` keeps this honest without an
    // all-`None` literal.
    WelcomeRenderResult {
        menu_rects,
        ..Default::default()
    }
}

/// Header text shared by Loopback and Command auth modes.
const AUTH_HEADER: &str = "A browser window will open for authentication.";
/// Header text for the device-flow auth mode.
const DEVICE_AUTH_HEADER: &str = "Approve in your browser to finish signing in.";
/// Caption beneath the device code.
const DEVICE_CODE_CAPTION: &str = "Make sure your browser shows this code.";

/// Extract `user_code` from a device verification URL (`None` if absent or
/// malformed). Shown on-screen so the user can confirm it matches the browser
/// before approving (anti-phishing).
fn extract_user_code(url: &str) -> Option<&str> {
    let code = url
        .split('?')
        .nth(1)?
        .split('&')
        .find_map(|kv| kv.strip_prefix("user_code="))?;
    let valid = !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then_some(code)
}
/// Clickable copy prompt shared by Loopback and Command auth modes.
const AUTH_COPY_PREFIX: &str = "If it doesn't open, click ";
const AUTH_COPY_HERE: &str = "here";
const AUTH_COPY_SUFFIX: &str = " to copy.";

/// Build the "click here to copy" line with "here" underlined in accent color.
fn auth_copy_line(theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(AUTH_COPY_PREFIX, Style::default().fg(theme.gray_bright)),
        Span::styled(
            AUTH_COPY_HERE,
            Style::default()
                .fg(theme.accent_user)
                .add_modifier(Modifier::UNDERLINED),
        ),
        Span::styled(AUTH_COPY_SUFFIX, Style::default().fg(theme.gray_bright)),
    ])
    .alignment(Alignment::Center)
}

/// Number of physical rows the header + blank occupy before the copy line.
fn auth_copy_preceding_rows(header: &str, inner_width: u16) -> u16 {
    let header_rows = (header.len() as u16).div_ceil(inner_width);
    header_rows + 1 // header + blank
}

/// Number of physical rows the copy line occupies when wrapped.
fn auth_copy_line_rows(inner_width: u16) -> u16 {
    let copy_len = AUTH_COPY_PREFIX.len() + AUTH_COPY_HERE.len() + AUTH_COPY_SUFFIX.len();
    (copy_len as u16).div_ceil(inner_width)
}

const AUTH_FALLBACK_TEXT: &str = "Copying not working? Click here to show full URL.";

/// Build the fallback "show full URL" link line.
fn auth_fallback_line(theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        AUTH_FALLBACK_TEXT,
        Style::default()
            .fg(theme.gray)
            .add_modifier(Modifier::UNDERLINED),
    ))
    .alignment(Alignment::Center)
}

/// Push the shared copy-prompt block, stable feedback slot, and raw-URL fallback.
fn push_auth_copy_block(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    clipboard_delivery: Option<crate::clipboard::ClipboardDelivery>,
) {
    lines.push(Line::default());
    lines.push(auth_copy_line(theme));
    lines.push(Line::default());
    lines.push(match clipboard_delivery {
        Some(crate::clipboard::ClipboardDelivery::Confirmed) => {
            Line::from(Span::styled("copied!", Style::default().fg(theme.gray)))
                .alignment(Alignment::Center)
        }
        Some(crate::clipboard::ClipboardDelivery::Unverified) => Line::from(Span::styled(
            "copy sent—verify paste",
            Style::default().fg(theme.gray),
        ))
        .alignment(Alignment::Center),
        Some(crate::clipboard::ClipboardDelivery::Failed) => {
            Line::from(Span::styled("copy failed", Style::default().fg(theme.gray)))
                .alignment(Alignment::Center)
        }
        None => Line::default(),
    });
    lines.push(Line::default());
    lines.push(auth_fallback_line(theme));
}

/// Rows occupied by [`push_auth_copy_block`].
fn auth_copy_block_rows(inner_width: u16) -> u16 {
    auth_copy_line_rows(inner_width) + 5
}

/// Click hit-rects for the copy line and fallback link. `header`'s wrapped row
/// count sets the copy line's vertical offset.
fn auth_hit_rects(
    msg_area: Rect,
    h_pad: u16,
    inner_width: u16,
    header: &str,
    preceding_extra: u16,
) -> (Option<Rect>, Option<Rect>) {
    let preceding = auth_copy_preceding_rows(header, inner_width) + preceding_extra;
    let copy_rows = auth_copy_line_rows(inner_width);
    let copy_rect = Rect {
        x: msg_area.x + h_pad,
        y: msg_area.y + preceding,
        width: inner_width,
        height: copy_rows,
    };
    // fallback line is after: copy_rows + blank + copied_slot + blank
    let fallback_y = msg_area.y + preceding + copy_rows + 3;
    let fb_rect = Rect {
        x: msg_area.x + h_pad,
        y: fallback_y,
        width: inner_width,
        height: 1,
    };
    (Some(copy_rect), Some(fb_rect))
}

/// Render the "raw URL" mode: shows the full URL with mouse capture disabled
/// so the user can select and copy it natively.
fn render_raw_url_mode(
    content_area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    top_pad: u16,
    logo_line_count: u16,
    auth_url: Option<&str>,
) -> (Option<Rect>, Option<Rect>) {
    // Use full terminal width for the URL so the terminal wraps it
    // naturally without inserting spaces (important for copy-paste).
    let full_width = content_area.width.max(1);
    let url_lines = auth_url
        .map(|u| (u.len() as u16).div_ceil(full_width))
        .unwrap_or(0);
    let msg_height = 1 + 1 + url_lines; // hint + blank + URL
    let [_, logo_area, _, msg_area, _, hint_area, _] = Layout::vertical([
        Constraint::Length(top_pad),
        Constraint::Length(logo_line_count),
        Constraint::Length(2),
        Constraint::Length(msg_height),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(content_area);

    render_logo(
        logo_area,
        buf,
        theme,
        content_area.width,
        content_area.height,
    );

    // Render hint above the URL.
    let hint = Line::from(Span::styled(
        "Select the URL below with your mouse and copy manually.",
        Style::default().fg(theme.gray),
    ))
    .alignment(Alignment::Center);
    Paragraph::new(hint).render(
        Rect {
            height: 1,
            ..msg_area
        },
        buf,
    );

    // Write the URL directly to the buffer character-by-character so the
    // terminal wraps naturally at the screen edge. Ratatui's Paragraph
    // wrap inserts spaces at break points which corrupts the URL on copy.
    //
    // When the URL fits on a single line, center it to match the rest of the
    // screen. When it's longer, keep it flush-left at the full terminal width
    // so the natural wrap preserves copy-paste (centering a wrapped URL would
    // inject leading spaces into the selection).
    if let Some(url) = auth_url {
        let url_style = Style::default().fg(theme.accent_user);
        let url_y = msg_area.y + 2; // after hint + blank
        // Control characters are skipped below to prevent terminal escape
        // injection, so measure the URL without them.
        let url_len = url.chars().filter(|c| !c.is_control()).count() as u16;
        let x_offset = if url_len <= full_width {
            (full_width - url_len) / 2
        } else {
            0
        };
        let buf_area = buf.area();
        let buf_max_col = buf_area.x + buf_area.width;
        let buf_max_row = buf_area.y + buf_area.height;
        for (i, ch) in url.chars().filter(|c| !c.is_control()).enumerate() {
            let col = msg_area.x + x_offset + (i as u16) % full_width;
            let row = url_y + (i as u16) / full_width;
            if row >= msg_area.y + msg_area.height {
                break;
            }
            // Guard against OOB access during resize races.
            if col >= buf_max_col || row >= buf_max_row {
                continue;
            }
            buf[(col, row)].set_char(ch).set_style(url_style);
        }
    }

    let hint_spans = vec![
        Span::styled(
            "ctrl+q",
            Style::default()
                .fg(theme.accent_user)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  go back", Style::default().fg(theme.gray)),
    ];
    let hints = Line::from(hint_spans).alignment(Alignment::Center);
    Paragraph::new(hints).render(hint_area, buf);

    (None, None) // no click rects — mouse capture is disabled
}

/// Which "browser opened, now waiting" arm to render; owns the header,
/// waiting caption, and (for `Device`) the device-code derivation.
#[derive(Clone, Copy)]
enum BrowserStatusKind {
    /// External auth provider opened its own browser.
    Command,
    /// RFC 8628 device flow — also shows the device code.
    Device,
}

/// Render a "browser opened, now waiting" auth arm (Command + Device).
///
/// Shared status layout: logo, then a centered block of header, optional device
/// code + caption, optional copy/fallback links (when there's a URL), and the
/// waiting caption; finally quit hints.
#[allow(clippy::too_many_arguments)]
fn render_browser_status_arm(
    content_area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    top_pad: u16,
    logo_line_count: u16,
    auth_url: Option<&str>,
    show_raw_url: bool,
    clipboard_delivery: Option<crate::clipboard::ClipboardDelivery>,
    kind: BrowserStatusKind,
) -> (Option<Rect>, Option<Rect>) {
    let h_pad: u16 = content_area.width / 6;
    let inner_width = content_area.width.saturating_sub(h_pad * 2).max(1);

    if show_raw_url {
        return render_raw_url_mode(content_area, buf, theme, top_pad, logo_line_count, auth_url);
    }

    // Device also parses the user code from the verification URL.
    let (header, waiting_text, user_code) = match kind {
        BrowserStatusKind::Command => (AUTH_HEADER, "Waiting for login to complete...", None),
        BrowserStatusKind::Device => (
            DEVICE_AUTH_HEADER,
            "Waiting for approval...",
            auth_url.and_then(extract_user_code),
        ),
    };

    let header_rows = (header.len() as u16).div_ceil(inner_width);
    let code_extra = if user_code.is_some() {
        let caption_rows = (DEVICE_CODE_CAPTION.len() as u16).div_ceil(inner_width);
        1 + 1 + 1 + caption_rows // blank + code + blank + caption
    } else {
        0
    };
    let copy_extra = if auth_url.is_some() {
        auth_copy_block_rows(inner_width)
    } else {
        0
    };
    let msg_height = header_rows + code_extra + copy_extra + 1 + 1; // blank + waiting

    let [_, logo_area, _, msg_area, _, hint_area, _] = Layout::vertical([
        Constraint::Length(top_pad),
        Constraint::Length(logo_line_count),
        Constraint::Length(2),          // gap
        Constraint::Length(msg_height), // status message
        Constraint::Min(1),             // gap
        Constraint::Length(1),          // hints
        Constraint::Min(0),
    ])
    .areas(content_area);

    render_logo(
        logo_area,
        buf,
        theme,
        content_area.width,
        content_area.height,
    );

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(header, Style::default().fg(theme.gray_bright)))
            .alignment(Alignment::Center),
    ];
    if let Some(code) = user_code {
        lines.push(Line::default());
        lines.push(
            Line::from(Span::styled(
                code.to_owned(),
                Style::default()
                    .fg(theme.text_primary)
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center),
        );
        lines.push(Line::default());
        lines.push(
            Line::from(Span::styled(
                DEVICE_CODE_CAPTION,
                Style::default().fg(theme.gray),
            ))
            .alignment(Alignment::Center),
        );
    }
    if auth_url.is_some() {
        push_auth_copy_block(&mut lines, theme, clipboard_delivery);
    }
    lines.push(Line::default());
    lines.push(
        Line::from(Span::styled(waiting_text, Style::default().fg(theme.gray)))
            .alignment(Alignment::Center),
    );
    Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(Block::default().padding(Padding::horizontal(h_pad)))
        .render(msg_area, buf);

    let (click_rect, fallback_rect) = if auth_url.is_some() {
        auth_hit_rects(msg_area, h_pad, inner_width, header, code_extra)
    } else {
        (None, None)
    };

    let hints = Line::from(quit_hint_spans(theme)).alignment(Alignment::Center);
    Paragraph::new(hints).render(hint_area, buf);

    (click_rect, fallback_rect)
}

/// Render the welcome screen during authentication (Authenticating state).
#[allow(clippy::too_many_arguments)]
fn render_welcome_authenticating(
    content_area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    logo_line_count: u16,
    auth_url: Option<&str>,
    mode: AuthMode,
    auth_code_input: &str,
    auth_code_cursor_byte: usize,
    clipboard_delivery: Option<crate::clipboard::ClipboardDelivery>,
    show_raw_url: bool,
) -> (Option<Rect>, Option<Rect>) {
    let top_pad = content_area.height.saturating_sub(logo_line_count) / 10;

    match mode {
        AuthMode::Loopback => {
            // Manual token paste: show copy prompt + input box
            let h_pad: u16 = content_area.width / 6;
            let inner_width = content_area.width.saturating_sub(h_pad * 2).max(1);

            if show_raw_url {
                return render_raw_url_mode(
                    content_area,
                    buf,
                    theme,
                    top_pad,
                    logo_line_count,
                    auth_url,
                );
            }

            let msg_height = if auth_url.is_some() {
                let header_rows = (AUTH_HEADER.len() as u16).div_ceil(inner_width);
                header_rows + auth_copy_block_rows(inner_width)
            } else {
                1u16
            };
            let [_, logo_area, _, msg_area, _, prompt_area, _, hint_area, _] = Layout::vertical([
                Constraint::Length(top_pad),
                Constraint::Length(logo_line_count),
                Constraint::Length(1),          // gap
                Constraint::Length(msg_height), // instruction + copy prompt
                Constraint::Min(1),             // gap
                Constraint::Length(5),          // prompt box
                Constraint::Length(1),          // gap
                Constraint::Length(1),          // hints
                Constraint::Min(0),
            ])
            .areas(content_area);

            render_logo(
        logo_area,
        buf,
        theme,
        content_area.width,
        content_area.height,
    );

            // Instruction text
            let mut lines: Vec<Line> = Vec::new();
            if auth_url.is_some() {
                lines.push(
                    Line::from(Span::styled(
                        AUTH_HEADER,
                        Style::default().fg(theme.gray_bright),
                    ))
                    .alignment(Alignment::Center),
                );
                push_auth_copy_block(&mut lines, theme, clipboard_delivery);
            } else {
                lines.push(
                    Line::from(Span::styled(
                        "Waiting for auth URL...",
                        Style::default().fg(theme.gray),
                    ))
                    .alignment(Alignment::Center),
                );
            }
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(Block::default().padding(Padding::horizontal(h_pad)))
                .render(msg_area, buf);

            let (click_rect, fallback_rect) = if auth_url.is_some() {
                auth_hit_rects(msg_area, h_pad, inner_width, AUTH_HEADER, 0)
            } else {
                (None, None)
            };

            // Prompt box with token input
            let prompt_width = content_area.width;
            let [_, prompt_centered, _] = Layout::horizontal([
                Constraint::Min(0),
                Constraint::Length(prompt_width),
                Constraint::Min(0),
            ])
            .flex(Flex::Center)
            .areas(prompt_area);
            render_auth_input_box(
                prompt_centered,
                buf,
                theme,
                auth_code_input,
                auth_code_cursor_byte,
            );

            // Hints
            let mut hint_spans = vec![
                Span::styled(
                    "enter",
                    Style::default()
                        .fg(theme.accent_user)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  submit    ", Style::default().fg(theme.gray)),
            ];
            hint_spans.extend(quit_hint_spans(theme));
            let hints = Line::from(hint_spans).alignment(Alignment::Center);
            Paragraph::new(hints).render(hint_area, buf);

            (click_rect, fallback_rect)
        }

        AuthMode::Command => render_browser_status_arm(
            content_area,
            buf,
            theme,
            top_pad,
            logo_line_count,
            auth_url,
            show_raw_url,
            clipboard_delivery,
            BrowserStatusKind::Command,
        ),

        AuthMode::Device => render_browser_status_arm(
            content_area,
            buf,
            theme,
            top_pad,
            logo_line_count,
            auth_url,
            show_raw_url,
            clipboard_delivery,
            BrowserStatusKind::Device,
        ),

        AuthMode::Pending => {
            // Connecting: status text
            let [_, logo_area, _, msg_area, _, hint_area, _] = Layout::vertical([
                Constraint::Length(top_pad),
                Constraint::Length(logo_line_count),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(content_area);

            render_logo(
        logo_area,
        buf,
        theme,
        content_area.width,
        content_area.height,
    );

            let msg = Line::from(Span::styled(
                "Connecting...",
                Style::default().fg(theme.gray_bright),
            ))
            .alignment(Alignment::Center);
            Paragraph::new(msg).render(msg_area, buf);

            let hints = Line::from(quit_hint_spans(theme)).alignment(Alignment::Center);
            Paragraph::new(hints).render(hint_area, buf);

            (None, None)
        }
    }
}

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
    let has_resume_tip = !has_update_tip && p.foreign_resume_hint.is_some();
    // Tip slot precedence: pending update > resume hint > random tip.
    let tip_height = if !show_picker {
        if has_update_tip {
            1u16
        } else if has_resume_tip {
            1u16
        } else if let Some(tip_text) = p.tip {
            let inset = prompt::prompt_inset(welcome_compact);
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
            // Reserve a row for the pinned hidden-external hint when shown.
            let hint_row = u16::from(
                crate::views::session_picker::hidden_external_hint(
                    p.session_picker,
                    p.session_picker_source_filter,
                )
                .is_some(),
            );
            (picker_count as u16).min(15) + 3 + hint_row // +3 for title + search + gap
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
                tick: p.welcome_tick,
                grouped: p.session_picker_grouped,
                source_filter: p.session_picker_source_filter,
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
        );
        announcement_truncated = rects.announcement_truncated;
        announcement_rect = rects.announcement_rect;
        promo_cta_rect = rects.promo_cta_rect;
        (rects.menu_rects, None)
    };

    // Bottom tip slot: update banner > foreign-resume hint > random tip.
    if hero.tip.height > 0 {
        let [_, tip_centered, _] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(content_area.width),
            Constraint::Min(0),
        ])
        .flex(Flex::Center)
        .areas(hero.tip);
        let inset = prompt::prompt_inset(p.compact);
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
        } else if let Some(hint) = p.foreign_resume_hint {
            // Recent foreign session: offer a one-click resume in the tip area
            // (only when no update is pending — the update shares ctrl+u and wins).
            let mins = hint.age.as_secs() / 60;
            let when = if mins == 0 {
                "moments ago".to_string()
            } else {
                format!("{mins}m ago")
            };
            let accent = Style::default().fg(theme.accent_user);
            let accent_bold = accent.add_modifier(Modifier::BOLD);
            let tool = crate::app::foreign_tool_display_label(hint.tool);
            let line = Line::from(vec![
                Span::styled("Coming from ", accent),
                Span::styled(tool, accent_bold),
                Span::styled(format!("? Resume your session from {when} using "), accent),
                Span::styled("ctrl+u", accent_bold),
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
        auth_url_rect: None,
        auth_fallback_rect: None,
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
    pub(crate) content_results:
        Option<&'a [grow_shell::extensions::session_search::SearchSessionHit]>,
    pub(crate) content_loading: bool,
    /// The query `sessions` were server-fetched with (see
    /// [`crate::views::session_picker::effective_filter_query`]).
    pub(crate) entries_query: Option<&'a str>,
    pub(crate) tick: u64,
    /// When true, entries are grouped by `repo_name` with non-selectable headers.
    pub(crate) grouped: bool,
    /// Source filter for filtering session entries.
    pub(crate) source_filter: crate::views::session_picker::SourceFilter,
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

    // Filter entries by query and source (shared helper). The same effective
    // query must drive filtering AND the content header/rows gates below, or
    // this render disagrees with `handle_welcome_input`'s `build_entry_map`
    // (which receives the effective query) on row indices.
    let filter_query =
        crate::views::session_picker::effective_filter_query(ctx.state.query(), ctx.entries_query);
    let filtered_indices =
        crate::app::app_view::filter_session_entries(ctx.sessions, filter_query, ctx.source_filter);

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
        && ctx.source_filter != crate::views::session_picker::SourceFilter::External
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
    let content_loading = ctx.content_loading
        && ctx.source_filter != crate::views::session_picker::SourceFilter::External;
    let spinner_label = build_content_header_label(content_loading, has_content_rows, ctx.tick);
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

    let hidden_hint =
        crate::views::session_picker::hidden_external_hint(ctx.sessions, ctx.source_filter);

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
        filter_label: Some(ctx.source_filter.label()),
        filter_key_hint: Some("f"),
        filter_active: ctx.source_filter.is_active(),
        header_note: hidden_hint.as_deref(),
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
        ctx.tick,
    )
}

/// Render the auth token input box (loopback mode).
fn render_auth_input_box(
    area: Rect,
    buf: &mut Buffer,
    theme: &Theme,
    input: &str,
    cursor_byte: usize,
) {
    let prompt_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent_user))
        .padding(Padding {
            left: 2,
            right: 1,
            top: 0,
            bottom: 0,
        });
    let inner = prompt_block.inner(area);
    prompt_block.render(area, buf);

    if inner.height > 0 && inner.width > 2 {
        let prompt = crate::glyphs::prompt_arrow();
        let prompt_width = prompt.width() as u16;
        let input_width = inner.width.saturating_sub(prompt_width);
        let (display, cursor_column) =
            masked_auth_token_view(input, cursor_byte, input_width as usize);

        let style = if input.is_empty() {
            Style::default().fg(theme.gray_dim)
        } else {
            Style::default().fg(theme.accent_user)
        };

        let line = Line::from(vec![
            Span::styled(prompt, Style::default().fg(theme.accent_user)),
            Span::styled(display, style),
        ]);
        buf.set_line(inner.x, inner.y, &line, inner.width);
        if input_width > 0 {
            let cursor_x = inner.x + prompt_width + cursor_column as u16;
            if let Some(cell) = buf.cell_mut((cursor_x, inner.y)) {
                cell.set_style(Style::default().fg(theme.bg_base).bg(theme.text_primary));
            }
        }
    }
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

fn auth_token_grapheme_visible(index: usize, total: usize) -> bool {
    total <= 8 || index + 4 >= total
}

struct MaskedAuthToken {
    display: String,
    cursor_byte: usize,
}

fn build_masked_auth_token(input: &str, cursor_byte: usize) -> MaskedAuthToken {
    let graphemes: Vec<(usize, &str)> = input.grapheme_indices(true).collect();
    let total = graphemes.len();
    let mut display = String::new();
    let mut mapped_cursor = None;
    for (index, (byte, grapheme)) in graphemes.into_iter().enumerate() {
        if byte == cursor_byte {
            mapped_cursor = Some(display.len());
        }
        if auth_token_grapheme_visible(index, total) {
            display.push_str(grapheme);
        } else {
            display.push('\u{2022}');
        }
    }
    MaskedAuthToken {
        cursor_byte: mapped_cursor.unwrap_or(display.len()),
        display,
    }
}

fn masked_auth_token_view(input: &str, cursor_byte: usize, width: usize) -> (String, usize) {
    if input.is_empty() {
        return ("Paste your token here...".to_string(), 0);
    }
    let masked = build_masked_auth_token(input, cursor_byte);
    let buffer =
        xai_ratatui_textarea::EditBuffer::from_parts(masked.display.as_str(), masked.cursor_byte);
    let viewport = buffer.single_line_viewport(width);
    (
        masked.display[viewport.visible_byte_range].to_owned(),
        viewport.cursor_display_column,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::app_view::SessionPickerEntry;
    use crate::views::picker::PickerState;
    use crate::views::session_picker::{build_grouped_picker_entries, build_session_entry_data};

    #[test]
    fn auth_copy_feedback_covers_delivery_states() {
        let theme = Theme::current();
        for (delivery, expected) in [
            (crate::clipboard::ClipboardDelivery::Confirmed, "copied!"),
            (
                crate::clipboard::ClipboardDelivery::Unverified,
                "copy sent—verify paste",
            ),
            (crate::clipboard::ClipboardDelivery::Failed, "copy failed"),
        ] {
            let mut lines = Vec::new();
            push_auth_copy_block(&mut lines, &theme, Some(delivery));
            let feedback = lines[3]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert_eq!(feedback, expected);
        }
    }

    #[test]
    fn masked_auth_token_preserves_reveal_policy() {
        assert_eq!(
            masked_auth_token_view("", 0, 24),
            ("Paste your token here...".to_string(), 0)
        );
        assert_eq!(build_masked_auth_token("12345678", 8).display, "12345678");
        assert_eq!(build_masked_auth_token("123456789", 9).display, "•••••6789");

        let input = "abcdefghMIDDLEwxyz";
        let masked = build_masked_auth_token(input, input.len()).display;
        assert!(masked.starts_with("••••"));
        assert!(masked.ends_with("wxyz"));
        assert!(!masked.contains("MIDDLE"));
        assert!(masked.contains("\u{2022}"));

        let input = "测试令牌一二三四五六七八九十";
        let masked = build_masked_auth_token(input, input.len()).display;
        assert!(masked.starts_with("••••"));
        assert!(masked.contains("\u{2022}"));
    }

    #[test]
    fn masked_auth_mapping_handles_zero_width_combining_and_zwj_middle() {
        let prefix = "abcdefgh";
        let hidden = "\u{200b}e\u{301}👩🏽\u{200d}💻MID";
        let suffix = "wxyz";
        let token = format!("{prefix}{hidden}{suffix}");
        let before = prefix.len();
        let inside = prefix.len() + "\u{200b}e\u{301}".len();
        let after = prefix.len() + hidden.len();
        let expected = format!("{}{}", "\u{2022}".repeat(14), suffix);

        let before_masked = build_masked_auth_token(&token, before);
        let inside_masked = build_masked_auth_token(&token, inside);
        let after_masked = build_masked_auth_token(&token, after);
        assert_eq!(before_masked.display, expected);
        assert_eq!(inside_masked.display, expected);
        assert_eq!(after_masked.display, expected);
        assert_eq!(before_masked.cursor_byte, "\u{2022}".len() * 8);
        assert_eq!(inside_masked.cursor_byte, "\u{2022}".len() * 10);
        assert_eq!(after_masked.cursor_byte, "\u{2022}".len() * 14);

        for width in [1, 2, 5] {
            for cursor in [before, inside, after] {
                let (view, cursor_column) = masked_auth_token_view(&token, cursor, width);
                assert!(view.width() <= width);
                assert!(cursor_column < width);
                assert!(!view.contains('\u{200b}'));
                assert!(!view.contains("e\u{301}"));
                assert!(!view.contains("👩🏽\u{200d}💻"));
                assert!(!view.contains("MID"));
            }
        }

        let wide_prefix = "中bcdefgh";
        let wide_token = format!("{wide_prefix}HIDDEN{suffix}");
        let (_, cursor_column) = masked_auth_token_view(&wide_token, wide_prefix.len(), 40);
        assert_eq!(cursor_column, wide_prefix.graphemes(true).count());
    }

    #[test]
    fn masked_auth_render_keeps_narrow_caret_visible() {
        let token = "abcdefghSECRET-MIDDLEwxyz";
        let cursor = "abcdefghSECRET".len();
        let area = Rect::new(0, 0, 9, 3);
        let theme = Theme::current();
        let mut buffer = Buffer::empty(area);
        render_auth_input_box(area, &mut buffer, &theme, token, cursor);
        assert!((0..area.width).any(|x| buffer[(x, 1)].bg == theme.text_primary));
    }

    fn make_entry(id: &str, summary: &str, repo_name: &str) -> SessionPickerEntry {
        SessionPickerEntry {
            id: id.into(),
            summary: summary.into(),
            updated_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            cwd: format!("/home/user/{repo_name}"),
            hostname: None,
            source: "local".into(),
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
        auth_state: &'a AuthState,
        trust_state: &'a TrustState,
        session_picker: Option<&'a [SessionPickerEntry]>,
    ) -> WelcomeRenderParams<'a> {
        WelcomeRenderParams {
            auth_state,
            trust_state,
            login_label: None,
            auth_code_input: "",
            auth_code_cursor_byte: 0,
            clipboard_delivery: None,
            show_raw_url: false,
            announcement: None,
            tip: None,
            model_name: "test",
            flags: &[],
            selected: None,
            team_name: None,
            has_claude_import: false,
            mouse_pos: None,
            is_zdr_blocked: false,
            session_picker,
            session_picker_loading: false,
            compact: false,
            pending_hint: None,
            startup_warnings: &[],
            pending_update_version: None,
            foreign_resume_hint: None,
            session_picker_content_results: None,
            session_picker_content_loading: false,
            session_picker_entries_query: None,
            welcome_tick: 0,
            session_picker_grouped: false,
            session_picker_source_filter: crate::views::session_picker::SourceFilter::default(),
            cwd: std::path::Path::new("/repo"),
            welcome_announcement_expanded: false,
            promo_cta: None,
        }
    }

    fn render_done_text(params: &WelcomeRenderParams<'_>) -> String {
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut prompt = PromptWidget::new();
        let mut picker = PickerState::default();
        render_welcome(area, &mut buf, params, &mut prompt, &mut picker);
        buffer_text(&buf)
    }

    #[test]
    fn foreign_resume_tip_names_each_tool_and_age() {
        use grow_workspace::foreign_sessions::ForeignSessionTool;

        let auth = AuthState::Done;
        let trust = TrustState::Done;
        for (tool, label) in [
            (ForeignSessionTool::Claude, "Claude Code"),
            (ForeignSessionTool::Codex, "Codex"),
            (ForeignSessionTool::Cursor, "Cursor"),
        ] {
            let hint = grow_workspace::foreign_sessions::RecentForeignSession {
                tool,
                native_id: "native-id".into(),
                age: std::time::Duration::from_secs(125),
            };
            let mut params = render_params(&auth, &trust, None);
            params.foreign_resume_hint = Some(&hint);
            let text = render_done_text(&params);
            assert!(text.contains(&format!("Coming from {label}?")), "{text}");
            assert!(text.contains("2m ago"), "{text}");
            assert!(text.contains("ctrl+u"), "{text}");
        }
    }

    #[test]
    fn pending_update_suppresses_foreign_resume_tip() {
        let auth = AuthState::Done;
        let trust = TrustState::Done;
        let hint = grow_workspace::foreign_sessions::RecentForeignSession {
            tool: grow_workspace::foreign_sessions::ForeignSessionTool::Cursor,
            native_id: "native-id".into(),
            age: std::time::Duration::from_secs(30),
        };
        let mut params = render_params(&auth, &trust, None);
        params.foreign_resume_hint = Some(&hint);
        params.pending_update_version = Some("9.9.9");

        let text = render_done_text(&params);
        assert!(text.contains("v9.9.9 available"), "{text}");
        assert!(!text.contains("Coming from Cursor?"), "{text}");
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
    fn authenticating_welcome_returns_paired_overlay_clear() {
        let _guard = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        crate::terminal::overlay::reset_owner();
        seed_static_owner(81);
        let auth_state = AuthState::Authenticating {
            request_seq: 1,
            handle: None,
            auth_url: None,
            mode: AuthMode::Command,
        };
        let trust_state = TrustState::Done;
        let params = render_params(&auth_state, &trust_state, None);
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut prompt = PromptWidget::new();
        let mut picker = PickerState::default();

        let result = render_welcome(area, &mut buf, &params, &mut prompt, &mut picker);
        assert_promptless_clear(result, 81);
    }

    #[test]
    fn picker_welcome_returns_paired_overlay_clear() {
        let _guard = crate::terminal::image::set_protocol_for_test(
            crate::terminal::image::GraphicsProtocol::Kitty,
        );
        crate::terminal::overlay::reset_owner();
        seed_static_owner(82);
        let auth_state = AuthState::Done;
        let trust_state = TrustState::Done;
        let sessions = [make_entry("session-1", "summary", "repo")];
        let params = render_params(&auth_state, &trust_state, Some(&sessions));
        let area = Rect::new(0, 0, 100, 40);
        let mut buf = Buffer::empty(area);
        let mut prompt = PromptWidget::new();
        let mut picker = PickerState::default();

        let result = render_welcome(area, &mut buf, &params, &mut prompt, &mut picker);
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
                    tick: 0,
                    grouped: false,
                    source_filter: crate::views::session_picker::SourceFilter::default(),
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

    /// The hidden-external hint stays pinned on the welcome picker's default
    /// Grow view when scanned foreign rows exist — even when the native list
    /// overflows the viewport.
    #[test]
    fn hidden_external_hint_stays_pinned() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;

        let theme = crate::theme::Theme::default();
        let area = Rect::new(0, 0, 80, 20);
        // More native rows than the viewport fits: a trailing list row would
        // scroll out of view, a pinned row must not.
        let mut entries: Vec<SessionPickerEntry> = (0..30)
            .map(|i| make_entry(&format!("s{i}"), &format!("native session {i}"), "repo"))
            .collect();
        let mut foreign = make_entry("f1", "Claude work", "repo");
        foreign.source = "claude".into();
        entries.push(foreign);

        let render = || -> String {
            let mut buf = Buffer::empty(area);
            let mut state = PickerState::default();
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
                    content_loading: false,
                    entries_query: None,
                    tick: 0,
                    grouped: false,
                    source_filter: crate::views::session_picker::SourceFilter::default(),
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

        let build_mode = render();
        assert!(
            build_mode.contains("1 external session hidden \u{b7} f to show"),
            "default Grow filter must pin the hidden-external hint:\n{build_mode}"
        );
        assert!(
            build_mode.find("external session hidden") < build_mode.find("native session 0"),
            "the hint must be pinned above the first list row:\n{build_mode}"
        );
        assert!(
            !build_mode.contains("Claude work"),
            "the foreign row itself stays hidden under the default filter:\n{build_mode}"
        );
    }

    #[test]
    fn grouped_entries_insert_headers() {
        let entries = vec![
            make_entry("s1", "Fix auth", "xai"),
            make_entry("s2", "Add streaming", "xai"),
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
        // Groups are sorted alphabetically: fw-1 before xai.
        // Header positions: 0 (fw-1), 2 (xai)
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
            matches!(&result[2], crate::views::picker::PickerEntry::Header { label } if label == &"xai")
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
            make_entry("s1", "Fix auth", "xai"),
            make_entry("s2", "Add streaming", "xai"),
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
        let entries = vec![make_entry("s1", "Fix auth", "xai")];
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
        let auth = AuthState::Done;
        let trust = TrustState::Done;
        let text = render_done_text(&render_params(&auth, &trust, None));
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
        let auth = AuthState::Done;
        let trust = TrustState::Done;
        let mut params = render_params(&auth, &trust, None);
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
        let auth = AuthState::Done;
        let trust = TrustState::Done;
        let text = render_done_text(&render_params(&auth, &trust, None));
        assert!(!text.contains("Changelog"), "{text}");
    }

    #[test]
    fn blocked_layout_keeps_logo_menu_and_version() {
        // The login / ZDR / trust screens still render the stacked layout
        // (logo + menu + prompt + bottom version) even on wide terminals.
        let area = Rect::new(0, 0, 120, 40);
        let layout = WelcomeLayout::compute_stacked(WelcomeLayoutInput {
            content_area: area,
            menu_height: 2,
            ..Default::default()
        });
        assert!(layout.logo.height > 0, "gate screens keep the logo");
        assert!(layout.menu.height > 0);
        assert!(layout.prompt.height > 0);
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
    fn extract_user_code_parses_verification_url() {
        assert_eq!(
            extract_user_code("https://login.example.com/oauth2/device?user_code=ABCD-EFGH"),
            Some("ABCD-EFGH"),
        );
        // Trailing params after the code are ignored.
        assert_eq!(
            extract_user_code("https://example.com/oauth2/device?user_code=WXYZ-1234&foo=bar"),
            Some("WXYZ-1234"),
        );
        // A param whose name merely ends in `user_code` must not be matched.
        assert_eq!(
            extract_user_code("https://example.com/d?foo_user_code=BAD&user_code=GOOD"),
            Some("GOOD"),
        );
        // No code param, empty code, and unexpected characters all yield None.
        assert_eq!(extract_user_code("https://example.com/oauth2/device"), None);
        assert_eq!(extract_user_code("https://example.com/d?user_code="), None);
        assert_eq!(
            extract_user_code("https://example.com/d?user_code=AB%20CD"),
            None
        );
    }

    #[test]
    fn device_auth_arm_shows_url_and_no_paste_box() {
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let theme = Theme::current();
        let url = "https://login.example.com/oauth2/device?user_code=ABCD-EFGH";

        let (copy_rect, fallback_rect) = render_welcome_authenticating(
            area,
            &mut buf,
            &theme,
            logo_line_count(area.width, area.height),
            Some(url),
            AuthMode::Device,
            "", // auth_code_input — unused in device mode
            0,
            None,  // clipboard_delivery
            false, // show_raw_url
        );

        let text = buffer_text(&buf);
        assert!(
            text.contains("Approve in your browser"),
            "device arm must show the approval header, got:\n{text}"
        );
        // Device code shown for the browser-match check (anti-phishing).
        assert!(
            text.contains("ABCD-EFGH"),
            "device arm must show the device code, got:\n{text}"
        );
        assert!(
            text.contains("Make sure your browser shows this code"),
            "device arm must show the code caption, got:\n{text}"
        );
        // Copy affordance (click-to-copy line) is present.
        assert!(
            text.contains("to copy"),
            "device arm must show the copy-URL affordance, got:\n{text}"
        );
        // No manual-paste affordance in device mode.
        assert!(
            !text.contains("Paste your token"),
            "device arm must NOT render the token paste box, got:\n{text}"
        );
        // Copy + fallback links are clickable.
        assert!(
            copy_rect.is_some(),
            "device arm must expose a copy hit-rect"
        );
        assert!(
            fallback_rect.is_some(),
            "device arm must expose a show-full-URL hit-rect"
        );
    }

    #[test]
    fn device_auth_arm_raw_url_mode_shows_full_url() {
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let theme = Theme::current();
        let url = "https://login.example.com/oauth2/device?user_code=WXYZ-1234";

        render_welcome_authenticating(
            area,
            &mut buf,
            &theme,
            logo_line_count(area.width, area.height),
            Some(url),
            AuthMode::Device,
            "",
            0,
            None,
            true, // show_raw_url
        );

        let text = buffer_text(&buf);
        assert!(
            text.contains("WXYZ-1234"),
            "raw URL mode must render the full URL including the user code, got:\n{text}"
        );
    }

    #[test]
    fn raw_url_mode_centers_url_that_fits_on_one_line() {
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let theme = Theme::current();
        let url = "https://login.example.com/oauth2/device?user_code=WXYZ-1234";

        render_welcome_authenticating(
            area,
            &mut buf,
            &theme,
            logo_line_count(area.width, area.height),
            Some(url),
            AuthMode::Device,
            "",
            0,
            None,
            true, // show_raw_url
        );

        let text = buffer_text(&buf);
        let url_line = text
            .lines()
            .find(|l| l.contains("https://"))
            .expect("raw URL mode must render the URL");
        // Whole URL on one line, not wrapped.
        assert!(url_line.contains(url), "URL must be intact: {url_line:?}");
        // Centered: leading pad within 1 cell of trailing pad (integer split).
        let lead = url_line.len() - url_line.trim_start().len();
        let trail = url_line.len() - url_line.trim_end().len();
        assert!(
            lead > 0 && lead.abs_diff(trail) <= 1,
            "URL must be horizontally centered, lead={lead} trail={trail}:\n{text}"
        );
    }

    #[test]
    fn raw_url_mode_uses_full_width_for_long_urls() {
        let area = Rect::new(0, 0, 40, 40);
        let mut buf = Buffer::empty(area);
        let theme = Theme::current();
        // 40-col terminal; URL longer than one row must wrap at the exact
        // screen edge with no leading spaces so copy-paste stays intact.
        let url = "https://login.example.com/oauth2/device?user_code=WXYZ-1234&extra=0123456789";

        render_welcome_authenticating(
            area,
            &mut buf,
            &theme,
            logo_line_count(area.width, area.height),
            Some(url),
            AuthMode::Device,
            "",
            0,
            None,
            true, // show_raw_url
        );

        let text = buffer_text(&buf);
        let mut lines = text.lines();
        let first = lines
            .by_ref()
            .find(|l| l.contains("https://"))
            .expect("raw URL mode must render the URL");
        let second = lines.next().expect("URL must wrap to a second row");
        // First row flush against both edges (full width), remainder on the
        // next row starting at column 0.
        assert_eq!(
            first,
            &url[..40],
            "long URL row must span the full terminal width:\n{text}"
        );
        assert!(
            second.starts_with(&url[40..]),
            "wrapped remainder must start at column 0:\n{text}"
        );
    }

    #[test]
    fn command_auth_arm_shows_url_and_waiting() {
        let area = Rect::new(0, 0, 80, 40);
        let mut buf = Buffer::empty(area);
        let theme = Theme::current();
        let url = "https://login.example.com/oauth2/authorize?client_id=grow";

        let (copy_rect, fallback_rect) = render_welcome_authenticating(
            area,
            &mut buf,
            &theme,
            logo_line_count(area.width, area.height),
            Some(url),
            AuthMode::Command,
            "", // auth_code_input — unused
            0,
            None,  // clipboard_delivery
            false, // show_raw_url
        );

        let text = buffer_text(&buf);
        assert!(
            text.contains("A browser window will open"),
            "command arm must show the auth header, got:\n{text}"
        );
        assert!(
            text.contains("Waiting for login to complete"),
            "command arm must show the waiting status, got:\n{text}"
        );
        // No device code — that's device-flow only.
        assert!(
            !text.contains("Make sure your browser shows this code"),
            "command arm must NOT show the device-code caption, got:\n{text}"
        );
        // No manual-paste affordance in command mode.
        assert!(
            !text.contains("Paste your token"),
            "command arm must NOT render the token paste box, got:\n{text}"
        );
        // Copy + fallback links are clickable.
        assert!(
            copy_rect.is_some(),
            "command arm must expose a copy hit-rect"
        );
        assert!(
            fallback_rect.is_some(),
            "command arm must expose a show-full-URL hit-rect"
        );
    }

    #[test]
    fn home_page_renders_announcement_in_hero_info_slot() {
        // A wide terminal shows the announcement inside the hero text group
        // (the info slot), and the clickable block rect is reported.
        let auth = AuthState::Done;
        let trust = TrustState::Done;
        let mut params = render_params(&auth, &trust, None);
        let a = grow_announcements::Announcement {
            title: Some("Security policy".into()),
            message: Some("Report security incidents to the security team.".into()),
            ..Default::default()
        };
        params.announcement = Some(&a);
        let area = Rect::new(0, 0, 150, 50);
        let mut buf = Buffer::empty(area);
        let mut prompt = PromptWidget::new();
        let mut picker = PickerState::default();
        let result = render_welcome(area, &mut buf, &params, &mut prompt, &mut picker);
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
}
