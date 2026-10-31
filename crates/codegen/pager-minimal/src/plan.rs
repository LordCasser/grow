//! Minimal-mode plan-approval host (design PR10).
//!
//! The full TUI renders plan approval as a fullscreen line-viewer plus a live
//! feedback prompt. Minimal takes a simpler route: the **whole plan is committed
//! into native scrollback** as a normal conversation block (see
//! [`maybe_commit_plan`]), so it reads and scrolls exactly like the rest of the
//! transcript. The prompt-anchored live region then holds only the decision
//! controls — approve / revise / defer / abandon — plus the feedback input when
//! revising. Nothing of the plan body is drawn under the prompt.
//!
//! Input routing is unchanged: while `line_viewer.is_some()` the agent's input
//! handler already routes keys to `handle_line_viewer_key` (Preview focus:
//! `a` approve / `s`/`Tab` revise / `l` defer / `q` abandon) and `handle_plan_feedback_key`
//! (Prompt focus: type feedback, `Enter` send, `Esc` collapse). Minimal keeps the
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

fn control_shortcuts(agent: &AgentView) -> Vec<pager::views::modal_window::Shortcut<'static>> {
    use pager::views::modal_window::Shortcut;

    let pending = minimal_api::agent_plan_decision_pending(agent)
        || minimal_api::plan_approval_view(agent).is_some_and(|pav| pav.decision_pending());
    let has_viewer = minimal_api::agent_line_viewer(agent).is_some();
    let labels: Vec<&'static str> = if pending {
        if has_viewer {
            vec!["r check confirmation", "Esc collapse"]
        } else {
            vec!["r check confirmation"]
        }
    } else {
        match focus(agent) {
            PlanApprovalFocus::Prompt => {
                let mut labels = vec![
                    if !minimal_api::agent_prompt(agent).text().trim().is_empty()
                        || minimal_api::plan_approval_view(agent)
                            .is_some_and(|pav| !pav.comments.is_empty())
                    {
                        "Enter request changes"
                    } else {
                        "a approve"
                    },
                    "Tab plan",
                ];
                if has_viewer {
                    labels.push("Esc collapse");
                }
                labels
            }
            PlanApprovalFocus::Commenting => {
                vec!["Enter save comment", "Esc cancel", "Tab discard comment"]
            }
            PlanApprovalFocus::Preview if has_viewer => vec![
                "a approve",
                "s revise",
                "l later",
                "q abandon",
                "c comment",
                "y copy plan",
                "Esc collapse",
            ],
            PlanApprovalFocus::Preview => vec!["a approve", "Tab feedback"],
        }
    };
    labels
        .into_iter()
        .enumerate()
        .map(|(id, label)| Shortcut {
            label,
            clickable: false,
            id,
        })
        .collect()
}

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
/// The block is anchored **above** the still-running `plan_control` tool row,
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
    let plan = minimal_api::app_visible_agent(app, id).and_then(|agent| {
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
    if let Some(agent) = minimal_api::app_visible_agent_mut(app, id) {
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
pub fn height(agent: &AgentView, width: u16) -> u16 {
    let input = if matches!(
        focus(agent),
        PlanApprovalFocus::Prompt | PlanApprovalFocus::Commenting
    ) {
        1
    } else {
        0
    };
    let shortcuts = control_shortcuts(agent);
    1u16 + pager::views::modal_window::shortcuts_rows_needed(&shortcuts, width) + input
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
    let input_h: u16 = if matches!(
        foc,
        PlanApprovalFocus::Prompt | PlanApprovalFocus::Commenting
    ) {
        1
    } else {
        0
    };

    let shortcuts = control_shortcuts(agent);
    let hint_rows = pager::views::modal_window::shortcuts_rows_needed(&shortcuts, area.width);
    let controls_y = (area.y + area.height).saturating_sub(hint_rows + input_h);

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

    // ── controls hints ──
    let controls_rect = Rect {
        x: area.x,
        y: controls_y,
        width: area.width,
        height: hint_rows,
    };
    buf.set_style(controls_rect, theme.dim().bg(Color::Reset));
    pager::views::modal_window::render_modal_shortcuts(buf, controls_rect, &shortcuts, None, theme);

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

/// Render an ordinary or retained Plan preview in Minimal's live region.
pub fn render_preview(
    buf: &mut Buffer,
    area: Rect,
    agent: &mut AgentView,
    theme: &Theme,
) -> Option<(u16, u16)> {
    let cwd = minimal_api::agent_cwd(agent).to_path_buf();
    let comment_count = minimal_api::agent_plan_comment_count(agent);
    let viewer = minimal_api::agent_line_viewer_mut(agent)?;
    pager::views::file_search::line_viewer::render_line_viewer(
        buf,
        area,
        viewer,
        &cwd,
        theme,
        comment_count,
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pager::views::file_search::line_viewer::{LineViewerKind, LineViewerState};
    use ratatui::layout::Rect;

    fn agent() -> AgentView {
        let mut agent = minimal_api::test_agent_view(Some("test-session"), "/tmp".into());
        minimal_api::set_agent_minimal_mode_for_test(&mut agent);
        agent
    }

    fn approval(
        focus: PlanApprovalFocus,
    ) -> pager::views::plan_approval_view::PlanApprovalViewState {
        minimal_api::test_plan_approval(focus)
    }

    fn buffer_text(buf: &Buffer) -> String {
        let area = *buf.area();
        (area.top()..area.bottom())
            .map(|y| {
                (area.left()..area.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn rebuilt_approval_uses_global_pending_decision_hint() {
        let mut agent = agent();
        let mut viewer = LineViewerState::open_markdown_content(
            "plan.md",
            "# Plan\n\n- Keep this visible".into(),
            None,
        )
        .unwrap();
        viewer.kind = LineViewerKind::PlanPreview;
        viewer.plan_mut().feedback_active = true;
        minimal_api::set_plan_viewer_for_test(&mut agent, viewer);
        minimal_api::set_plan_approval_for_test(&mut agent, approval(PlanApprovalFocus::Preview));
        minimal_api::approve_then_rebuild_plan_approval_for_test(&mut agent);
        let pav = minimal_api::plan_approval_view(&agent).unwrap();
        assert!(!pav.decision_pending());
        assert!(minimal_api::agent_plan_decision_pending(&agent));

        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("r check confirmation"), "{text}");
        assert!(text.contains("Esc collapse"), "{text}");
        assert!(!text.contains("a approve"), "{text}");
        assert!(!text.contains("s revise"), "{text}");
        assert!(!text.contains("l later"), "{text}");
        assert!(!text.contains("q abandon"), "{text}");
        assert!(!text.contains("y copy"), "{text}");
    }

    #[test]
    fn collapsed_revision_prompt_omits_unavailable_preview_keys() {
        let mut agent = agent();
        minimal_api::set_plan_approval_for_test(&mut agent, approval(PlanApprovalFocus::Prompt));
        minimal_api::agent_prompt_mut(&mut agent).set_text("revise this step");

        let area = Rect::new(0, 0, 80, 3);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("Enter request changes"), "{text}");
        assert!(text.contains("Tab plan"), "{text}");
        assert!(!text.contains("Esc collapse"), "{text}");
        assert!(!text.contains("l later"), "{text}");
        assert!(!text.contains("q abandon"), "{text}");
        assert!(!text.contains("y copy"), "{text}");
    }

    #[test]
    fn empty_revision_prompt_does_not_hint_enter_to_approve() {
        let mut agent = agent();
        minimal_api::set_plan_approval_for_test(&mut agent, approval(PlanApprovalFocus::Prompt));
        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("a approve"), "{text}");
        assert!(!text.contains("Enter approve"), "{text}");
    }

    #[test]
    fn minimal_preview_hints_wrap_at_narrow_width() {
        let mut agent = agent();
        minimal_api::set_plan_approval_for_test(&mut agent, approval(PlanApprovalFocus::Preview));
        let mut viewer =
            LineViewerState::open_markdown_content("plan.md", "# Plan".into(), None).unwrap();
        viewer.kind = LineViewerKind::PlanPreview;
        minimal_api::set_plan_viewer_for_test(&mut agent, viewer);
        let area = Rect::new(0, 0, 24, height(&agent, 24));
        assert!(area.height > 2);
        let mut buf = Buffer::empty(area);
        render(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("q abandon"), "{text}");
        assert!(text.contains("Esc collapse"), "{text}");
    }

    #[test]
    fn minimal_plan_commenting_shows_the_live_editor() {
        let mut agent = agent();
        minimal_api::set_plan_approval_for_test(
            &mut agent,
            approval(PlanApprovalFocus::Commenting),
        );
        minimal_api::agent_prompt_mut(&mut agent).set_text("comment draft");

        let area = Rect::new(0, 0, 60, 3);
        let mut buf = Buffer::empty(area);
        let cursor = render(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("comment draft"), "{text}");
        assert!(cursor.is_some());
    }

    #[test]
    fn ordinary_minimal_plan_preview_and_comment_editor_are_visible() {
        let mut agent = agent();
        let mut viewer = LineViewerState::open_markdown_content(
            "plan.md",
            "# Ordinary preview body\n\n- Visible in Minimal".into(),
            None,
        )
        .unwrap();
        viewer.kind = LineViewerKind::PlanPreview;
        viewer.plan_mut().show_action_buttons = true;
        viewer.list_state.open_comment_input("casual comment draft");
        minimal_api::set_plan_viewer_for_test(&mut agent, viewer);
        assert_eq!(
            crate::overlay::active_modal(&agent),
            Some(crate::overlay::Modal::PlanPreview)
        );

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_preview(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("Ordinary preview body"), "{text}");
        assert!(text.contains("casual comment draft"), "{text}");
    }

    #[test]
    fn retained_minimal_plan_preview_shows_its_review_action() {
        let mut agent = agent();
        let mut viewer =
            LineViewerState::open_markdown_content("plan.md", "# Retained plan".into(), None)
                .unwrap();
        viewer.kind = LineViewerKind::PlanPreview;
        viewer.plan_mut().show_action_buttons = true;
        viewer.plan_mut().show_review_button = true;
        minimal_api::set_plan_viewer_for_test(&mut agent, viewer);

        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        render_preview(&mut buf, area, &mut agent, &Theme::current());
        let text = buffer_text(&buf);
        assert!(text.contains("r review"), "{text}");
    }
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
