//! Minimal-mode live region: the small pinned viewport holding the running-turn
//! tail (model B), optional todos / `/btw` panels, a one-line status indicator,
//! and the always-focused prompt.
//!
//! Layout (top → bottom): live tail · todos · `/btw` · status · prompt ·
//! overlay/info. The tail shows the bottom of the uncommitted run (streaming
//! message / running tool) so output is visible as it generates; finished blocks
//! scroll up into native scrollback via [`super::commit`]. When idle the tail is
//! empty and only status + prompt (+ optional panels) show.
use pager::app::PagerTerminal;
use pager::app::root::AppView;
use pager::minimal_api;
use pager::render::Renderable;
use pager::scrollback::state::ScrollbackState;
use pager::scrollback::wrappers::EntryRenderer;
use pager::theme::Theme;
use pager::views::prompt_widget::{PromptBg, PromptStyle};
use pager::views::turn_status;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Widget};
/// Left inset (columns) for every auxiliary live-region row: the status row,
/// the info bar, the exit hint, and the todo panel — and the prompt's
/// `chrome_pad_left`.
///
/// Minimal is flush-left: committed/tail blocks zero block pads via
/// [`super::commit::committed_appearance`] and reclaim the accent column via
/// `hide_accent`, so content glyphs (`◆` / `$` / message text) start at column
/// 0, matching the welcome card's outer edge. The prompt and auxiliary rows
/// share that left edge (no chrome pad) so nothing sits ragged against the
/// welcome box.
pub(super) fn live_left_inset(_appearance: &pager::appearance::AppearanceConfig) -> u16 {
    0
}
/// Shrink `area` from the left by `inset` columns (clamped to the width).
fn inset_left(area: Rect, inset: u16) -> Rect {
    let dx = inset.min(area.width);
    Rect {
        x: area.x + dx,
        width: area.width - dx,
        ..area
    }
}
/// Drop cached `/btw` geometry so minimal input cannot scroll an invisible
/// panel after a modal host path skipped painting it.
fn clear_btw_geometry(agent: &mut pager::app::agent_view::AgentView) {
    minimal_api::clear_agent_btw_geometry(agent);
}
/// Keep a paintable `/btw` area only when it is wholly inside the frame buffer.
fn paintable_btw_area(frame_area: Rect, area: Rect) -> Option<Rect> {
    (minimal_api::minimal_btw_geometry_is_paintable(area)
        && area.x >= frame_area.x
        && area.y >= frame_area.y
        && area.x.saturating_add(area.width) <= frame_area.x.saturating_add(frame_area.width)
        && area.y.saturating_add(area.height) <= frame_area.y.saturating_add(frame_area.height))
    .then_some(area)
}
/// The prompt style used by the minimal live region.
///
/// Shared with [`super::overlay::sync_viewport`] so viewport sizing measures the
/// prompt's height exactly as the live region will draw it.
///
/// `input_mode` wires special composer modes (bash `! `, feedback `~ `,
/// remember `# `) the same way the full TUI does — without this, `!` on an
/// empty prompt would flip mode invisibly (key consumed, default `❯` remains).
pub(super) fn prompt_style(
    appearance: &pager::appearance::AppearanceConfig,
    input_mode: pager::app::agent_view::PromptInputMode,
    theme: &Theme,
    multiline: bool,
) -> PromptStyle {
    PromptStyle {
        focused: true,
        show_prefix: appearance.prompt.show_prefix,
        vpad_top: 0,
        compact: appearance.prompt.compact,
        chrome: true,
        chrome_pad_left: live_left_inset(appearance),
        chrome_pad_right: 0,
        bg: PromptBg::Canvas(Color::Reset),
        accent_color_override: input_mode.accent_color(theme),
        border_color_override: None,
        prefix_override: input_mode.prefix_override(theme),
        placeholder_override: input_mode.placeholder_override(multiline),
        show_accent_line: false,
        show_borders: false,
        title: None,
        image_preview: true,
    }
}
/// Draw the pinned live region (tail + status + prompt) into the inline viewport.
pub fn draw_live(
    app: &mut AppView,
    terminal: &mut PagerTerminal,
    frame_stamp: pager::motion::FrameStamp,
) {
    let force_todos = minimal_api::minimal_show_todos(app);
    let auth_hint = crate::startup::minimal_startup_hint(minimal_api::app_trust_state(app));
    let pending_hint = minimal_pending_hint(minimal_api::app_pending_action(app));
    let transcript_hint = if minimal_api::minimal_ctrl_o_opens_transcript(app) {
        "ctrl+o transcript"
    } else {
        "/transcript"
    };
    let transcript_progress = minimal_api::minimal_transcript_progress(app);
    minimal_api::with_minimal_live_state(app, |cursor, mut agent, appearance| {
        let theme = Theme::current();
        let commit_app = super::commit::committed_appearance(appearance);
        let compact = appearance.prompt.compact;
        let (input_mode, multiline) = agent
            .as_deref()
            .map(|a| {
                (
                    minimal_api::agent_prompt_input_mode(a),
                    minimal_api::agent_multiline_mode(a),
                )
            })
            .unwrap_or_default();
        let style = prompt_style(appearance, input_mode, &theme, multiline);
        let row_inset = live_left_inset(appearance);
        let layout_cfg = &appearance.scrollback.layout;
        let term_h = terminal.last_known_area().height;
        if let Some(agent) = agent.as_deref_mut() {
            clear_btw_geometry(agent);
        }
        pager::render::draw::draw_frame(terminal, cursor, |frame, link_spans| {
            let area = frame.area();
            if area.height == 0 || area.width < 4 {
                return (None, None);
            }
            Clear.render(area, frame.buffer_mut());
            let Some(agent) = agent.as_deref_mut() else {
                crate::startup::render_startup(frame.buffer_mut(), area, &theme, &auth_hint);
                return (None, None);
            };
            minimal_api::ensure_agent_media_link_paths(agent);
            let hyperlink_route = pager::hyperlink_route::hyperlink_route();
            minimal_api::set_agent_active_pane(agent, pager::app::agent_view::AgentPane::Prompt);
            let status_activity = minimal_advance_phase_timer(agent);
            let show_todos = crate::todo::todo_panel_visible(agent, force_todos);
            let queued = minimal_api::agent_pending_prompt_count(agent)
                + minimal_api::agent_shared_queue_len(agent);
            if let Some(kind) = super::panel::active(agent) {
                let cursor = super::panel::render(
                    frame.buffer_mut(),
                    area,
                    agent,
                    kind,
                    &theme,
                    frame_stamp,
                );
                return (cursor, None);
            }
            if super::overlay::app_modal_active(agent) {
                super::overlay::render_app_modal(
                    frame.buffer_mut(),
                    area,
                    agent,
                    compact,
                    frame_stamp,
                );
                return (None, None);
            }
            if minimal_api::extensions_modal(agent).is_some() {
                if let Some(state) = minimal_api::extensions_modal_mut(agent) {
                    pager::views::extensions_modal::render_extensions_modal(
                        frame.buffer_mut(),
                        area,
                        state,
                        None,
                        compact,
                        frame_stamp,
                    );
                }
                return (None, None);
            }
            if let Some(modal) = super::overlay::active_modal(agent) {
                let status_h = 1u16.min(area.height);
                let content_w = area.width as usize;
                let modal_h = super::overlay::modal_height(modal, agent, term_h, content_w)
                    .min(area.height.saturating_sub(status_h))
                    .max(1);
                let tail_h = area.height.saturating_sub(status_h + modal_h);
                if tail_h > 0 && !minimal_api::agent_session_reload_active(agent) {
                    let turn_running = minimal_api::agent_state(agent).is_turn_running();
                    draw_tail(
                        frame.buffer_mut(),
                        Rect {
                            x: area.x,
                            y: area.y,
                            width: area.width,
                            height: tail_h,
                        },
                        minimal_api::agent_scrollback(agent),
                        turn_running,
                        &theme,
                        &commit_app,
                        minimal_api::agent_cwd(agent),
                        minimal_api::agent_media_link_paths(agent),
                        frame_stamp,
                        link_spans,
                        hyperlink_route.emit_osc8,
                        hyperlink_route.emit_id,
                    );
                }
                render_minimal_status(
                    frame.buffer_mut(),
                    inset_left(
                        Rect {
                            x: area.x,
                            y: area.y + tail_h,
                            width: area.width,
                            height: status_h,
                        },
                        row_inset,
                    ),
                    agent,
                    &status_activity,
                    transcript_progress,
                    &theme,
                    frame_stamp,
                );
                let modal_area = Rect {
                    x: area.x,
                    y: area.y + tail_h + status_h,
                    width: area.width,
                    height: modal_h,
                };
                let cursor = super::overlay::render_modal(
                    frame.buffer_mut(),
                    modal_area,
                    modal,
                    agent,
                    &theme,
                    term_h,
                );
                return (cursor, None);
            }
            let status_h = 1u16.min(area.height);
            let confirming_behavior = minimal_api::agent_behavior_switch_hint(agent).is_some();
            let overlay_h = if confirming_behavior {
                0
            } else {
                super::overlay::overlay_rows(minimal_api::agent_prompt(agent), area.width)
                    .min(area.height.saturating_sub(status_h + 1))
            };
            let info_h = if overlay_h == 0 {
                1u16.min(area.height.saturating_sub(status_h + 1))
            } else {
                0
            };
            let below_h = overlay_h + info_h;
            let avail = area.height.saturating_sub(status_h + below_h);
            let prompt_h = minimal_api::agent_prompt(agent)
                .desired_height(area.width, &style, false, avail)
                .min(avail)
                .max(1);
            let rest = avail.saturating_sub(prompt_h);
            let raw_btw = if minimal_api::minimal_btw_surface_available(agent) {
                pager::views::btw_overlay::btw_panel_height(
                    minimal_api::agent_btw_state(agent),
                    area.width,
                )
            } else {
                0
            };
            let btw_desired = minimal_api::minimal_btw_visible_height(raw_btw, area.width, rest);
            let after_btw = rest.saturating_sub(btw_desired);
            let todos_cap = if force_todos {
                after_btw
            } else {
                after_btw.min(crate::todo::MAX_TODO_ROWS)
            };
            let todo_lines = if show_todos {
                crate::todo::todo_panel_lines(agent, todos_cap, force_todos)
            } else {
                Vec::new()
            };
            let todos_h = (todo_lines.len() as u16).min(after_btw);
            let btw_h = btw_desired;
            let tail_h = rest.saturating_sub(todos_h + btw_h);
            if tail_h > 0 && !minimal_api::agent_session_reload_active(agent) {
                let tail_area = Rect {
                    x: area.x,
                    y: area.y,
                    width: area.width,
                    height: tail_h,
                };
                let turn_running = minimal_api::agent_state(agent).is_turn_running();
                draw_tail(
                    frame.buffer_mut(),
                    tail_area,
                    minimal_api::agent_scrollback(agent),
                    turn_running,
                    &theme,
                    &commit_app,
                    minimal_api::agent_cwd(agent),
                    minimal_api::agent_media_link_paths(agent),
                    frame_stamp,
                    link_spans,
                    hyperlink_route.emit_osc8,
                    hyperlink_route.emit_id,
                );
            }
            if todos_h > 0 {
                crate::todo::render_todo_panel(
                    frame.buffer_mut(),
                    inset_left(
                        Rect {
                            x: area.x,
                            y: area.y + tail_h,
                            width: area.width,
                            height: todos_h,
                        },
                        row_inset,
                    ),
                    &theme,
                    &todo_lines,
                );
            }
            let btw_area = paintable_btw_area(
                area,
                Rect {
                    x: area.x,
                    y: area.y.saturating_add(tail_h).saturating_add(todos_h),
                    width: area.width,
                    height: btw_h,
                },
            );
            let btw_state = minimal_api::agent_btw_state(agent).cloned();
            if let (Some(btw), Some(btw_area)) = (btw_state.as_ref(), btw_area) {
                let focused = minimal_api::btw_focused(agent);
                pager::views::btw_overlay::render_btw_panel(
                    frame.buffer_mut(),
                    btw,
                    btw_area,
                    frame_stamp,
                    focused,
                    None,
                    minimal_api::agent_btw_selection_model_mut(agent),
                    None,
                    None,
                    &[],
                );
                minimal_api::set_agent_btw_area(agent, btw_area);
            }
            let status_area = inset_left(
                Rect {
                    x: area.x,
                    y: area.y + tail_h + todos_h + btw_h,
                    width: area.width,
                    height: status_h,
                },
                row_inset,
            );
            render_minimal_status(
                frame.buffer_mut(),
                status_area,
                agent,
                &status_activity,
                transcript_progress,
                &theme,
                frame_stamp,
            );
            let prompt_area = Rect {
                x: area.x,
                y: area.y + tail_h + todos_h + btw_h + status_h,
                width: area.width,
                height: prompt_h,
            };
            if overlay_h > 0 {
                super::overlay::render(
                    frame.buffer_mut(),
                    area,
                    prompt_area,
                    minimal_api::agent_prompt_mut(agent),
                    layout_cfg,
                    compact,
                    &theme,
                );
            } else if info_h > 0 {
                let info_area = inset_left(
                    Rect {
                        x: area.x,
                        y: prompt_area.y + prompt_h,
                        width: area.width,
                        height: info_h,
                    },
                    row_inset,
                );
                if let Some(hint) = minimal_api::agent_behavior_switch_hint(agent) {
                    render_exit_hint(frame.buffer_mut(), info_area, &theme, hint);
                } else if let Some(hint) = &pending_hint {
                    render_exit_hint(frame.buffer_mut(), info_area, &theme, hint);
                } else {
                    render_prompt_info(
                        frame.buffer_mut(),
                        info_area,
                        agent,
                        queued,
                        transcript_hint,
                        &theme,
                    );
                }
            }
            let result = minimal_api::agent_prompt_mut(agent).draw(
                frame.buffer_mut(),
                prompt_area,
                None,
                &style,
                None,
            );
            (
                if confirming_behavior {
                    None
                } else {
                    result.cursor_pos
                },
                result
                    .post_flush_escapes
                    .map(pager::terminal::overlay::PostFlush::from),
            )
        });
    });
}
fn live_tail_renderer<'a>(
    entry: &'a pager::scrollback::entry::ScrollbackEntry,
    theme: &'a Theme,
    appearance: &pager::appearance::AppearanceConfig,
    cwd: &'a std::path::Path,
    frame: pager::motion::FrameStamp,
) -> EntryRenderer<'a> {
    super::commit::minimal_renderer(entry, theme, appearance.clone(), cwd, frame)
}
/// Render the uncommitted tail (entries past the commit frontier), bottom-anchored
/// so the most recent output is always visible; the topmost visible entry is
/// clipped via `with_skip_rows` when the run is taller than the tail area.
///
/// Starts at the shared [`super::commit::scan_frontier`] stop point so it renders
/// exactly the entries [`tail_height`] measured (the viewport was sized to that —
/// any disagreement makes the prompt jump on commit).
#[allow(clippy::too_many_arguments)]
fn draw_tail(
    buf: &mut Buffer,
    area: Rect,
    sb: &ScrollbackState,
    turn_running: bool,
    theme: &Theme,
    appearance: &pager::appearance::AppearanceConfig,
    cwd: &std::path::Path,
    media_paths: &[std::path::PathBuf],
    frame: pager::motion::FrameStamp,
    link_spans: &mut Vec<ratatui_inline::LinkSpan>,
    emit_links: bool,
    emit_link_ids: bool,
) {
    if area.height == 0 {
        return;
    }
    let width = area.width;
    let renderer = |e| live_tail_renderer(e, theme, appearance, cwd, frame);
    let mut entries = Vec::new();
    let mut i = super::commit::scan_frontier(sb, turn_running).tail_start;
    while let Some(e) = sb.get(i) {
        entries.push(e);
        i += 1;
    }
    if entries.is_empty() {
        return;
    }
    let gap = super::commit::MINIMAL_BLOCK_GAP;
    let heights: Vec<u16> = entries
        .iter()
        .map(|e| renderer(*e).desired_height(width))
        .collect();
    let total: u16 = heights
        .iter()
        .fold(0u16, |acc, &h| acc.saturating_add(h).saturating_add(gap));
    let mut skip_top = total.saturating_sub(area.height);
    let mut y = area.y;
    let bottom = area.y + area.height;
    for (e, &content_h) in entries.iter().zip(&heights) {
        let slot_h = content_h.saturating_add(gap);
        if skip_top >= slot_h {
            skip_top -= slot_h;
            continue;
        }
        let slot_skip = skip_top;
        skip_top = 0;
        let entry_skip = slot_skip.min(content_h);
        let visible_content = content_h.saturating_sub(entry_skip);
        if visible_content > 0 {
            let draw_h = visible_content.min(bottom.saturating_sub(y));
            if draw_h == 0 {
                break;
            }
            let rect = Rect {
                x: area.x,
                y,
                width,
                height: draw_h,
            };
            let renderer = renderer(*e).with_skip_rows(entry_skip);
            if emit_links {
                link_spans.extend(
                    renderer
                        .link_overlay(rect, media_paths)
                        .resolved_spans(emit_link_ids),
                );
            }
            renderer.render(rect, buf);
            y += draw_h;
            if y >= bottom {
                break;
            }
        }
        let gap_skipped = slot_skip.saturating_sub(entry_skip);
        let gap_visible = gap
            .saturating_sub(gap_skipped)
            .min(bottom.saturating_sub(y));
        y += gap_visible;
        if y >= bottom {
            break;
        }
    }
}
/// Resolve the current turn activity. Phase transitions are reconciled by the
/// pager reducer before rendering, identically for visible and hidden agents.
fn minimal_advance_phase_timer(
    agent: &mut pager::app::agent_view::AgentView,
) -> Option<pager::acp::tracker::TurnActivity> {
    minimal_api::resolve_turn_activity(agent)
}
/// Render the one-line minimal status indicator above the prompt.
///
/// Reuses the full-TUI [`turn_status::render_turn_status`] widget so minimal
/// surfaces the same rich activity detail (`Run …` / `Thinking…` /
/// `Waiting on subagent…` / `Retrying (attempt N)…` / `Cancelling…`), the
/// per-phase + turn timers, and the "… still running" cue (running commands /
/// monitors / loops / background subagents) — instead of collapsing
/// everything to "working…". Keyboard-only, so the mouse `[stop]` / `[↓]`
/// buttons are suppressed (`None`), and `flat_background` keeps the row
/// transparent like the rest of the live region. When the widget would draw
/// nothing a small `minimal · /help` hint is shown instead.
fn render_minimal_status(
    buf: &mut Buffer,
    area: Rect,
    agent: &pager::app::agent_view::AgentView,
    activity: &Option<pager::acp::tracker::TurnActivity>,
    transcript_progress: Option<(usize, usize)>,
    theme: &Theme,
    frame: pager::motion::FrameStamp,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if let Some((done, total)) = transcript_progress {
        let style = theme.primary().bg(Color::Reset);
        buf.set_style(area, style);
        buf.set_span(
            area.x,
            area.y,
            &Span::styled(format!("rendering transcript… {done}/{total}"), style),
            area.width,
        );
        return;
    }
    let watchers = minimal_api::watchers(agent);
    let drain_blocked = minimal_api::drain_blocked(agent);
    let parked = minimal_api::renders_parked(agent);
    let control_status = minimal_api::control_status(agent, area.width as usize);
    if control_status.is_none()
        && !turn_status::should_show(
            minimal_api::agent_state(agent),
            drain_blocked,
            minimal_api::mcp_init_progress(agent),
            watchers,
            parked,
        )
    {
        render_idle_hint(buf, area, theme);
        return;
    }
    let is_pending_user_input = !minimal_api::agent_permission_queue(agent).is_empty()
        || minimal_api::question_view(agent).is_some();
    turn_status::render_turn_status(
        buf,
        area,
        turn_status::TurnStatusArgs {
            state: minimal_api::agent_state(agent),
            activity,
            turn_elapsed: minimal_api::agent_turn_elapsed_at(agent, frame.now()),
            activity_started_at: minimal_api::agent_activity_started_at(agent),
            frame,
            drain_blocked,
            buttons: None,
            has_running_execute: false,
            total_tokens: minimal_api::agent_context_used(agent),
            mcp_init_progress: minimal_api::mcp_init_progress(agent),
            is_bash_turn: minimal_api::agent_is_bash_turn(agent),
            is_pending_user_input,
            watchers,
            parked,
            flat_background: true,
            held_queue: minimal_api::held_queue_count(agent),
            held_queue_top_sendable: minimal_api::held_queue_top_sendable(agent),
            control_status: control_status.as_deref(),
        },
    );
}
/// Idle status: `minimal · [/fullscreen to go back ·] /help` (+ auto-set note).
fn render_idle_hint(buf: &mut Buffer, area: Rect, theme: &Theme) {
    let style = theme.dim().bg(Color::Reset);
    buf.set_style(area, style);
    let auto = pager::app::minimal_auto_set_for_mouse_leak();
    let switch_back = pager::app::minimal_show_switch_back_to_fullscreen();
    let hint = match (auto, switch_back) {
        (true, true) => {
            "minimal · auto-set on JetBrains/Windows due to JetBrains mouse reporting issues \
             · /fullscreen to go back · /help"
        }
        (true, false) => {
            "minimal · auto-set on JetBrains/Windows due to JetBrains mouse reporting issues · /help"
        }
        (false, true) => "minimal · /fullscreen to go back · /help",
        (false, false) => "minimal · /help",
    };
    buf.set_span(area.x, area.y, &Span::styled(hint, style), area.width);
}
/// Render the one-line info bar directly below the prompt: the selected model,
/// the active session mode (cycled with Ctrl+R),
/// context usage (absolute + percentage), an `N queued` count when prompts
/// are waiting behind a running turn, and the full-transcript shortcut hint
/// (`transcript_hint`: "ctrl+o transcript", or "/transcript" where Ctrl+O is
/// the interject chord — Apple Terminal). Mirrors the regular TUI's model
/// label, mode flags, and context bar; the transcript hint stands in for the
/// full TUI's shortcuts bar, which minimal never renders — without it the
/// folded conversation has no visible way back to the full view. The mode flag
/// keeps its accent color so the current mode is always shown. Drawn only when
/// no menu/dropdown owns the
/// band below the prompt (the caller gates on that). The elapsed-time / token
/// count lives in the turn-status row above the prompt (see
/// [`render_minimal_status`]), so it is not repeated here.
fn render_prompt_info(
    buf: &mut Buffer,
    area: Rect,
    agent: &pager::app::agent_view::AgentView,
    queued: usize,
    transcript_hint: &str,
    theme: &Theme,
) {
    use pager::views::context_bar::fmt_tokens;
    let base = theme.primary().bg(Color::Reset);
    let sep = theme.dim().bg(Color::Reset);
    let mut segs: Vec<(String, Style)> = Vec::new();
    if let Some(label) = minimal_api::agent_prompt_input_mode(agent).prompt_info_override() {
        segs.push((label.to_string(), base));
    } else {
        if let Some(model) = minimal_api::agent_current_model_name(agent) {
            let label = match minimal_api::agent_reasoning_effort(agent) {
                Some(eff) => format!("{model} ({eff})"),
                None => model,
            };
            segs.push((label, base));
        }
        let behavior = minimal_api::effective_behavior_mode(agent);
        let behavior_label = match behavior {
            minimal_api::BehaviorId::Normal => "normal",
            minimal_api::BehaviorId::Clarify => "clarify",
            minimal_api::BehaviorId::Plan => match minimal_api::plan_phase(agent) {
                Some("awaiting_approval") => "plan · awaiting approval",
                Some("executing") => "plan · executing",
                Some("amending") => "plan · amending",
                _ => "plan · drafting",
            },
            minimal_api::BehaviorId::Workflow => "workflow",
            minimal_api::BehaviorId::Goal => "goal",
        };
        let behavior_color = if behavior == minimal_api::BehaviorId::Plan {
            theme.accent_plan
        } else {
            theme.accent_system
        };
        segs.push((behavior_label.to_string(), base.fg(behavior_color)));
        let (permission_label, permission_style) = if minimal_api::agent_is_always_approve(agent) {
            ("always-approve", base.fg(theme.warning))
        } else if minimal_api::agent_is_auto(agent) {
            ("auto", base.fg(theme.accent_system))
        } else {
            ("ask", base)
        };
        segs.push((permission_label.to_string(), permission_style));
        let used = minimal_api::agent_context_used(agent);
        let total = minimal_api::agent_context_total(agent)
            .or_else(|| minimal_api::agent_model_context_window(agent));
        if let (Some(used), Some(total)) = (used, total)
            && total > 0
        {
            let pct = token_estimation::usage_percentage(used, total);
            segs.push((
                format!("{} / {} ({:.0}%)", fmt_tokens(used), fmt_tokens(total), pct),
                base,
            ));
        }
    }
    if queued > 0 {
        segs.push((format!("{queued} queued"), base));
        segs.push(("/queue".to_string(), base));
    }
    segs.push((transcript_hint.to_string(), base));
    if segs.is_empty() {
        return;
    }
    buf.set_style(area, base);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (text, style)) in segs.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", sep));
        }
        spans.push(Span::styled(text, style));
    }
    buf.set_line(area.x, area.y, &Line::from(spans), area.width);
}
/// The double-press confirmation hint to show under the prompt (e.g. "press
/// Ctrl+q again to quit"), or `None` when nothing is armed / it has expired or
/// is a silent arm (no label). Mirrors the full-TUI shortcuts-bar `PendingHint`,
/// which minimal does not render.
fn minimal_pending_hint(pending: &Option<pager::app::root::PendingAction>) -> Option<String> {
    let pending = pending.as_ref()?;
    if pending.expired() {
        return None;
    }
    let label = pending.label?;
    Some(format!(
        "press {} again to {label}",
        pending.shortcut.display()
    ))
}
/// Render the one-line double-press confirmation hint under the prompt, in the
/// warning color so it stands out from the model/context info row.
fn render_exit_hint(buf: &mut Buffer, area: Rect, theme: &Theme, hint: &str) {
    let style = Style::default().fg(theme.warning).bg(Color::Reset);
    buf.set_style(area, style);
    buf.set_span(
        area.x,
        area.y,
        &Span::styled(hint.to_string(), style),
        area.width,
    );
}
/// Height (rows) of the tail that will REMAIN after this frame's commit pass —
/// i.e. the entries `commit_active` will NOT consume, from the first
/// non-committable entry (past the scan cursor) onward.
///
/// The overlay host sizes the live viewport to this *post-commit* tail so the
/// prompt sits right after the streaming output (no fixed gap while a turn is
/// "thinking" with nothing streamed yet). Sizing to the post-commit tail
/// (rather than the current tail) is load-bearing: because `sync_viewport` runs
/// just *before* `commit_active`, the viewport is already at its post-commit
/// height when the commit's `insert_before` prints finalized blocks — so it can
/// reposition the correctly-sized viewport to sit directly after them
/// (content-anchored). Sizing to the tall streaming tail instead left the
/// viewport oversized at commit time, and the following collapse stranded the
/// prompt at the top of the screen (the "snaps to top" bug).
pub(super) fn tail_height(
    agent: &pager::app::agent_view::AgentView,
    width: u16,
    appearance: &pager::appearance::AppearanceConfig,
    frame: pager::motion::FrameStamp,
) -> u16 {
    if minimal_api::agent_session_reload_active(agent) {
        return 0;
    }
    let theme = Theme::current();
    let sb = minimal_api::agent_scrollback(agent);
    let turn_running = minimal_api::agent_state(agent).is_turn_running();
    let gap = super::commit::MINIMAL_BLOCK_GAP;
    let mut i = super::commit::scan_frontier(sb, turn_running).tail_start;
    let mut total = 0u16;
    while let Some(e) = sb.get(i) {
        let h = live_tail_renderer(e, &theme, appearance, minimal_api::agent_cwd(agent), frame)
            .desired_height(width);
        total = total.saturating_add(h).saturating_add(gap);
        i += 1;
    }
    total
}
#[cfg(test)]
mod tests {
    use super::*;
    use pager::scrollback::RenderBlock;
    fn agent() -> pager::app::agent_view::AgentView {
        minimal_api::test_agent_view(Some("s1"), std::path::PathBuf::from("/tmp"))
    }
    #[test]
    fn btw_area_must_be_fully_paintable() {
        let frame = Rect::new(0, 0, 80, 20);
        assert_eq!(
            paintable_btw_area(frame, Rect::new(0, 4, 80, 3)),
            Some(Rect::new(0, 4, 80, 3))
        );
        assert!(!minimal_api::minimal_btw_size_is_paintable(11, 3));
        assert!(minimal_api::minimal_btw_size_is_paintable(12, 3));
        assert!(!minimal_api::minimal_btw_size_is_paintable(80, 2));
        assert!(paintable_btw_area(frame, Rect::new(0, 4, 11, 3)).is_none());
        assert!(paintable_btw_area(frame, Rect::new(0, 4, 80, 2)).is_none());
        assert!(paintable_btw_area(frame, Rect::new(0, 19, 80, 3)).is_none());
        assert!(paintable_btw_area(frame, Rect::new(79, 4, 2, 3)).is_none());
    }
    #[test]
    fn tail_height_uses_owning_session_cwd_for_tool_paths() {
        use pager::app::session::AgentState;
        use pager::scrollback::RenderBlock;
        use pager::scrollback::entry::ScrollbackEntry;
        use pager::scrollback::types::DisplayMode;
        let cwd = std::path::PathBuf::from("/alternate/worktree");
        let mut agent = minimal_api::test_agent_view(Some("s1"), cwd.clone());
        minimal_api::set_agent_state_for_test(&mut agent, AgentState::TurnRunning);
        let mut entry = ScrollbackEntry::running(RenderBlock::edit(
            "/alternate/worktree/src/components/really_long_file_name.rs",
            None,
        ));
        entry.set_display_mode(DisplayMode::Expanded);
        minimal_api::agent_scrollback_mut(&mut agent).push(entry);
        let appearance = super::super::commit::committed_appearance(
            &pager::appearance::AppearanceConfig::default(),
        );
        let theme = Theme::current();
        let entry = minimal_api::agent_scrollback(&agent).get(0).unwrap();
        let (width, painted_height, visible_accent_height) = (10..=40)
            .find_map(|width| {
                let painted = live_tail_renderer(
                    entry,
                    &theme,
                    &appearance,
                    &cwd,
                    pager::motion::FrameStamp::default(),
                )
                .desired_height(width);
                let visible_accent = EntryRenderer::new(entry, &theme)
                    .with_appearance(appearance.clone())
                    .with_cwd(Some(&cwd))
                    .with_frame(pager::motion::FrameStamp::default())
                    .with_flat_background(true)
                    .desired_height(width);
                (painted != visible_accent).then_some((width, painted, visible_accent))
            })
            .expect("fixture must wrap differently when the accent column is reclaimed");
        assert_ne!(painted_height, visible_accent_height);
        assert_eq!(
            tail_height(
                &agent,
                width,
                &appearance,
                pager::motion::FrameStamp::default(),
            ),
            painted_height.saturating_add(super::super::commit::MINIMAL_BLOCK_GAP)
        );
    }

    #[test]
    fn reconnect_full_replay_is_frozen_then_skipped_as_existing_history() {
        let mut agent = agent();
        minimal_api::set_agent_minimal_mode_for_test(&mut agent);
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("old native history"));
        super::super::commit::commit_leading_run(
            minimal_api::agent_scrollback_mut(&mut agent),
            false,
            |_, _| true,
        );

        minimal_api::begin_agent_session_reload_for_test(&mut agent, 7);
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("old native history"));
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("missed while disconnected"));
        minimal_api::mark_agent_reload_replay_seen_for_test(&mut agent);
        let appearance = super::super::commit::committed_appearance(
            &pager::appearance::AppearanceConfig::default(),
        );
        assert_eq!(
            tail_height(
                &agent,
                40,
                &appearance,
                pager::motion::FrameStamp::default(),
            ),
            0,
            "staged replay must not leak into Minimal's live region"
        );

        assert!(minimal_api::finish_agent_session_reload_for_test(
            &mut agent, 7, true
        ));
        let sb = minimal_api::agent_scrollback(&agent);
        let scan = super::super::commit::scan_frontier(sb, false);
        assert_eq!(scan.tail_start, 2);
        assert!(
            scan.will_commit,
            "only the prefix already printed before reconnect is skipped"
        );

        let mut emitted = Vec::new();
        super::super::commit::commit_leading_run(
            minimal_api::agent_scrollback_mut(&mut agent),
            false,
            |_, index| {
                emitted.push(index);
                true
            },
        );
        assert_eq!(
            emitted,
            vec![1],
            "the missed replay tail prints exactly once"
        );

        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("new after reconnect"));
        let scan =
            super::super::commit::scan_frontier(minimal_api::agent_scrollback(&agent), false);
        assert!(scan.will_commit, "only the true post-reload tail commits");
    }

    #[test]
    fn reconnect_cursor_success_commits_only_the_staged_tail() {
        let mut agent = agent();
        minimal_api::set_agent_minimal_mode_for_test(&mut agent);
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("old native history"));
        super::super::commit::commit_leading_run(
            minimal_api::agent_scrollback_mut(&mut agent),
            false,
            |_, _| true,
        );

        minimal_api::begin_agent_session_reload_for_test(&mut agent, 8);
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("cursor tail"));
        let appearance = super::super::commit::committed_appearance(
            &pager::appearance::AppearanceConfig::default(),
        );
        assert_eq!(
            tail_height(
                &agent,
                40,
                &appearance,
                pager::motion::FrameStamp::default(),
            ),
            0,
            "cursor tail stays staged until the transaction resolves"
        );

        assert!(minimal_api::finish_agent_session_reload_for_test(
            &mut agent, 8, true
        ));
        let mut emitted = Vec::new();
        super::super::commit::commit_leading_run(
            minimal_api::agent_scrollback_mut(&mut agent),
            false,
            |_, index| {
                emitted.push(index);
                true
            },
        );
        assert_eq!(emitted, vec![1], "the old prefix must not be reprinted");
    }

    #[test]
    fn reconnect_failure_discards_staged_replay_without_advancing_frontier() {
        let mut agent = agent();
        minimal_api::set_agent_minimal_mode_for_test(&mut agent);
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("old native history"));
        super::super::commit::commit_leading_run(
            minimal_api::agent_scrollback_mut(&mut agent),
            false,
            |_, _| true,
        );

        minimal_api::begin_agent_session_reload_for_test(&mut agent, 9);
        minimal_api::agent_scrollback_mut(&mut agent)
            .push_block(RenderBlock::agent_message("partial replay to discard"));
        assert!(minimal_api::finish_agent_session_reload_for_test(
            &mut agent, 9, false
        ));

        let sb = minimal_api::agent_scrollback(&agent);
        assert_eq!(sb.len(), 1, "failed staging state must be rolled back");
        let scan = super::super::commit::scan_frontier(sb, false);
        assert_eq!(scan.tail_start, 1);
        assert!(!scan.will_commit, "restored native history stays committed");
    }
    /// The tail and the committed footprint are one builder with a different
    /// tick; this is the net for anyone tempted to fork them again.
    #[test]
    fn the_animation_tick_never_changes_a_blocks_height() {
        use pager::scrollback::RenderBlock;
        use pager::scrollback::entry::ScrollbackEntry;
        minimal_api::set_show_thinking_blocks(true);
        let theme = Theme::current();
        let cwd = std::path::PathBuf::from("/tmp");
        let appearance = super::super::commit::committed_appearance(
            &pager::appearance::AppearanceConfig::default(),
        );
        let long = "reasoning that wraps a good few times even at a hundred and \
                    twenty columns because it simply keeps going and going and going";
        for block in [
            RenderBlock::thinking(long),
            RenderBlock::agent_message(long),
            RenderBlock::execute("ls -la"),
        ] {
            let entry = ScrollbackEntry::new(block);
            for width in [20u16, 40, 80, 120] {
                let live = live_tail_renderer(
                    &entry,
                    &theme,
                    &appearance,
                    &cwd,
                    pager::motion::FrameStamp::at(
                        std::time::Instant::now(),
                        std::time::Instant::now() + std::time::Duration::from_millis(231),
                    ),
                )
                .desired_height(width);
                let committed = live_tail_renderer(
                    &entry,
                    &theme,
                    &appearance,
                    &cwd,
                    super::super::commit::committed_frame(),
                )
                .desired_height(width);
                assert_eq!(
                    live, committed,
                    "{:?} @{width}: a block's height must not depend on the tick, or the \
                     prompt jumps on commit",
                    entry.block
                );
            }
        }
    }
    #[test]
    fn minimal_status_shows_rich_activity_and_idle_hint() {
        use pager::acp::tracker::TurnActivity;
        use pager::app::session::AgentState;
        let theme = Theme::current();
        let area = Rect::new(0, 0, 60, 1);
        let read = |buf: &Buffer| -> String {
            (0..area.width)
                .filter_map(|x| buf.cell((x, 0)).map(|c| c.symbol().to_string()))
                .collect()
        };
        pager::app::set_minimal_show_switch_back_to_fullscreen_for_test(false);
        let a = agent();
        let mut buf = Buffer::empty(area);
        render_minimal_status(
            &mut buf,
            area,
            &a,
            &None,
            None,
            &theme,
            pager::motion::FrameStamp::default(),
        );
        let idle = read(&buf);
        assert!(idle.contains("/help"), "idle hint: {idle:?}");
        assert!(
            !idle.contains("/fullscreen"),
            "cold start must not show switch-back: {idle:?}"
        );
        pager::app::set_minimal_show_switch_back_to_fullscreen_for_test(true);
        let mut buf = Buffer::empty(area);
        render_minimal_status(
            &mut buf,
            area,
            &a,
            &None,
            None,
            &theme,
            pager::motion::FrameStamp::default(),
        );
        let switched = read(&buf);
        assert!(
            switched.contains("/fullscreen to go back"),
            "relaunch into minimal must show switch-back: {switched:?}"
        );
        pager::app::set_minimal_show_switch_back_to_fullscreen_for_test(false);
        let mut a = agent();
        minimal_api::set_agent_state_for_test(&mut a, AgentState::TurnRunning);
        let mut buf = Buffer::empty(area);
        render_minimal_status(
            &mut buf,
            area,
            &a,
            &Some(TurnActivity::Responding),
            None,
            &theme,
            pager::motion::FrameStamp::default(),
        );
        let text = read(&buf);
        assert!(text.contains("Responding"), "rich activity: {text:?}");
        let mut buf = Buffer::empty(area);
        render_minimal_status(
            &mut buf,
            area,
            &a,
            &Some(TurnActivity::Retrying {
                attempt: 2,
                max_retries: 3,
                reason: "transient error".to_string(),
            }),
            None,
            &theme,
            pager::motion::FrameStamp::default(),
        );
        assert!(read(&buf).contains("Retrying"), "retry: {:?}", read(&buf));
    }
    #[test]
    fn minimal_status_shows_idle_watching_cue() {
        use pager::app::session::AgentState;
        let theme = Theme::current();
        let area = Rect::new(0, 0, 60, 1);
        let read = |buf: &Buffer| -> String {
            (0..area.width)
                .filter_map(|x| buf.cell((x, 0)).map(|c| c.symbol().to_string()))
                .collect()
        };
        let mut a = agent();
        minimal_api::set_agent_state_for_test(&mut a, AgentState::Idle);
        minimal_api::insert_scheduled_task_for_test(
            &mut a,
            pager::app::session::ScheduledTaskInfo {
                task_id: "loop-1".to_string(),
                prompt: "do the thing".to_string(),
                human_schedule: "every 5m".to_string(),
                created_at: std::time::Instant::now(),
                next_fire_at: None,
                tag: "loop".to_string(),
                last_subagent_id: None,
            },
        );
        assert_eq!(minimal_api::watchers(&a).loops, 1);
        let mut buf = Buffer::empty(area);
        render_minimal_status(
            &mut buf,
            area,
            &a,
            &None,
            None,
            &theme,
            pager::motion::FrameStamp::default(),
        );
        let text = read(&buf);
        assert!(
            text.contains("1 loop still running"),
            "watching cue: {text:?}"
        );
        assert!(!text.contains("/help"), "not the idle hint: {text:?}");
    }
    #[test]
    fn prompt_style_bash_mode_shows_bang_prefix() {
        use pager::app::agent_view::PromptInputMode;
        use pager::appearance::AppearanceConfig;
        let appearance = AppearanceConfig::default();
        let theme = Theme::current();
        let normal = prompt_style(&appearance, PromptInputMode::Normal, &theme, false);
        assert!(normal.prefix_override.is_none());
        assert!(normal.accent_color_override.is_none());
        assert!(normal.placeholder_override.is_none());
        let bash = prompt_style(&appearance, PromptInputMode::Bash, &theme, false);
        assert_eq!(
            bash.prefix_override,
            Some(("! ", theme.command)),
            "bash mode must paint the yellow `! ` prefix (full-TUI parity)"
        );
        assert_eq!(bash.accent_color_override, Some(theme.command));
        assert!(
            bash.placeholder_override.is_none(),
            "bash keeps the default placeholder"
        );
    }
    #[test]
    fn prompt_info_renders_model_context_and_queued() {
        let mut a = agent();
        minimal_api::set_agent_context_for_test(
            &mut a,
            shell::session::ContextInfo {
                used: 276_000,
                total: 2_000_000,
                ..Default::default()
            },
        );
        let theme = Theme::current();
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        render_prompt_info(&mut buf, area, &a, 3, "ctrl+o transcript", &theme);
        let text: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 0)).map(|c| c.symbol().to_string()))
            .collect();
        assert!(text.contains("276K"), "absolute used tokens: {text:?}");
        assert!(text.contains("2.0M"), "total context window: {text:?}");
        assert!(text.contains('%'), "percentage: {text:?}");
        assert!(text.contains("3 queued"), "queued count: {text:?}");
        assert!(
            text.trim_end().ends_with("ctrl+o transcript"),
            "trailing transcript hint: {text:?}"
        );
    }
    #[test]
    fn prompt_info_bash_mode_shows_run_shell_command() {
        use pager::app::agent_view::PromptInputMode;
        let mut a = agent();
        minimal_api::set_agent_prompt_input_mode_for_test(&mut a, PromptInputMode::Bash);
        minimal_api::set_agent_context_for_test(
            &mut a,
            shell::session::ContextInfo {
                used: 276_000,
                total: 2_000_000,
                ..Default::default()
            },
        );
        let theme = Theme::current();
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        render_prompt_info(&mut buf, area, &a, 2, "ctrl+o transcript", &theme);
        let text: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 0)).map(|c| c.symbol().to_string()))
            .collect();
        assert!(
            text.contains("Run shell command"),
            "bash mode info label: {text:?}"
        );
        assert!(
            !text.contains("276K"),
            "context usage hidden under bash mode: {text:?}"
        );
        assert!(text.contains("2 queued"), "queued still shown: {text:?}");
        assert!(
            text.trim_end().ends_with("ctrl+o transcript"),
            "transcript hint still trails: {text:?}"
        );
    }
    /// Where Ctrl+O is the interject chord (Apple Terminal) the caller passes
    /// the `/transcript` fallback, and the info row advertises that instead.
    #[test]
    fn prompt_info_shows_slash_transcript_fallback_hint() {
        let a = agent();
        let theme = Theme::current();
        let area = Rect::new(0, 0, 80, 1);
        let mut buf = Buffer::empty(area);
        render_prompt_info(&mut buf, area, &a, 0, "/transcript", &theme);
        let text: String = (0..area.width)
            .filter_map(|x| buf.cell((x, 0)).map(|c| c.symbol().to_string()))
            .collect();
        assert!(text.contains("/transcript"), "fallback hint: {text:?}");
        assert!(!text.contains("ctrl+o"), "no dead chord: {text:?}");
    }
    #[test]
    fn prompt_info_shows_session_mode_flag() {
        let theme = Theme::current();
        let area = Rect::new(0, 0, 80, 1);
        let read = |buf: &Buffer| -> String {
            (0..area.width)
                .filter_map(|x| buf.cell((x, 0)).map(|c| c.symbol().to_string()))
                .collect()
        };
        let render = |a: &pager::app::agent_view::AgentView| -> String {
            let mut buf = Buffer::empty(area);
            render_prompt_info(&mut buf, area, a, 0, "ctrl+o transcript", &theme);
            read(&buf)
        };
        let mut a = agent();
        let text = render(&a);
        assert!(text.contains("normal"), "normal behavior: {text:?}");
        assert!(text.contains("ask"), "ask permission: {text:?}");
        assert!(!text.contains("always-approve"), "normal: {text:?}");
        minimal_api::set_behavior_mode_for_test(&mut a, minimal_api::BehaviorId::Plan);
        assert!(render(&a).contains("plan"), "plan flag: {:?}", render(&a));
        minimal_api::set_behavior_mode_for_test(&mut a, minimal_api::BehaviorId::Workflow);
        assert!(
            render(&a).contains("workflow"),
            "workflow flag: {:?}",
            render(&a)
        );
        minimal_api::set_permission_mode_for_test(
            &mut a,
            shell::util::config::PermissionMode::AlwaysApprove,
        );
        let text = render(&a);
        assert!(
            text.contains("always-approve"),
            "always-approve flag: {text:?}"
        );
        minimal_api::set_permission_mode_for_test(
            &mut a,
            shell::util::config::PermissionMode::Auto,
        );
        let text = render(&a);
        assert!(text.contains("auto"), "auto flag: {text:?}");
    }
    #[test]
    fn pending_hint_formats_press_again() {
        use crossterm::event::{KeyCode, KeyModifiers};
        use pager::app::actions::Action;
        use pager::app::root::PendingAction;
        use pager::input::key::KeyShortcut;
        assert!(minimal_pending_hint(&None).is_none());
        let shortcut = KeyShortcut::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let pending = Some(PendingAction::new(Action::Quit, shortcut, "quit"));
        assert_eq!(
            minimal_pending_hint(&pending).as_deref(),
            Some("press Ctrl+q again to quit")
        );
        let silent = Some(PendingAction::with_ttl(
            Action::Quit,
            shortcut,
            None,
            std::time::Duration::from_secs(1),
        ));
        assert!(minimal_pending_hint(&silent).is_none());
        let expired = Some(PendingAction::with_ttl(
            Action::Quit,
            shortcut,
            Some("quit"),
            std::time::Duration::ZERO,
        ));
        assert!(minimal_pending_hint(&expired).is_none());
    }
}
