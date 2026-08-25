//! Minimal-mode plan-approval host (design PR10).
//!
//! The full TUI renders plan approval as a fullscreen line-viewer plus a live
//! feedback prompt. Minimal takes a simpler route: the **whole plan is committed
//! into native scrollback** as a normal conversation block (see
//! [`maybe_commit_plan`]), so it reads and scrolls exactly like the rest of the
//! transcript. The prompt-anchored live region then holds only the decision
//! controls — approve / revise / keep planning — plus the feedback input when
//! revising. Nothing of the plan body is drawn under the prompt.
//!
//! Input routing is unchanged: while `line_viewer.is_some()` the agent's input
//! handler already routes keys to `handle_line_viewer_key` (Preview focus:
//! `a` approve / `s`/`Tab` revise / `q` keep planning) and `handle_plan_feedback_key`
//! (Prompt focus: type feedback, `Enter` send, `Esc` back). Minimal keeps the
//! line viewer open (so those keys fire) but renders this compact controls strip
//! in place of the never-drawn fullscreen viewer.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

use pager::app::agent_view::AgentView;
use pager::app::root::{ActiveView, AppView};
use pager::minimal_api;
use pager::scrollback::block::RenderBlock;
use pager::theme::Theme;
use pager::views::plan_approval_view::PlanApprovalFocus;
use pager::views::prompt_widget::{PromptBg, PromptStyle};

/// The active plan-approval focus, defaulting to `Preview`.
fn focus(agent: &AgentView) -> PlanApprovalFocus {
    minimal_api::plan_approval_view(agent)
        .map(|p| p.focus)
        .unwrap_or(PlanApprovalFocus::Preview)
}

/// Controls-strip header for the parked plan-approval surface.
const PLAN_HEADER: &str = "Plan ready for review";

/// Commit the active plan into native scrollback, once per plan (and once per
/// revision).
///
/// Minimal has no separate plan pane: the terminal's scrollback *is* the
/// history, so the plan is pushed as an ordinary finalized agent-message block
/// and printed into native scrollback by the normal commit pass — leaving only
/// the decision controls under the prompt. De-duplicated by the plan's
/// `tool_call_id`; a revised plan arrives as a fresh PlanControl call with a new id
/// and is committed as its own block. The PlanControl protocol rejects empty plans
/// before an approval view can be created.
///
/// The block is anchored **above** the still-running `exit_plan_mode` tool row,
/// not appended after it, so the commit frontier reaches the plan while the
/// approval is still parked. Users reported losing the head of a plan to the
/// clipped live tail; design doc §6.16 has the full argument and the rejected
/// alternatives.
///
/// NOTE (draw-path state mutation + replay durability): this pushes into
/// `ScrollbackState` from the render path — a deliberate exception, since the
/// plan block must enter the normal commit pipeline. A pending approval is
/// durable: Shell reloads the content-addressed plan artifact and reissues the
/// approval request, so Minimal commits it again. Once approval has settled,
/// the block remains client render state and is not reconstructed in a fresh
/// `/transcript`; the durable artifact/control snapshot and Todo replay are the
/// authoritative execution state. Do not turn the ACP Plan projection into a
/// second transcript persistence mechanism.
///
/// Call once per frame from [`crate::draw`], before the commit pass.
pub fn maybe_commit_plan(app: &mut AppView) {
    let ActiveView::Agent(id) = minimal_api::app_active_view(app) else {
        return;
    };
    let id = *id;

    // Extract the plan (owned) under a short immutable borrow so the mutable
    // scrollback push and the `minimal_state` read/write below don't overlap it.
    let plan = minimal_api::app_agent(app, id).and_then(|agent| {
        minimal_api::plan_approval_view(agent)
            .map(|pav| (pav.tool_call_id.clone(), pav.plan_content.clone()))
    });
    let Some((tool_call_id, content)) = plan else {
        return;
    };

    if minimal_api::minimal_committed_plan_id(app) == Some(tool_call_id.as_str()) {
        return; // already emitted this plan
    }

    // Mark the plan as emitted only when the block was actually pushed: the
    // agent borrow can't fail here (the plan was just extracted from it), but
    // if it ever did, stamping the id anyway would treat the plan as committed
    // while nothing ever reaches native scrollback.
    if let Some(agent) = minimal_api::app_agent_mut(app, id) {
        let block = RenderBlock::agent_message(content);
        // No anchor (the tool was reaped): append, and the plan commits at turn
        // end — the pre-fix behavior, still better than dropping it.
        match minimal_api::pending_tool_entry_id(agent, &tool_call_id) {
            Some(anchor) => {
                minimal_api::agent_scrollback_mut(agent).insert_block_before(anchor, block);
            }
            None => {
                minimal_api::agent_scrollback_mut(agent).push_block(block);
            }
        }
        minimal_api::set_minimal_committed_plan_id(app, Some(tool_call_id));
    }
}

/// Desired controls-strip height: header + controls + optional feedback input.
pub fn height(agent: &AgentView) -> u16 {
    let input = if focus(agent) == PlanApprovalFocus::Prompt {
        1
    } else {
        0
    };
    // header (1) + controls (1) + input (0/1)
    2u16.saturating_add(input)
}

/// Render the compact plan-approval controls strip into `area`. The plan itself
/// lives in native scrollback ([`maybe_commit_plan`]); this only draws the
/// header, the decision hint, and — when revising — the feedback input. Returns
/// the text cursor when the feedback input is focused, else `None`.
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    agent: &mut AgentView,
    theme: &Theme,
) -> Option<(u16, u16)> {
    if area.height == 0 || area.width < 4 {
        return None;
    }
    let foc = focus(agent);
    let input_h: u16 = if foc == PlanApprovalFocus::Prompt {
        1
    } else {
        0
    };

    // header (1) · controls (1) · input (0/1)
    let controls_y = (area.y + area.height).saturating_sub(1 + input_h);

    // ── header ──
    let header_style = Style::default()
        .fg(theme.accent_user)
        .bg(Color::Reset)
        .add_modifier(Modifier::BOLD);
    buf.set_style(
        Rect { height: 1, ..area },
        Style::default().bg(Color::Reset),
    );
    buf.set_span(
        area.x,
        area.y,
        &Span::styled(PLAN_HEADER, header_style),
        area.width,
    );

    // ── controls hint ──
    let has_content = minimal_api::plan_approval_view(agent)
        .map(|p| !p.comments.is_empty())
        .unwrap_or(false)
        || !minimal_api::agent_prompt(agent).text().trim().is_empty();
    // Tab reopens the submitted plan preview.
    let hint = match foc {
        PlanApprovalFocus::Prompt if has_content => {
            "enter request changes \u{00b7} tab plan \u{00b7} esc back"
        }
        PlanApprovalFocus::Prompt => "enter approve \u{00b7} tab plan \u{00b7} esc back",
        PlanApprovalFocus::Commenting => "enter save comment \u{00b7} esc cancel",
        PlanApprovalFocus::Preview => "a approve \u{00b7} s revise \u{00b7} q keep planning",
    };
    let hint_style = theme.dim().bg(Color::Reset);
    let controls_rect = Rect {
        x: area.x,
        y: controls_y,
        width: area.width,
        height: 1,
    };
    buf.set_style(controls_rect, hint_style);
    buf.set_span(
        area.x,
        controls_y,
        &Span::styled(hint, hint_style),
        area.width,
    );

    // ── feedback input (revise mode) ──
    if input_h > 0 {
        let row = Rect {
            x: area.x,
            y: (area.y + area.height).saturating_sub(1),
            width: area.width,
            height: 1,
        };
        let style = input_style(theme);
        buf.set_style(row, Style::default().bg(theme.bg_visual));
        return minimal_api::agent_prompt_mut(agent)
            .draw(buf, row, None, &style, None)
            .cursor_pos;
    }
    None
}

/// Chromeless prompt style for the feedback editor (the modal supplies framing).
fn input_style(theme: &Theme) -> PromptStyle {
    PromptStyle {
        focused: true,
        show_prefix: false,
        vpad_top: 0,
        compact: false,
        chrome: false,
        chrome_pad_left: 0,
        chrome_pad_right: 0,
        bg: PromptBg::Panel(theme.bg_visual),
        accent_color_override: None,
        border_color_override: None,
        prefix_override: None,
        placeholder_override: None,
        show_accent_line: false,
        show_borders: false,
        title: None,
        image_preview: true,
    }
}
