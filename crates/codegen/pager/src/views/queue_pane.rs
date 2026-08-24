//! Queue pane — renders queued prompts in a `ListPane`.
//!
//! Similar to [`super::todo_pane::TodoPane`] but for queued prompts.
//! Shows `#1`, `#2`, … prefixes (positional, 1-based) with the first
//! line of each prompt's text. Multiline prompts show a `(+N lines)` indicator.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::agent::{QueueEntryKind, QueuedPrompt};
use crate::app::prompt_queue::QueueEntryWire;
use crate::render::line_utils::truncate_str;
use crate::theme::{Theme, ThemeKind};

use super::list_pane::ListItem;

/// Server row is visible in the held pane when it is not the foreground turn.
#[inline]
pub(crate) fn visible_held_server_row(id: &str, running_id: Option<&str>) -> bool {
    Some(id) != running_id
}

// ---------------------------------------------------------------------------
// QueuedPromptEntry — ListItem wrapper around QueuedPrompt
// ---------------------------------------------------------------------------

/// Where a rendered queue row originates, which determines how an edit
/// (delete / reorder) is routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueRowOrigin {
    /// A client-local `pending_prompts` entry (skill/image/bash/cron/etc., or
    /// any idle-drained prompt). Edits mutate the local queue directly.
    Local,
    /// A server-authoritative `shared_prompt_queues` entry (plain prompt queued
    /// while running). Edits route to the agent as `grow/queue/*`.
    Server,
}

/// A resolved reference to a queue row, used by the edit handlers to route by
/// origin. Returned by [`QueuePane::row_ref`].
#[derive(Debug, Clone)]
pub struct QueueRowRef {
    pub origin: QueueRowOrigin,
    /// The server `prompt_id` for `Server` rows; `None` for `Local` rows.
    pub server_id: Option<String>,
    /// The server queue-entry version (for versioned removes); 0 for local.
    pub version: u64,
}

/// A `QueuedPrompt` wrapped for display in a `ListPane`.
#[derive(Debug, Clone)]
pub struct QueuedPromptEntry {
    /// Stable selection ID. For local rows this is the `QueuedPrompt.id`
    /// monotonic counter; for server rows it is a stable hash of the server
    /// `prompt_id` (so `ListPane` selection works uniformly across origins).
    pub id: u64,
    /// 1-based positional index for display (`#1`, `#2`, …).
    pub position: usize,
    /// The full prompt text (for search/copy).
    pub text: String,
    /// First non-empty line (trimmed) for display.
    first_line: String,
    /// Total line count in the prompt.
    line_count: usize,
    /// Entry kind (Prompt or Command).
    kind: QueueEntryKind,
    /// Row origin — Local (client queue) or Server (shared queue).
    origin: QueueRowOrigin,
    /// Server `prompt_id` for `Server` rows; `None` for local rows.
    server_id: Option<String>,
    /// Server queue-entry version (for versioned removes); 0 for local.
    version: u64,
    /// Cached styled content (rebuilt with width in `rebuild_styled_for_width`).
    styled: Line<'static>,
}

/// Stable 64-bit id derived from a server `prompt_id` string, so server rows
/// have a `u64` selection id like local rows. Collision with local monotonic
/// counters (small integers) is astronomically unlikely.
fn synth_server_id(prompt_id: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prompt_id.hash(&mut h);
    // Set the top bit so a synthesized id can never equal a small local
    // monotonic counter (which start at 0 and increment by 1).
    h.finish() | (1 << 63)
}

/// Map a shared-queue wire `kind` string to the display [`QueueEntryKind`].
/// Unknown kinds fall back to a plain prompt.
pub fn kind_from_wire(kind: &str) -> QueueEntryKind {
    match kind {
        "bash" => QueueEntryKind::BashCommand,
        "cron" => QueueEntryKind::Cron,
        "command" => QueueEntryKind::Command,
        _ => QueueEntryKind::Prompt,
    }
}

impl QueuedPromptEntry {
    /// Create a new entry from a `QueuedPrompt` and its current position.
    pub fn new(prompt: &QueuedPrompt, position: usize) -> Self {
        // Show first non-empty line, trimmed.
        let first_line = prompt
            .text
            .lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();

        let line_count = prompt.text.lines().count();

        // Build initial styled line (will be rebuilt with proper width later).
        let styled = Self::build_styled(&first_line, line_count, prompt.kind, None);

        Self {
            id: prompt.id,
            position,
            text: prompt.text.clone(),
            first_line,
            line_count,
            kind: prompt.kind,
            origin: QueueRowOrigin::Local,
            server_id: None,
            version: 0,
            styled,
        }
    }

    /// Create an entry from a server-authoritative shared-queue wire entry.
    /// The wire `kind` selects the display style: plain prompts, bash
    /// (`! ` prefix), and the agent-driven cron kind.
    pub fn from_server(wire: &QueueEntryWire, position: usize) -> Self {
        let first_line = wire
            .text
            .lines()
            .map(|l| l.trim())
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .to_string();
        let line_count = wire.text.lines().count();
        let kind = kind_from_wire(&wire.kind);
        let styled = Self::build_styled(&first_line, line_count, kind, None);
        Self {
            id: synth_server_id(&wire.id),
            position,
            text: wire.text.clone(),
            first_line,
            line_count,
            kind,
            origin: QueueRowOrigin::Server,
            server_id: Some(wire.id.clone()),
            version: wire.version,
            styled,
        }
    }

    /// Rebuild the styled content with proper truncation for the given width.
    ///
    /// This ensures the `(+N lines)` suffix is always visible by truncating
    /// the first line content to make room.
    pub fn rebuild_styled_for_width(&mut self, available_width: u16) {
        self.styled = Self::build_styled(
            &self.first_line,
            self.line_count,
            self.kind,
            Some(available_width as usize),
        );
    }

    /// Build the styled `Line` for display.
    ///
    /// If `max_width` is provided and the entry is multiline, truncates the
    /// first line content to ensure the `(+N lines)` suffix fits.
    fn build_styled(
        first_line: &str,
        line_count: usize,
        kind: QueueEntryKind,
        max_width: Option<usize>,
    ) -> Line<'static> {
        let theme = Theme::current();
        let extra_lines = line_count.saturating_sub(1);

        // Build the suffix for multiline prompts: " (+N lines)" or " (+1 line)"
        let suffix = if extra_lines > 0 {
            if extra_lines == 1 {
                " (+1 line)".to_string()
            } else {
                format!(" (+{extra_lines} lines)")
            }
        } else {
            String::new()
        };
        let suffix_width = suffix.width();

        // Determine how much space we have for the first line content.
        // Reserve space for the suffix if multiline.
        let content_max_width = max_width.map(|w| {
            if extra_lines > 0 {
                w.saturating_sub(suffix_width)
            } else {
                w
            }
        });

        match kind {
            QueueEntryKind::Prompt => {
                let display_text = if let Some(max_w) = content_max_width {
                    truncate_str(first_line, max_w)
                } else {
                    first_line.to_string()
                };

                let mut spans = vec![Span::styled(
                    display_text,
                    Style::default().fg(theme.accent_user),
                )];

                if extra_lines > 0 {
                    spans.push(Span::styled(suffix, Style::default().fg(theme.gray)));
                }
                Line::from(spans)
            }
            QueueEntryKind::Command => {
                // Slash commands: `/command` in magenta, args (if any) in bright gray.
                let trimmed = first_line.trim();

                // For commands, we need to be smarter about truncation.
                // Truncate the whole thing first, then split.
                let truncated = if let Some(max_w) = content_max_width {
                    truncate_str(trimmed, max_w)
                } else {
                    trimmed.to_string()
                };

                let mut spans = if let Some(space_idx) = truncated.find(' ') {
                    let (cmd, args) = truncated.split_at(space_idx);
                    vec![
                        Span::styled(cmd.to_string(), Style::default().fg(theme.accent_assistant)),
                        Span::styled(args.to_string(), Style::default().fg(theme.gray_bright)),
                    ]
                } else {
                    vec![Span::styled(
                        truncated,
                        Style::default().fg(theme.accent_assistant),
                    )]
                };

                if extra_lines > 0 {
                    spans.push(Span::styled(suffix, Style::default().fg(theme.gray)));
                }

                Line::from(spans)
            }
            QueueEntryKind::BashCommand => {
                // Bash commands: `! ` prefix in yellow (theme.command), command text in yellow.
                let display_text = if let Some(max_w) = content_max_width {
                    // Reserve 2 chars for "! " prefix.
                    truncate_str(first_line, max_w.saturating_sub(2))
                } else {
                    first_line.to_string()
                };

                let mut spans = vec![
                    Span::styled("! ", Style::default().fg(theme.command)),
                    Span::styled(display_text, Style::default().fg(theme.command)),
                ];

                if extra_lines > 0 {
                    spans.push(Span::styled(suffix, Style::default().fg(theme.gray)));
                }

                Line::from(spans)
            }
            QueueEntryKind::Cron => {
                let display_text = if let Some(max_w) = content_max_width {
                    truncate_str(first_line, max_w.saturating_sub(2))
                } else {
                    first_line.to_string()
                };

                let mut spans = vec![
                    Span::styled("\u{21BB}  ", Style::default().fg(theme.gray_dim)),
                    Span::styled(display_text, Style::default().fg(theme.accent_user)),
                ];

                if extra_lines > 0 {
                    spans.push(Span::styled(suffix, Style::default().fg(theme.gray)));
                }

                Line::from(spans)
            }
        }
    }
}

impl ListItem for QueuedPromptEntry {
    fn content(&self) -> &Line<'_> {
        &self.styled
    }

    fn prefix(&self) -> Option<Line<'_>> {
        let theme = Theme::current();
        Some(Line::from(vec![
            Span::styled(
                format!("#{}", self.position),
                Style::default().fg(theme.gray),
            ),
            Span::raw(" "),
        ]))
    }

    fn stable_id(&self) -> u64 {
        self.id
    }

    fn search_text(&self) -> &str {
        &self.text
    }

    fn copy_text(&self) -> String {
        self.text.clone()
    }
}

// ---------------------------------------------------------------------------
// QueuePane — self-contained pane owning entries, state, and rendering
// ---------------------------------------------------------------------------

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::StatefulWidget;

use crate::appearance::LayoutConfig;
use crate::render::{PreviewConfig, PreviewStyle, render_preview_overlay};

use super::list_pane::{ListPane, ListPaneConfig, ListPaneState, ListPaneStyle, WrapMode};
use super::overlay::OverlayState;

/// Event returned by [`QueuePane::handle_key`] signaling intent to the caller.
///
/// The queue pane never mutates business state (the prompt queue) directly.
/// It signals intent via these events, and `AgentView::handle_queue_key`
/// performs the actual mutation on `AgentSession::pending_prompts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueEvent {
    /// Delete the selected prompt from the queue.
    DeleteSelected { id: u64 },
    /// Edit the selected prompt (load into prompt widget).
    EditSelected { id: u64 },
    /// Swap the selected prompt with its neighbor above.
    SwapUp { id: u64 },
    /// Swap the selected prompt with its neighbor below.
    SwapDown { id: u64 },
    /// Force-send the selected prompt as a mid-turn interjection.
    ForceInterject { id: u64 },
}

/// Maximum height (in lines) the queue pane will request.
const MAX_QUEUE_HEIGHT: u16 = 3;

/// Hit-test + hover state for one per-row action button (`[edit]`,
/// `[Send now]`, `[cancel]`). Rect and row binding are rebound on every
/// `QueuePane::render`; hover persists across frames so a stationary
/// pointer keeps its highlight.
#[derive(Default)]
struct RowActionButton {
    /// Screen rect from the last render; `None` when the button didn't render.
    rect: Option<Rect>,
    /// Entry id the rendered button acts on.
    entry_id: Option<u64>,
    /// Entry id whose button is under the mouse, if any. Drives the button's
    /// hover fg color.
    hovered_id: Option<u64>,
}

impl RowActionButton {
    /// Drop the previous frame's rect/row binding (start of each render).
    fn reset(&mut self) {
        self.rect = None;
        self.entry_id = None;
    }

    /// Bind the just-rendered button to its screen rect and row.
    fn bind(&mut self, rect: Rect, id: u64) {
        self.rect = Some(rect);
        self.entry_id = Some(id);
    }

    /// Row id the button acts on when `(col, row)` hits its rect. `fallback`
    /// is the pane's selected row id.
    fn hit(&self, col: u16, row: u16, fallback: Option<u64>) -> Option<u64> {
        let rect = self.rect?;
        if rect.contains((col, row).into()) {
            self.entry_id.or(fallback)
        } else {
            None
        }
    }

    /// Update the hover from the mouse position. Returns `true` if it
    /// changed (caller redraws).
    fn update_hover(&mut self, col: u16, row: u16, fallback: Option<u64>) -> bool {
        let over = self.rect.is_some_and(|r| r.contains((col, row).into()));
        let new_id = if over {
            self.entry_id.or(fallback)
        } else {
            None
        };
        if new_id != self.hovered_id {
            self.hovered_id = new_id;
            true
        } else {
            false
        }
    }

    /// Clear the hover (mouse left the queue pane). Returns `true` if it was
    /// previously hovered (caller should redraw).
    fn clear_hover(&mut self) -> bool {
        if self.hovered_id.is_some() {
            self.hovered_id = None;
            true
        } else {
            false
        }
    }

    /// Whether the button bound to row `id` is under the mouse.
    fn is_hovered_for(&self, id: u64) -> bool {
        self.hovered_id == Some(id)
    }
}

/// Self-contained queue pane component.
///
/// Does NOT own the queue data — entries are rebuilt each frame from
/// `AgentSession::pending_prompts`. Owns `ListPaneState` for scroll,
/// selection, and rendering state.
pub struct QueuePane {
    /// Styled entries for `ListPane` rendering.
    /// Rebuilt from the queue at the start of each `render()` call.
    entries: Vec<QueuedPromptEntry>,
    /// List pane state (scroll, selection, search, layout cache).
    pub list_state: ListPaneState,
    /// Visual style for the list pane framework.
    list_style: ListPaneStyle,
    /// Theme kind at the last render. Used to detect a theme switch and
    /// refresh `list_style`, whose `selection_bg` is captured from the theme
    /// (otherwise the focused-row highlight keeps the previous theme's
    /// `bg_highlight` — e.g. GrowNight's dark band leaking into GrowDay).
    last_theme: ThemeKind,
    /// Shared visibility/focus state.
    pub overlay: OverlayState,
    /// Previous queue length — used for auto-show detection.
    prev_len: usize,
    /// `[Send now]` (force-interject) action button.
    send_now: RowActionButton,
    /// `[cancel]` (row delete) action button.
    delete_button: RowActionButton,
    /// `[edit]` (queued-row edit) action button.
    edit_button: RowActionButton,
    /// Entry id of the row currently under the mouse cursor, if any. Drives
    /// the hover affordance: the hovered row reveals its action buttons just
    /// like the selected row does when the pane is focused.
    hovered_row_id: Option<u64>,
    /// Content area from the last `render`, used to hit-test which row the
    /// mouse is over (for `hovered_row_id`).
    last_inner: Option<Rect>,
}

impl Default for QueuePane {
    fn default() -> Self {
        Self::new()
    }
}

impl QueuePane {
    /// Create a new empty queue pane.
    pub fn new() -> Self {
        let config = ListPaneConfig {
            follow_enabled: false,
            wrap_toggle_enabled: false,
            search_enabled: false, // Queue is small, no search needed
            copy_enabled: true,
            show_selection_when_unfocused: false,
            visual_select_enabled: false,
            filter_enabled: false,
            goto_line_enabled: false,
        };
        let mut list_state = ListPaneState::new_with_config(WrapMode::NoWrap, false, config);
        list_state.set_clipboard_provider(Box::new(crate::clipboard::SystemClipboard));
        Self {
            entries: Vec::new(),
            list_state,
            list_style: ListPaneStyle::default(),
            last_theme: Theme::current_kind(),
            overlay: OverlayState::hidden(),
            prev_len: 0,
            send_now: RowActionButton::default(),
            delete_button: RowActionButton::default(),
            edit_button: RowActionButton::default(),
            hovered_row_id: None,
            last_inner: None,
        }
    }

    // -- Data management -----------------------------------------------------

    /// Rebuild the queue rows from the **union** of the local drip-feed queue
    /// (`local`) and the server-authoritative shared queue (`server`), tagging
    /// each row with its origin so edits route correctly.
    ///
    /// Merge ordering rule: **server rows first** (in their broadcast
    /// `position` order), then **local rows** (in `pending_prompts` order).
    /// Rationale: server (plain) prompts were already accepted into the agent's
    /// authoritative FIFO; local rows (skill/image/bash/cron, or plain prompts
    /// queued before the session/turn was ready) wait behind them. This is only
    /// correct because every server-queued prompt is older than every local
    /// one, an invariant enforced by `immediate_server_send_eligible`: a prompt
    /// may take the immediate-send (server) path only while the local queue is
    /// empty, so a newer prompt can never sit on the server queue ahead of an
    /// older local prompt. The two queues are disjoint by construction
    /// (plain-while-running routes to the server queue and everything else
    /// local), so no row appears in both.
    ///
    /// `running_id` is excluded via [`visible_held_server_row`].
    pub fn sync_from_merged(
        &mut self,
        local: &std::collections::VecDeque<QueuedPrompt>,
        server: &[QueueEntryWire],
        running_id: Option<&str>,
    ) {
        // Rebuild entries; positions are recomputed 1-based across the union.
        self.entries.clear();
        let mut pos = 1;
        for wire in server {
            if !visible_held_server_row(&wire.id, running_id) {
                continue;
            }
            self.entries.push(QueuedPromptEntry::from_server(wire, pos));
            pos += 1;
        }
        for prompt in local {
            self.entries.push(QueuedPromptEntry::new(prompt, pos));
            pos += 1;
        }

        let new_len = self.entries.len();

        // Auto-show when hidden and queue grew, or empty→non-empty (after external hide reset).
        if new_len > 0 && !self.overlay.visible && (self.prev_len == 0 || new_len > self.prev_len) {
            self.overlay.visible = true;
        }

        if new_len == 0 && self.overlay.visible {
            self.overlay.visible = false;
            self.overlay.focused = false;
        }

        // Hidden+empty: pin prev_len so the next enqueue always recovers.
        if new_len == 0 && !self.overlay.visible {
            self.prev_len = 0;
        } else {
            self.prev_len = new_len;
        }
    }

    /// Selection ids of the current rows, in display order.
    pub fn entry_ids(&self) -> Vec<u64> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// Resolve a row's origin metadata by its selection id (for edit routing).
    pub fn row_ref(&self, id: u64) -> Option<QueueRowRef> {
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| QueueRowRef {
                origin: e.origin,
                server_id: e.server_id.clone(),
                version: e.version,
            })
    }

    /// Whether the pane should be visible in the layout.
    pub fn is_visible(&self) -> bool {
        self.overlay.visible && !self.entries.is_empty()
    }

    /// Called after overlay state changes that hide the pane.
    pub fn on_state_change(&mut self) {
        if !self.overlay.visible {
            self.list_state.close_input_bar();
        }
    }

    /// Reset auto-show edge after external hide on an empty merged queue.
    pub fn reset_auto_show_edge(&mut self) {
        self.prev_len = 0;
    }

    /// Desired height in lines for layout computation.
    ///
    /// Returns 0 when hidden or empty.
    /// Otherwise: `min(queue.len(), MAX_QUEUE_HEIGHT)`.
    pub fn desired_height(&self) -> u16 {
        if !self.is_visible() {
            return 0;
        }
        (self.entries.len() as u16).clamp(1, MAX_QUEUE_HEIGHT)
    }

    // -- Input handling ------------------------------------------------------

    /// Handle a key event when the queue pane is focused.
    ///
    /// Returns `Some(QueueEvent)` for queue-specific actions (delete, edit,
    /// reorder). Returns `None` if the key wasn't a queue action — the caller
    /// should then try `handle_navigation_key` for j/k/y etc.
    pub fn handle_key(
        &self,
        key: &KeyEvent,
        registry: &crate::actions::ActionRegistry,
    ) -> Option<QueueEvent> {
        let id = self.selected_id()?;
        if registry.matches_id(crate::actions::ActionId::InterjectPrompt, key) {
            return Some(QueueEvent::ForceInterject { id });
        }
        match key.code {
            KeyCode::Char('x') | KeyCode::Delete | KeyCode::Backspace => {
                Some(QueueEvent::DeleteSelected { id })
            }
            KeyCode::Char('e') | KeyCode::Enter => Some(QueueEvent::EditSelected { id }),
            KeyCode::Char('J')
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::NONE) =>
            {
                // Shift+J or uppercase J (same thing in practice)
                if key.code == KeyCode::Char('J') {
                    Some(QueueEvent::SwapDown { id })
                } else {
                    None
                }
            }
            KeyCode::Char('K')
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::NONE) =>
            {
                if key.code == KeyCode::Char('K') {
                    Some(QueueEvent::SwapUp { id })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Handle navigation keys (j/k, y to copy, scrolling, etc.).
    ///
    /// Delegates to ListPane. Returns `true` if consumed.
    pub fn handle_navigation_key(&mut self, key: &KeyEvent) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.list_state.handle_key_event(key, &self.entries)
    }

    pub fn handle_paste(&mut self, text: &str) -> bool {
        self.list_state.handle_paste(text, &self.entries)
    }

    /// Get the stable ID of the currently selected entry, if any.
    pub fn selected_id(&self) -> Option<u64> {
        self.list_state.selected_id()
    }

    /// Get the full text of the currently selected entry, if any.
    ///
    /// Used to determine if the entry is multiline (for preview overlay).
    pub fn selected_text(&self) -> Option<&str> {
        let id = self.list_state.selected_id()?;
        self.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.text.as_str())
    }

    /// Select the deleted row's neighbor (so the cursor doesn't jump to the
    /// top), resolved from the merged `entries` to stay correct across the
    /// server/local boundary. Call before the next `sync_from_merged` rebuild.
    pub fn select_after_delete(&mut self, deleted_id: u64) {
        let Some(pos) = self.entries.iter().position(|e| e.id == deleted_id) else {
            return;
        };
        // `pos` exists, so `entries` is non-empty and `len() - 1` is safe.
        let survivors = self.entries.len() - 1;
        if survivors == 0 {
            return;
        }
        let new_pos = pos.min(survivors - 1);
        if let Some(entry) = self
            .entries
            .iter()
            .filter(|e| e.id != deleted_id)
            .nth(new_pos)
        {
            self.list_state.select_by_id(entry.id);
        }
    }

    /// Handle a mouse scroll event over the queue pane area.
    pub fn handle_scroll(&mut self, lines: i32, col: u16, row: u16) {
        let max = match self.list_state.viewport_height() {
            0..=5 => 1,
            _ => lines.unsigned_abs() as i32,
        };
        let capped = lines.signum() * lines.abs().min(max);
        self.list_state
            .handle_scroll_event(capped, col, row, &self.entries);
        // Rows move under a stationary cursor while wheel-scrolling, but hover
        // is otherwise only refreshed on `Moved` events. Re-resolve the hovered
        // row from the pointer's current position against the just-updated
        // scroll offset, so the hover bg and action buttons track the row now
        // under the pointer instead of the pre-scroll entry.
        self.update_row_hover(col, row);
    }

    /// Handle a mouse click/drag event over the queue pane area.
    ///
    /// Returns `true` if the event was consumed.
    pub fn handle_mouse(&mut self, kind: MouseEventKind, col: u16, row: u16, area: Rect) -> bool {
        self.list_state
            .handle_mouse_event(kind, col, row, area, &self.entries)
    }

    /// Hit-test the action-row `[Interject]` button. Returns the id of the row
    /// the button belongs to (the hovered row, else the selected row).
    pub fn send_now_click(&self, col: u16, row: u16) -> Option<u64> {
        self.send_now.hit(col, row, self.selected_id())
    }

    /// Hit-test the action-row `[cancel]` button (removes the row).
    pub fn delete_click(&self, col: u16, row: u16) -> Option<u64> {
        self.delete_button.hit(col, row, self.selected_id())
    }

    /// Hit-test the action-row `[edit]` button (opens the queued-row edit).
    pub fn edit_click(&self, col: u16, row: u16) -> Option<u64> {
        self.edit_button.hit(col, row, self.selected_id())
    }

    /// Update the `[cancel]` hover. Returns `true` if it changed (caller redraws).
    pub fn update_delete_hover(&mut self, col: u16, row: u16) -> bool {
        let selected = self.selected_id();
        self.delete_button.update_hover(col, row, selected)
    }

    /// Clear the `[cancel]` hover. Returns `true` if it was previously hovered.
    pub fn clear_delete_hover(&mut self) -> bool {
        self.delete_button.clear_hover()
    }

    /// Update the `[Interject]` hover (drives the brighten-on-hover fg, same
    /// as the `[Dashboard]` button). Returns `true` if it changed.
    pub fn update_send_now_hover(&mut self, col: u16, row: u16) -> bool {
        let selected = self.selected_id();
        self.send_now.update_hover(col, row, selected)
    }

    /// Clear the `[Interject]` hover. Returns `true` if it was previously hovered.
    pub fn clear_send_now_hover(&mut self) -> bool {
        self.send_now.clear_hover()
    }

    /// Update the `[edit]` hover. Returns `true` if it changed (caller redraws).
    pub fn update_edit_hover(&mut self, col: u16, row: u16) -> bool {
        let selected = self.selected_id();
        self.edit_button.update_hover(col, row, selected)
    }

    /// Clear the `[edit]` hover. Returns `true` if it was previously hovered.
    pub fn clear_edit_hover(&mut self) -> bool {
        self.edit_button.clear_hover()
    }

    /// Update which row the mouse is hovering over (the row, not just its
    /// delete button). Drives the action-button reveal on hover. Returns
    /// `true` if the hovered row changed (caller should redraw).
    pub fn update_row_hover(&mut self, col: u16, row: u16) -> bool {
        let new_id = self.row_id_at(col, row);
        if new_id != self.hovered_row_id {
            self.hovered_row_id = new_id;
            true
        } else {
            false
        }
    }

    /// Clear the hovered row (mouse left the queue pane). Returns `true` if a
    /// row was previously hovered (caller should redraw).
    pub fn clear_row_hover(&mut self) -> bool {
        if self.hovered_row_id.is_some() {
            self.hovered_row_id = None;
            true
        } else {
            false
        }
    }

    /// Resolve the entry id at screen position `(col, row)` using the last
    /// rendered content area and the list layout. `None` when the position is
    /// outside the rows.
    fn row_id_at(&self, col: u16, row: u16) -> Option<u64> {
        let inner = self.last_inner?;
        if !inner.contains((col, row).into()) {
            return None;
        }
        let ry = row.saturating_sub(inner.y) as usize;
        let vy = self.list_state.scroll_offset() + ry;
        let idx = self.list_state.layout().item_at_y(vy)?;
        self.entries.get(idx).map(|e| e.id)
    }

    // -- Rendering -----------------------------------------------------------

    /// Compute the inner content area for the queue rows.
    ///
    /// Indents one column less than scrollback content (which uses
    /// `ACCENT + block_pad_left`) so the `#N` prefix vertically aligns with the
    /// turn-status loading indicator rendered just below — it shares this exact
    /// `ACCENT + block_pad_left - 1` left indent (see `agent_view.rs`).
    ///
    /// The right edge is NOT inset: it extends to the full area width so the
    /// row's right-aligned action buttons (`[cancel]`) and hover bg line up with
    /// the turn-status line's `[stop]` button just below, which is likewise
    /// right-aligned to the full width (it insets only on the left).
    fn content_area(area: Rect, layout_cfg: &LayoutConfig) -> Rect {
        use crate::scrollback::layout::HorizontalLayout;
        let pad_left = HorizontalLayout::ACCENT + layout_cfg.block_pad_left.saturating_sub(1);
        Rect {
            x: area.x + pad_left,
            y: area.y,
            width: area.width.saturating_sub(pad_left),
            height: area.height,
        }
    }

    /// Render the queue pane into the given area.
    ///
    /// If `overlay_area` is provided and the selected entry is multiline,
    /// renders a preview overlay showing the full content.
    ///
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        focused: bool,
        layout_cfg: &LayoutConfig,
        overlay_area: Option<Rect>,
        is_turn_running: bool,
    ) {
        // Detect a theme switch and refresh the list chrome style. Its
        // `selection_bg` (the focused-row highlight) is captured from the
        // theme's `bg_highlight`; without this it would keep the theme active
        // at construction (default GrowNight, dark) after the user switches —
        // painting a dark band on a light GrowDay canvas.
        let current_theme = Theme::current_kind();
        if current_theme != self.last_theme {
            self.last_theme = current_theme;
            self.list_style = ListPaneStyle::default();
        }

        let inner = Self::content_area(area, layout_cfg);
        self.last_inner = Some(inner);
        if self.entries.is_empty() {
            return;
        }

        // Rebuild styled content with proper width for truncation.
        // Account for prefix width: "#N " where N is the position (1-based).
        // Max position determines prefix width: #1-#9 = 3 chars, #10-#99 = 4 chars, etc.
        let max_pos = self.entries.len();
        let prefix_width = 2 + digit_count(max_pos); // "#" + digits + " "
        let content_width = (inner.width as usize).saturating_sub(prefix_width);
        for entry in &mut self.entries {
            entry.rebuild_styled_for_width(content_width as u16);
        }

        // When the queue overflows, the ListPane reserves its scrollbar in the
        // last column of its render area. That column is where the right-aligned
        // action buttons ([cancel]) sit, so render the list one column wider —
        // borrowing a column of the right outer padding — to push the scrollbar
        // just past the buttons instead of underneath them.
        let list_area = if self.entries.len() as u16 > inner.height {
            Rect {
                width: inner.width + 1,
                ..inner
            }
        } else {
            inner
        };
        self.list_state
            .prepare_layout(&self.entries, list_area.width, list_area.height);
        ListPane::new(&self.entries)
            .focused(focused)
            .style(self.list_style)
            .render(list_area, buf, &mut self.list_state);

        // Hover affordance: paint a dim hover bg on the row under the mouse,
        // matching the scrollback tool-call hover (`blend(bg_base, bg_dark, 0.5)`).
        // Selection wins — skip when the hovered row is the focused selection,
        // which already carries the (stronger) selection bg.
        let hovered_idx = self
            .hovered_row_id
            .and_then(|id| self.entries.iter().position(|e| e.id == id));
        let selected_idx = if focused {
            self.list_state.selected_index()
        } else {
            None
        };
        if let Some(idx) = hovered_idx
            && hovered_idx != selected_idx
        {
            // Skip when the hovered row is scrolled outside the viewport (e.g.
            // a stale hover after a wheel-scroll). A plain saturating_sub would
            // collapse an above-viewport row to row 0 and mis-paint it.
            let item_y = self.list_state.layout().virtual_y(idx);
            if let Some(rel) = item_y.checked_sub(self.list_state.scroll_offset())
                && rel < inner.height as usize
            {
                let screen_y = inner.y + rel as u16;
                let theme = Theme::current();
                let hover_bg = crate::render::color::blend_color(theme.bg_base, theme.bg_dark, 0.5)
                    .unwrap_or(theme.bg_dark);
                let row = Rect::new(inner.x, screen_y, inner.width, 1);
                buf.set_style(row, Style::default().bg(hover_bg));
            }
        }

        // Action buttons (optional [Send now], then [edit], then [cancel],
        // right-aligned). They render for the row under the mouse (hover
        // affordance) or, when the pane is focused, the selected row. Hover
        // takes precedence so mouse users can act on any row without
        // focusing/selecting it first.
        self.send_now.reset();
        self.delete_button.reset();
        self.edit_button.reset();
        let action_idx = self
            .hovered_row_id
            .and_then(|id| self.entries.iter().position(|e| e.id == id))
            .or_else(|| {
                if focused {
                    self.list_state.selected_index()
                } else {
                    None
                }
            });
        if let Some(idx) = action_idx
            && let Some(entry) = self.entries.get(idx)
        {
            use crate::render::SafeBuf;
            let theme = Theme::current();
            // Skip when the action row is scrolled outside the viewport; a plain
            // saturating_sub would otherwise bind the buttons to row 0 and
            // mis-route clicks to an off-screen entry.
            let item_y = self.list_state.layout().virtual_y(idx);
            if let Some(rel) = item_y.checked_sub(self.list_state.scroll_offset())
                && rel < inner.height as usize
            {
                let screen_y = inner.y + rel as u16;
                let btn_style = Style::default().fg(theme.gray);
                // Right-to-left walk. A button renders only when its whole
                // label fits at or right of `inner.x`: saturating toward 0
                // would paint into the left gutter and overlap already-placed
                // buttons (checked_sub underflow = "doesn't fit", not x = 0).
                let mut right = inner.x + inner.width;
                let fits = |right: u16, w: u16| right.checked_sub(w).filter(|&x| x >= inner.x);

                let cancel_label = "[cancel]";
                let cancel_w = cancel_label.len() as u16;
                if let Some(cancel_x) = fits(right, cancel_w) {
                    right = cancel_x;
                    let cancel_style = if self.delete_button.is_hovered_for(entry.id) {
                        Style::default().fg(theme.accent_error)
                    } else {
                        btn_style
                    };
                    buf.set_string_safe(cancel_x, screen_y, cancel_label, cancel_style);
                    self.delete_button
                        .bind(Rect::new(cancel_x, screen_y, cancel_w, 1), entry.id);

                    // [edit] sits flush against [cancel] (no gap) — a gap
                    // would let the queued message behind the row leak through
                    // the seam. Unlike [Send now] it renders regardless of
                    // turn state — the keyboard `e` edit works either way.
                    let edit_label = "[edit]";
                    let edit_w = edit_label.len() as u16;
                    if let Some(edit_x) = fits(right, edit_w) {
                        right = edit_x;
                        let edit_style = if self.edit_button.is_hovered_for(entry.id) {
                            Style::default().fg(theme.text_primary)
                        } else {
                            btn_style
                        };
                        buf.set_string_safe(edit_x, screen_y, edit_label, edit_style);
                        self.edit_button
                            .bind(Rect::new(edit_x, screen_y, edit_w, 1), entry.id);
                    }

                    if is_turn_running {
                        // Compact action wording; same mouse hit-test as before
                        // (force-interject). Leftmost in the chain, flush
                        // against [edit] for the same no-seam reason.
                        let interject_label = "[Send now]";
                        let interject_w = interject_label.len() as u16;
                        if let Some(interject_x) = fits(right, interject_w) {
                            // Brighten the fg on hover (same hover color as the
                            // [Dashboard] button) so it reads as clickable.
                            let interject_style = if self.send_now.is_hovered_for(entry.id) {
                                Style::default().fg(theme.text_primary)
                            } else {
                                btn_style
                            };
                            buf.set_string_safe(
                                interject_x,
                                screen_y,
                                interject_label,
                                interject_style,
                            );
                            self.send_now
                                .bind(Rect::new(interject_x, screen_y, interject_w, 1), entry.id);
                        }
                    }
                }
            }
        }

        // Multiline preview overlay: show when focused and selected entry has 2+ lines.
        if focused
            && let Some(overlay) = overlay_area
            && let Some(text) = self.selected_text()
            && text.lines().count() > 1
        {
            let theme = Theme::current();
            // Use accent_user for text (matches prompt color in queue list).
            let style = PreviewStyle::new(theme.bg_dark, theme.accent_user, theme.gray_dim);
            render_preview_overlay(buf, overlay, text, style, PreviewConfig::default());
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Count the number of decimal digits in a number.
fn digit_count(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        (n as f64).log10().floor() as usize + 1
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_message_is_the_only_server_row_hidden() {
        assert!(!visible_held_server_row("p1", Some("p1")));
        assert!(visible_held_server_row("p2", Some("p1")));
        assert!(visible_held_server_row("p1", None));
    }

    #[test]
    fn queue_row_has_no_goal_lifecycle_gate_or_cue() {
        let styled = QueuedPromptEntry::build_styled(
            "additional context",
            1,
            QueueEntryKind::Prompt,
            Some(80),
        );
        let text: String = styled
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(text, "additional context");
    }
}
