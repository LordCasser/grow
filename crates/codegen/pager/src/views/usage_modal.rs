//! Usage / Context / Session-info modal — three read-only tabs.
//!
//! `/usage`, `/context`, and `/session-info` open this modal in fullscreen and
//! inline screen modes. All three tabs are fed by the existing local data
//! channels (ACP ext requests to the shell), and nothing is ever written into
//! the transcript: the modal closes with Esc and its contents are gone.
//!
//! Minimal mode keeps the legacy scrollback output and never opens this modal;
//! its dispatchers pass `nonce = 0` to mark scrollback-intent fetches.

use std::sync::atomic::{AtomicU64, Ordering};

use crossterm::event::{KeyCode, KeyEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use shell::extensions::notification::PromptUsage;
use shell::session::{ContextInfo, SessionInfoResponse};

use crate::util::group_thousands;
use crate::views::modal_window::{self as mw, ModalSizing, ModalWindowState};

/// Monotonic epoch source for usage-modal fetches.
///
/// Every modal open stamps a fresh epoch into its state and into the
/// `ShowSessionInfo` / `ShowContextInfo` / `FetchSessionUsage` effects it
/// fires. Task results carry the epoch back and are applied only while the
/// modal is still open with the same epoch — a result from before a
/// close/reopen can never overwrite the newer request. `0` is reserved for
/// scrollback-intent fetches (minimal mode); epochs start at `1`.
static USAGE_FETCH_NONCE: AtomicU64 = AtomicU64::new(0);

/// Next modal-open fetch epoch (always ≥ 1; `0` means "no modal, scrollback").
pub(crate) fn next_fetch_nonce() -> u64 {
    USAGE_FETCH_NONCE.fetch_add(1, Ordering::Relaxed) + 1
}

/// The three tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageModalTab {
    Usage,
    Context,
    SessionInfo,
}

impl UsageModalTab {
    pub const ALL: [UsageModalTab; 3] = [
        UsageModalTab::Usage,
        UsageModalTab::Context,
        UsageModalTab::SessionInfo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            UsageModalTab::Usage => "Usage",
            UsageModalTab::Context => "Context",
            UsageModalTab::SessionInfo => "Session Info",
        }
    }

    pub fn index(self) -> usize {
        match self {
            UsageModalTab::Usage => 0,
            UsageModalTab::Context => 1,
            UsageModalTab::SessionInfo => 2,
        }
    }

    fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    fn prev(self) -> Self {
        Self::ALL[(self.index() + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Fetch state of one tab's data. `Loading` is the initial state after open.
#[derive(Debug, Clone)]
pub enum UsageTabData<T> {
    Loading,
    Loaded(T),
    Failed(String),
}

/// Context-tab payload: the snapshot plus the model it was captured for.
#[derive(Debug, Clone)]
pub struct ContextSnapshot {
    pub snapshot: ContextInfo,
    pub model: String,
}

/// One copyable Session Info row.
#[derive(Debug, Clone)]
pub struct SessionInfoRow {
    pub label: String,
    pub value: String,
}

/// Clickable row hit rect from the last render (Session Info tab).
#[derive(Debug, Clone, Copy)]
pub struct RowHit {
    pub index: usize,
    pub rect: Rect,
}

/// Modal state. Lives inside [`crate::views::modal::ActiveModal::Usage`].
#[derive(Debug, Clone)]
pub struct UsageModalState {
    pub active_tab: UsageModalTab,
    /// Epoch stamped at open; TaskResults carry it and are dropped on mismatch.
    pub fetch_nonce: u64,
    pub usage: UsageTabData<Box<PromptUsage>>,
    pub context: UsageTabData<ContextSnapshot>,
    pub session_info: UsageTabData<Vec<SessionInfoRow>>,
    /// Vertical scroll offset in content lines.
    pub scroll: u16,
    /// Selected Session Info row (keyboard copy target).
    pub selected_row: usize,
    /// Hovered Session Info row (mouse).
    pub hovered_row: Option<usize>,
    /// Clickable row rects from the last render.
    pub row_hits: Vec<RowHit>,
    /// Shared modal window chrome state.
    pub window: ModalWindowState,
}

impl UsageModalState {
    /// Fresh state for a new open: all three tabs start fetching.
    pub fn open(tab: UsageModalTab, nonce: u64) -> Self {
        let mut window = ModalWindowState::with_tabs(UsageModalTab::ALL.len());
        window.active_tab = tab.index();
        Self {
            active_tab: tab,
            fetch_nonce: nonce,
            usage: UsageTabData::Loading,
            context: UsageTabData::Loading,
            session_info: UsageTabData::Loading,
            scroll: 0,
            selected_row: 0,
            hovered_row: None,
            row_hits: Vec::new(),
            window,
        }
    }

    pub fn switch_tab(&mut self, tab: UsageModalTab) {
        self.active_tab = tab;
        self.window.active_tab = tab.index();
        self.scroll = 0;
        self.selected_row = 0;
        self.hovered_row = None;
        self.row_hits.clear();
    }

    /// Copy payload for a Session Info row index, or `None` when the index
    /// does not address a loaded row (or the active tab has no copyable rows).
    pub fn copy_value(&self, index: usize) -> Option<String> {
        match &self.session_info {
            UsageTabData::Loaded(rows) => rows.get(index).map(|r| r.value.clone()),
            _ => None,
        }
    }
}

/// Content-input outcome from the modal body (after the shared chrome).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageModalOutcome {
    Changed,
    Unchanged,
    CopyRow(usize),
}

/// Handle keys the modal chrome did not consume. The caller routes Esc and
/// chrome keys through [`mw::handle_modal_key`] first.
pub fn handle_key(state: &mut UsageModalState, key: &KeyEvent) -> UsageModalOutcome {
    let session_info_tab = state.active_tab == UsageModalTab::SessionInfo;
    match key.code {
        KeyCode::Tab => {
            state.switch_tab(state.active_tab.next());
            UsageModalOutcome::Changed
        }
        KeyCode::BackTab => {
            state.switch_tab(state.active_tab.prev());
            UsageModalOutcome::Changed
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if session_info_tab {
                state.selected_row = state.selected_row.saturating_sub(1);
                UsageModalOutcome::Changed
            } else {
                state.scroll = state.scroll.saturating_sub(1);
                UsageModalOutcome::Changed
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if session_info_tab {
                if let UsageTabData::Loaded(rows) = &state.session_info {
                    state.selected_row = state
                        .selected_row
                        .saturating_add(1)
                        .min(rows.len().saturating_sub(1));
                }
                UsageModalOutcome::Changed
            } else {
                state.scroll = state.scroll.saturating_add(1);
                UsageModalOutcome::Changed
            }
        }
        KeyCode::PageDown => {
            state.scroll = state.scroll.saturating_add(10);
            UsageModalOutcome::Changed
        }
        KeyCode::PageUp => {
            state.scroll = state.scroll.saturating_sub(10);
            UsageModalOutcome::Changed
        }
        KeyCode::Enter if session_info_tab => UsageModalOutcome::CopyRow(state.selected_row),
        _ => UsageModalOutcome::Unchanged,
    }
}

/// Handle mouse events the modal chrome did not consume.
pub fn handle_mouse(
    state: &mut UsageModalState,
    kind: MouseEventKind,
    column: u16,
    row: u16,
) -> UsageModalOutcome {
    match kind {
        MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
            if let Some(hit) = state
                .row_hits
                .iter()
                .find(|h| h.rect.contains((column, row).into()))
            {
                state.selected_row = hit.index;
                return UsageModalOutcome::CopyRow(hit.index);
            }
            UsageModalOutcome::Changed
        }
        MouseEventKind::Moved => {
            let hovered = state
                .row_hits
                .iter()
                .find(|h| h.rect.contains((column, row).into()))
                .map(|h| h.index);
            if hovered != state.hovered_row {
                state.hovered_row = hovered;
                UsageModalOutcome::Changed
            } else {
                UsageModalOutcome::Unchanged
            }
        }
        MouseEventKind::ScrollUp => {
            state.scroll = state.scroll.saturating_sub(1);
            UsageModalOutcome::Changed
        }
        MouseEventKind::ScrollDown => {
            state.scroll = state.scroll.saturating_add(1);
            UsageModalOutcome::Changed
        }
        _ => UsageModalOutcome::Unchanged,
    }
}

/// Build the Session Info rows from a `grow/session/info` response. Mirrors
/// the minimal-mode scrollback formatter's field selection so both surfaces
/// show the same facts.
pub fn session_info_rows(
    info: &SessionInfoResponse,
    title: Option<&str>,
    show_resolved_model: bool,
) -> Vec<SessionInfoRow> {
    let mut rows = Vec::new();
    if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
        rows.push(SessionInfoRow {
            label: "Title".to_string(),
            value: title.to_string(),
        });
    }
    rows.push(SessionInfoRow {
        label: "Shell version".to_string(),
        value: version::display_version(update::channel_label()),
    });
    rows.push(SessionInfoRow {
        label: "Auth method".to_string(),
        value: "provider BYOK".to_string(),
    });
    rows.push(SessionInfoRow {
        label: "Session ID".to_string(),
        value: info.session_id.clone(),
    });
    rows.push(SessionInfoRow {
        label: "Working directory".to_string(),
        value: info.cwd.clone(),
    });
    let model = info.data.model.as_deref().unwrap_or("unknown");
    let model_display = shell::session::model_display_name(
        info.data.model_display_name.as_deref(),
        model,
        info.data.resolved_model_id.as_deref(),
        show_resolved_model,
    );
    rows.push(SessionInfoRow {
        label: "Model".to_string(),
        value: model_display,
    });
    if shell::session::should_show_model_fingerprint(info.data.show_model_fingerprint, model)
        && let Some(fp) = info.data.model_fingerprint.as_deref()
    {
        rows.push(SessionInfoRow {
            label: "Model hash".to_string(),
            value: fp.to_string(),
        });
    }
    if let Some(backend) = info.data.api_backend.as_deref() {
        rows.push(SessionInfoRow {
            label: "API backend".to_string(),
            value: backend.to_string(),
        });
    }
    if let Some(profile) = sandbox::profile_name() {
        rows.push(SessionInfoRow {
            label: "Sandbox".to_string(),
            value: profile.to_string(),
        });
    }
    rows.push(SessionInfoRow {
        label: "Turn".to_string(),
        value: info.data.turn_index.to_string(),
    });
    let ctx = &info.data.context;
    rows.push(SessionInfoRow {
        label: "Context".to_string(),
        value: format!(
            "{} / {} tokens ({}%)",
            group_thousands(ctx.used),
            group_thousands(ctx.total),
            ctx.usage_pct
        ),
    });
    rows
}

/// Plain-text rows for the Context tab — the same breakdown the scrollback
/// `/context` block shows, without the 100-cell categorical bar.
pub fn context_rows(snapshot: &ContextInfo, model: &str) -> Vec<String> {
    let ctx = snapshot;
    let mut rows = vec![
        format!("Model: {model}"),
        format!(
            "Usage: {} / {} tokens ({}%)",
            group_thousands(ctx.used),
            group_thousands(ctx.total),
            ctx.usage_pct
        ),
        format!(
            "  System prompt  {} tokens ({})",
            group_thousands(ctx.system_prompt_tokens),
            percent_of_window(ctx.system_prompt_tokens, ctx.total)
        ),
        format!(
            "  Messages       {} tokens ({})",
            group_thousands(ctx.message_tokens),
            percent_of_window(ctx.message_tokens, ctx.total)
        ),
        format!(
            "  Free           {} tokens",
            group_thousands(ctx.free_tokens)
        ),
    ];
    for cat in &ctx.usage_categories {
        let detail = cat
            .detail
            .as_deref()
            .map(|d| format!(" · {d}"))
            .unwrap_or_default();
        rows.push(format!(
            "  {:<15} {} tokens ({}){detail}",
            cat.label,
            group_thousands(cat.tokens),
            percent_of_window(cat.tokens, ctx.total)
        ));
    }
    rows.push(format!(
        "Auto-compact at {}%",
        ctx.auto_compact_threshold_percent
    ));
    rows.push(format!(
        "Turns: {} · Tool calls: {} · Compactions: {}",
        ctx.turn_count, ctx.tool_call_count, ctx.compaction_count
    ));
    rows
}

/// `(0.6%)` share of the window; `(-)` when the total is zero.
fn percent_of_window(part: u64, total: u64) -> String {
    if total == 0 {
        "-".to_string()
    } else {
        format!("{:.1}%", part as f64 / total as f64 * 100.0)
    }
}

/// Render the modal: shared chrome (tabs, close button, footer) plus the
/// active tab's content. Refreshes [`UsageModalState::row_hits`] for mouse
/// hit-testing on the Session Info tab.
pub fn render_usage_modal(
    buf: &mut Buffer,
    area: Rect,
    state: &mut UsageModalState,
    compact: bool,
) {
    let theme = crate::theme::Theme::current();
    let labels: Vec<&str> = UsageModalTab::ALL.iter().map(|t| t.label()).collect();

    let session_info_tab = state.active_tab == UsageModalTab::SessionInfo;
    let footer: Vec<mw::Shortcut<'_>> = if session_info_tab {
        vec![
            mw::Shortcut {
                label: "\u{2191}/\u{2193} select",
                clickable: false,
                id: 0,
            },
            mw::Shortcut {
                label: "Enter copy",
                clickable: false,
                id: 1,
            },
            mw::Shortcut {
                label: "Tab switch",
                clickable: false,
                id: 2,
            },
            mw::Shortcut {
                label: "Esc close",
                clickable: false,
                id: 3,
            },
        ]
    } else {
        vec![
            mw::Shortcut {
                label: "\u{2191}/\u{2193} scroll",
                clickable: false,
                id: 0,
            },
            mw::Shortcut {
                label: "Tab switch",
                clickable: false,
                id: 1,
            },
            mw::Shortcut {
                label: "Esc close",
                clickable: false,
                id: 2,
            },
        ]
    };
    let config = mw::ModalWindowConfig {
        title: state.active_tab.label(),
        tabs: Some(&labels),
        shortcuts: &footer,
        sizing: ModalSizing::medium().with_compact(compact),
        fold_info: None,
    };
    let Some(mca) = mw::render_modal_window(buf, area, &mut state.window, &config, &theme) else {
        return;
    };

    let lines: Vec<String> = match state.active_tab {
        UsageModalTab::Usage => match &state.usage {
            UsageTabData::Loading => vec!["Loading\u{2026}".to_string()],
            UsageTabData::Failed(error) => {
                vec![format!("Couldn't load session usage: {error}")]
            }
            UsageTabData::Loaded(usage) => {
                crate::app::status_blocks::session_usage_block_text(usage)
                    .lines()
                    .map(str::to_owned)
                    .collect()
            }
        },
        UsageModalTab::Context => match &state.context {
            UsageTabData::Loading => vec!["Loading\u{2026}".to_string()],
            UsageTabData::Failed(error) => {
                vec![format!("Couldn't load context info: {error}")]
            }
            UsageTabData::Loaded(snap) => context_rows(&snap.snapshot, &snap.model),
        },
        UsageModalTab::SessionInfo => match &state.session_info {
            UsageTabData::Loading => vec!["Loading\u{2026}".to_string()],
            UsageTabData::Failed(error) => {
                vec![format!("Couldn't load session info: {error}")]
            }
            UsageTabData::Loaded(rows) => rows
                .iter()
                .map(|r| format!("{:<17} {}", r.label, r.value))
                .collect(),
        },
    };

    // Clamp scroll to the content height so short content never hides rows.
    let visible_rows = mca.content.height as usize;
    let max_scroll = lines.len().saturating_sub(visible_rows) as u16;
    if state.scroll > max_scroll {
        state.scroll = max_scroll;
    }

    state.row_hits.clear();
    if session_info_tab {
        // Row-by-row render so each row gets a hit rect and hover/selected
        // styling. Long values clip at the content width.
        let start = state.scroll as usize;
        let end = lines.len().min(start + visible_rows);
        for (y, index) in (mca.content.y..).zip(start..end) {
            let rect = Rect {
                x: mca.content.x,
                y,
                width: mca.content.width,
                height: 1,
            };
            state.row_hits.push(RowHit { index, rect });
            let hovered = state.hovered_row == Some(index);
            let selected = state.selected_row == index;
            let style = if hovered {
                Style::default().fg(theme.text_primary).bg(theme.bg_hover)
            } else {
                Style::default().fg(theme.text_primary)
            };
            let prefix = if selected {
                Span::styled("› ", Style::default().fg(theme.fuzzy_accent))
            } else {
                Span::raw("  ")
            };
            let line = Line::from(vec![prefix, Span::styled(lines[index].clone(), style)]);
            buf.set_line(mca.content.x, y, &line, mca.content.width);
        }
    } else {
        let text: Vec<Line<'_>> = lines[state.scroll as usize..]
            .iter()
            .map(|l| Line::styled(l.clone(), Style::default().fg(theme.text_primary)))
            .collect();
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(mca.content, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn info_response() -> SessionInfoResponse {
        SessionInfoResponse {
            session_id: "sess-1".to_string(),
            cwd: "/repo".to_string(),
            data: shell::session::SessionInfoData {
                agent_name: Some("grow-build".into()),
                model: Some("grow-build".into()),
                model_display_name: None,
                resolved_model_id: None,
                model_fingerprint: Some("fp-123".into()),
                show_model_fingerprint: false,
                api_backend: Some("backend-1".into()),
                turns: 3,
                turn_index: 2,
                context: ContextInfo {
                    used: 50_000,
                    total: 1_000_000,
                    system_prompt_tokens: 1_200,
                    tool_definitions_count: 12,
                    tool_definitions_tokens: 5_600,
                    compaction_count: 0,
                    turn_count: 5,
                    tool_call_count: 12,
                    message_count: 30,
                    message_tokens: 29_900,
                    free_tokens: 950_000,
                    usage_pct: 5,
                    auto_compact_threshold_percent: 85,
                    usage_categories: vec![shell::session::TokenUsageCategory {
                        label: "Skills".into(),
                        tokens: 2_400,
                        detail: Some("21 skills".into()),
                    }],
                },
            },
        }
    }

    #[test]
    fn session_info_rows_covers_core_facts_and_skips_absent() {
        let rows = session_info_rows(&info_response(), Some("my session"), true);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"Title"));
        assert!(labels.contains(&"Session ID"));
        assert!(labels.contains(&"Working directory"));
        assert!(labels.contains(&"Model"));
        // coding slug shows the fingerprint even without the catalog flag
        assert!(labels.contains(&"Model hash"));
        assert!(labels.contains(&"API backend"));
        assert!(labels.contains(&"Turn"));
        assert!(labels.contains(&"Context"));
        let session_id = rows
            .iter()
            .find(|r| r.label == "Session ID")
            .expect("session id row");
        assert_eq!(session_id.value, "sess-1");
    }

    #[test]
    fn session_info_rows_skips_empty_title_and_absent_backend() {
        let mut info = info_response();
        info.data.api_backend = None;
        let rows = session_info_rows(&info, Some("   "), false);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert!(!labels.contains(&"Title"), "blank title must be omitted");
        assert!(
            !labels.contains(&"API backend"),
            "absent backend must be omitted"
        );
    }

    #[test]
    fn context_rows_report_categories_and_auto_compact() {
        let ctx = info_response().data.context;
        let rows = context_rows(&ctx, "grow-build");
        let text = rows.join("\n");
        assert!(text.contains("Model: grow-build"), "{text}");
        assert!(text.contains("System prompt"), "{text}");
        assert!(text.contains("Skills"), "{text}");
        assert!(text.contains("21 skills"), "{text}");
        assert!(text.contains("Auto-compact at 85%"), "{text}");
        assert!(
            text.contains("Turns: 5 · Tool calls: 12 · Compactions: 0"),
            "{text}"
        );
    }

    #[test]
    fn percent_of_window_is_dash_when_total_is_zero() {
        assert_eq!(percent_of_window(0, 0), "-");
        assert_eq!(percent_of_window(1, 0), "-");
        assert_eq!(percent_of_window(25, 100), "25.0%");
    }

    #[test]
    fn tab_switch_resets_scroll_selection_and_window_index() {
        let mut state = UsageModalState::open(UsageModalTab::Usage, 1);
        state.scroll = 5;
        state.selected_row = 3;
        state.switch_tab(UsageModalTab::Context);
        assert_eq!(state.active_tab, UsageModalTab::Context);
        assert_eq!(state.window.active_tab, 1);
        assert_eq!(state.scroll, 0);
        assert_eq!(state.selected_row, 0);
        assert_eq!(state.fetch_nonce, 1);
    }

    #[test]
    fn key_tab_cycles_forward_and_backtab_backward() {
        let mut state = UsageModalState::open(UsageModalTab::Usage, 1);
        assert_eq!(
            handle_key(&mut state, &key(KeyCode::Tab)),
            UsageModalOutcome::Changed
        );
        assert_eq!(state.active_tab, UsageModalTab::Context);
        assert_eq!(
            handle_key(&mut state, &key(KeyCode::BackTab)),
            UsageModalOutcome::Changed
        );
        assert_eq!(state.active_tab, UsageModalTab::Usage);
        // wraps around both directions
        let mut wrapped = UsageModalState::open(UsageModalTab::SessionInfo, 1);
        handle_key(&mut wrapped, &key(KeyCode::Tab));
        assert_eq!(wrapped.active_tab, UsageModalTab::Usage);
    }

    #[test]
    fn key_selection_moves_on_session_info_and_clamps() {
        let mut state = UsageModalState::open(UsageModalTab::SessionInfo, 1);
        state.session_info = UsageTabData::Loaded(vec![
            SessionInfoRow {
                label: "A".into(),
                value: "1".into(),
            },
            SessionInfoRow {
                label: "B".into(),
                value: "2".into(),
            },
        ]);
        handle_key(&mut state, &key(KeyCode::Down));
        assert_eq!(state.selected_row, 1);
        handle_key(&mut state, &key(KeyCode::Down));
        assert_eq!(state.selected_row, 1, "clamped at last row");
        handle_key(&mut state, &key(KeyCode::Up));
        handle_key(&mut state, &key(KeyCode::Up));
        assert_eq!(state.selected_row, 0, "clamped at first row");
        assert_eq!(
            handle_key(&mut state, &key(KeyCode::Enter)),
            UsageModalOutcome::CopyRow(0)
        );
        assert_eq!(state.copy_value(0).as_deref(), Some("1"));
    }

    #[test]
    fn key_scroll_on_non_row_tabs() {
        let mut state = UsageModalState::open(UsageModalTab::Usage, 1);
        handle_key(&mut state, &key(KeyCode::Down));
        handle_key(&mut state, &key(KeyCode::Down));
        assert_eq!(state.scroll, 2);
        handle_key(&mut state, &key(KeyCode::PageDown));
        assert_eq!(state.scroll, 12);
        handle_key(&mut state, &key(KeyCode::PageUp));
        handle_key(&mut state, &key(KeyCode::Up));
        assert_eq!(state.scroll, 1);
    }

    #[test]
    fn mouse_click_copies_row_and_move_tracks_hover() {
        let mut state = UsageModalState::open(UsageModalTab::SessionInfo, 1);
        state.session_info = UsageTabData::Loaded(vec![SessionInfoRow {
            label: "Session ID".into(),
            value: "sess-1".into(),
        }]);
        state.row_hits.push(RowHit {
            index: 0,
            rect: Rect {
                x: 2,
                y: 3,
                width: 20,
                height: 1,
            },
        });
        // hover
        assert_eq!(
            handle_mouse(&mut state, MouseEventKind::Moved, 5, 3),
            UsageModalOutcome::Changed
        );
        assert_eq!(state.hovered_row, Some(0));
        // click copies
        assert_eq!(
            handle_mouse(
                &mut state,
                MouseEventKind::Down(crossterm::event::MouseButton::Left),
                5,
                3
            ),
            UsageModalOutcome::CopyRow(0)
        );
        assert_eq!(state.selected_row, 0);
        // click on empty space is consumed without copying
        assert_eq!(
            handle_mouse(
                &mut state,
                MouseEventKind::Down(crossterm::event::MouseButton::Left),
                5,
                9
            ),
            UsageModalOutcome::Changed
        );
    }

    #[test]
    fn copy_value_none_without_loaded_rows() {
        let state = UsageModalState::open(UsageModalTab::SessionInfo, 1);
        assert_eq!(state.copy_value(0), None);
    }

    #[test]
    fn render_usage_modal_draws_tabs_and_content() {
        let mut state = UsageModalState::open(UsageModalTab::Usage, 1);
        let usage = PromptUsage::default();
        state.usage = UsageTabData::Loaded(Box::new(usage));
        let area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        };
        let mut buf = Buffer::empty(area);
        render_usage_modal(&mut buf, area, &mut state, false);
        // tab labels visible on screen
        assert!(contains_text(&buf, "Usage"), "usage tab label");
        assert!(contains_text(&buf, "Context"), "context tab label");
        assert!(
            contains_text(&buf, "Session Info"),
            "session info tab label"
        );
        assert!(
            contains_text(&buf, "no model calls yet"),
            "empty-ledger message must render"
        );
    }

    #[test]
    fn render_session_info_tab_records_row_hits() {
        let mut state = UsageModalState::open(UsageModalTab::SessionInfo, 1);
        state.session_info = UsageTabData::Loaded(vec![SessionInfoRow {
            label: "Session ID".to_string(),
            value: "sess-1".to_string(),
        }]);
        let area = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 30,
        };
        let mut buf = Buffer::empty(area);
        render_usage_modal(&mut buf, area, &mut state, false);
        assert_eq!(state.row_hits.len(), 1, "one hit per loaded row");
        assert!(contains_text(&buf, "sess-1"), "row value must render");
    }

    fn contains_text(buf: &Buffer, needle: &str) -> bool {
        (0..buf.area.height).any(|y| {
            let mut line = String::new();
            for x in 0..buf.area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            line.contains(needle)
        })
    }
}
