//! SubagentBlock — scrollback entries for subagent lifecycle.
//!
//! Similar to BgTaskBlock: always collapsed, animated bullet while running,
//! colored bullet when done. Enter / Ctrl-F opens the subagent view.
//!
//! Started and terminal facts are always separate immutable rows. This is
//! required by minimal mode, whose committed native scrollback cannot mutate
//! an earlier row, and also gives retained mode the same replay semantics.

use std::time::Duration;

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::subagent::format_subagent_meta;
use crate::appearance::AppearanceConfig;
use crate::render::color::blend_color;
use crate::render::line_utils::truncate_str;
use crate::scrollback::block::BlockContent;
use crate::scrollback::types::{AccentStyle, BlockContext, BlockOutput, DisplayMode};
use crate::theme::Theme;
use crate::util::format_duration;

/// What kind of subagent lifecycle event this block represents.
#[derive(Debug, Clone)]
pub enum SubagentBlockKind {
    /// Subagent is running (or was running — `finish_running` stops animation).
    Started,
    /// Subagent completed successfully.
    Completed { elapsed: Duration },
    /// Subagent failed.
    Failed {
        elapsed: Duration,
        error: Option<String>,
    },
    /// Subagent was cancelled.
    Cancelled { elapsed: Duration },
}

/// Subagent scrollback block.
///
/// Always collapsed, not foldable, groupable, selectable.
/// Enter / Ctrl-F opens the subagent view.
#[derive(Debug, Clone)]
pub struct SubagentBlock {
    /// Stable durable lifecycle identity used across live delivery, replay,
    /// and Minimal's committed frontier.
    pub event_id: Option<String>,
    /// Human-readable description of the task.
    pub description: String,
    /// Child session ID (for opening the subagent view).
    pub child_session_id: String,
    /// Subagent type (e.g. "general-purpose", "explore").
    pub subagent_type: String,
    /// Effective model ID used by the subagent, if available.
    pub model: Option<String>,
    /// Whether the subagent was launched in background mode.
    pub is_background: bool,
    /// Lifecycle kind.
    pub kind: SubagentBlockKind,
}

impl SubagentBlock {
    /// Create a "Subagent started" block (for both sync and async).
    pub fn started(
        description: impl Into<String>,
        child_session_id: impl Into<String>,
        subagent_type: impl Into<String>,
        model: Option<String>,
        is_background: bool,
    ) -> Self {
        Self {
            event_id: None,
            description: description.into(),
            child_session_id: child_session_id.into(),
            subagent_type: subagent_type.into(),
            model,
            is_background,
            kind: SubagentBlockKind::Started,
        }
    }

    /// Create a "Subagent completed" block (background mode only).
    pub fn completed(
        description: impl Into<String>,
        child_session_id: impl Into<String>,
        elapsed: Duration,
    ) -> Self {
        Self {
            event_id: None,
            description: description.into(),
            child_session_id: child_session_id.into(),
            subagent_type: String::new(),
            model: None,
            is_background: true,
            kind: SubagentBlockKind::Completed { elapsed },
        }
    }

    /// Create a "Subagent failed" block (background mode only).
    pub fn failed(
        description: impl Into<String>,
        child_session_id: impl Into<String>,
        elapsed: Duration,
        error: Option<String>,
    ) -> Self {
        Self {
            event_id: None,
            description: description.into(),
            child_session_id: child_session_id.into(),
            subagent_type: String::new(),
            model: None,
            is_background: true,
            kind: SubagentBlockKind::Failed { elapsed, error },
        }
    }

    /// Create a "Subagent cancelled" block (background mode only).
    pub fn cancelled(
        description: impl Into<String>,
        child_session_id: impl Into<String>,
        elapsed: Duration,
    ) -> Self {
        Self {
            event_id: None,
            description: description.into(),
            child_session_id: child_session_id.into(),
            subagent_type: String::new(),
            model: None,
            is_background: true,
            kind: SubagentBlockKind::Cancelled { elapsed },
        }
    }

    pub fn with_identity(
        mut self,
        subagent_type: impl Into<String>,
        model: Option<String>,
    ) -> Self {
        self.subagent_type = subagent_type.into();
        self.model = model;
        self
    }

    pub fn with_event_id(mut self, event_id: Option<String>) -> Self {
        self.event_id = event_id;
        self
    }

    pub fn is_running(&self) -> bool {
        matches!(self.kind, SubagentBlockKind::Started)
    }
}

/// Truncate description and wrap in quotes for display.
fn quoted_desc(desc: &str, max_width: usize) -> String {
    // Reserve 2 chars for quotes
    if max_width <= 2 {
        return "\u{201C}\u{2026}\u{201D}".to_string(); // "…"
    }
    let inner = truncate_str(desc, max_width - 2);
    format!("\u{201C}{inner}\u{201D}")
}

impl BlockContent for SubagentBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        let theme = Theme::current();
        // When selected, lift only the bold "Subagent" label to
        // `text_primary` so it reads as undimmed (mirrors `read.rs` /
        // `search.rs`, which bump only the label and leave the rest at
        // `muted`). The detail text (verb + description + meta) stays
        // muted in every state.
        let bold = if ctx.is_selected {
            theme.primary().add_modifier(Modifier::BOLD)
        } else {
            theme.muted().add_modifier(Modifier::BOLD)
        };
        let muted = theme.muted();
        let w = ctx.width as usize;

        let line = match (&self.kind, self.is_background) {
            (SubagentBlockKind::Started, _) => {
                let verb = "started: ";
                let meta = format_subagent_meta(self.model.as_deref());
                // "Subagent started: " = 18 chars
                let overhead = 18 + meta.width();
                let desc = quoted_desc(&self.description, w.saturating_sub(overhead));
                let mut spans = vec![
                    Span::styled("Subagent ", bold),
                    Span::styled(verb, muted),
                    Span::styled(desc, muted),
                ];
                spans.push(Span::styled(meta, muted));
                Line::from(spans)
            }
            (SubagentBlockKind::Completed { elapsed }, _) => {
                let time_str = format_duration(*elapsed);
                let identity = if self.subagent_type.is_empty() {
                    "subagent"
                } else {
                    self.subagent_type.as_str()
                };
                let prefix = format!("{identity} completed · result delivered · {time_str} · ");
                let prefix_len = 10 + prefix.width();
                let desc = quoted_desc(&self.description, w.saturating_sub(prefix_len));
                Line::from(vec![
                    Span::styled("SUBAGENT  ", bold),
                    Span::styled(prefix, muted),
                    Span::styled(desc, muted),
                ])
            }
            (SubagentBlockKind::Failed { elapsed, error }, _) => {
                let time_str = format_duration(*elapsed);
                let identity = if self.subagent_type.is_empty() {
                    "subagent"
                } else {
                    self.subagent_type.as_str()
                };
                let delivery = if error.is_some() {
                    "error delivered"
                } else {
                    "no error detail"
                };
                let prefix = format!("{identity} failed · {delivery} · {time_str} · ");
                let prefix_len = 10 + prefix.width();
                let desc = quoted_desc(&self.description, w.saturating_sub(prefix_len));
                Line::from(vec![
                    Span::styled("SUBAGENT  ", bold),
                    Span::styled(prefix, muted),
                    Span::styled(desc, muted),
                ])
            }
            (SubagentBlockKind::Cancelled { elapsed }, _) => {
                let time_str = format_duration(*elapsed);
                let identity = if self.subagent_type.is_empty() {
                    "subagent"
                } else {
                    self.subagent_type.as_str()
                };
                let prefix = format!("{identity} cancelled · no result delivered · {time_str} · ");
                let prefix_len = 10 + prefix.width();
                let desc = quoted_desc(&self.description, w.saturating_sub(prefix_len));
                Line::from(vec![
                    Span::styled("SUBAGENT  ", bold),
                    Span::styled(prefix, muted),
                    Span::styled(desc, muted),
                ])
            }
        };

        BlockOutput {
            lines: vec![line.into()],
        }
    }

    fn accent(&self, ctx: &BlockContext) -> Option<AccentStyle> {
        let theme = Theme::current();
        match &self.kind {
            SubagentBlockKind::Started if ctx.is_running => {
                Some(AccentStyle::static_color(theme.accent_running))
            }
            _ => None,
        }
    }

    fn bullet(&self, ctx: &BlockContext) -> Option<AccentStyle> {
        let theme = Theme::current();
        match &self.kind {
            SubagentBlockKind::Started => {
                if ctx.is_running {
                    let dim = ctx.appearance.scrollback.display.dim_accent;
                    let dimmed = blend_color(theme.bg_base, theme.accent_running, dim)
                        .unwrap_or(theme.accent_running);
                    Some(AccentStyle::animated(dimmed))
                } else {
                    // Finished — gray bullet (same as bg task "started" after completion)
                    None
                }
            }
            SubagentBlockKind::Completed { .. } => {
                Some(AccentStyle::static_color(theme.accent_success))
            }
            SubagentBlockKind::Failed { .. } | SubagentBlockKind::Cancelled { .. } => {
                Some(AccentStyle::static_color(theme.accent_error))
            }
        }
    }

    fn has_vpad_for(&self, _appearance: &AppearanceConfig) -> bool {
        false
    }

    fn has_raw_mode(&self) -> bool {
        false
    }

    fn is_foldable(&self) -> bool {
        false
    }

    fn default_display_mode(&self) -> DisplayMode {
        DisplayMode::Collapsed
    }

    fn is_selectable(&self) -> bool {
        true
    }

    fn has_bullet(&self, _ctx: &BlockContext) -> bool {
        true
    }

    fn is_groupable(&self) -> bool {
        true
    }
}
