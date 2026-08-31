//! Immutable UI-only notices.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::fmt;

use crate::appearance::AppearanceConfig;
use crate::render::wrapping::word_wrap_lines;
use crate::scrollback::block::BlockContent;
use crate::scrollback::types::{AccentStyle, BlockContext, BlockLine, BlockOutput, Selectable};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeTone {
    Progress,
    Info,
    Success,
    Warning,
    Error,
}

impl NoticeTone {
    pub(crate) fn color(self, theme: &Theme) -> ratatui::style::Color {
        match self {
            Self::Progress => theme.accent_running,
            Self::Info => theme.accent_system,
            Self::Success => theme.accent_success,
            Self::Warning => theme.warning,
            Self::Error => theme.accent_error,
        }
    }
}

/// Shared transient presentation payload for retained UI surfaces. Persistence
/// and model-context projection are deliberately owned elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiFeedback {
    pub tone: NoticeTone,
    pub message: String,
}

impl UiFeedback {
    pub fn new(tone: NoticeTone, message: impl Into<String>) -> Self {
        Self {
            tone,
            message: message.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        self.message.as_str()
    }
}

impl std::ops::Deref for UiFeedback {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for UiFeedback {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeCategory {
    Command,
    Coordination,
    Lifecycle,
    Control,
    Subagent,
    Ui,
}

impl NoticeCategory {
    fn label(self) -> &'static str {
        match self {
            Self::Command => "COMMAND",
            Self::Coordination => "COORDINATION",
            Self::Lifecycle => "LIFECYCLE",
            Self::Control => "CONTROL",
            Self::Subagent => "SUBAGENT",
            Self::Ui => "NOTICE",
        }
    }
}

/// An immutable presentation event. Notice blocks never participate in model
/// context projection; transient progress belongs in the live status layer.
#[derive(Debug, Clone)]
pub struct NoticeBlock {
    /// Stable identity for a durable domain event. Ad-hoc local notices have
    /// no identity because identical text may legitimately occur twice.
    pub event_id: Option<String>,
    pub tone: NoticeTone,
    pub category: NoticeCategory,
    pub text: String,
    pub details: Option<String>,
}

impl NoticeBlock {
    /// Create a compact terminal UI notice for existing command/lifecycle
    /// call sites. Domain events should prefer [`Self::terminal`].
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            event_id: None,
            tone: NoticeTone::Info,
            category: NoticeCategory::Ui,
            text,
            details: None,
        }
    }

    /// Create a typed local UI event. Local command results do not have a
    /// durable domain-event identity, but still need the same tone/category
    /// semantics as replayable terminal notices.
    pub fn typed(
        tone: NoticeTone,
        category: NoticeCategory,
        text: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self {
            event_id: None,
            tone,
            category,
            text: text.into(),
            details,
        }
    }

    pub fn terminal(
        event_id: impl Into<String>,
        tone: NoticeTone,
        category: NoticeCategory,
        text: impl Into<String>,
        details: Option<String>,
    ) -> Self {
        Self {
            event_id: Some(event_id.into()),
            tone,
            category,
            text: text.into(),
            details,
        }
    }
}

impl BlockContent for NoticeBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        let theme = Theme::current();
        let body_style = theme.muted();
        let label_style = Style::default()
            .fg(self.tone.color(&theme))
            .add_modifier(Modifier::BOLD);
        let mut source_lines = self.text.lines();
        let first = source_lines.next().unwrap_or_default();
        let mut styled_lines = vec![Line::from(vec![
            Span::styled(format!("{}  ", self.category.label()), label_style),
            Span::styled(first.to_string(), body_style),
        ])];
        styled_lines.extend(
            source_lines.map(|line| Line::from(Span::styled(line.to_string(), body_style))),
        );
        if let Some(details) = self.details.as_deref() {
            styled_lines.extend(
                details
                    .lines()
                    .map(|line| Line::from(Span::styled(format!("  {line}"), body_style))),
            );
        }
        let wrapped = word_wrap_lines(styled_lines, ctx.width as usize);
        let all_lines: Vec<BlockLine> = wrapped
            .into_iter()
            .map(|line| BlockLine::styled(line).with_selection_range(Some(0)))
            .collect();

        // Apply max_lines budget if set
        let lines = if let Some(max) = ctx.max_lines {
            let max = max as usize;
            if all_lines.len() > max && max > 0 {
                let take_count = if max > 1 { max - 1 } else { 1 };
                let mut truncated: Vec<BlockLine> =
                    all_lines.into_iter().take(take_count).collect();
                if let Some(last) = truncated.last_mut() {
                    let content_end = last.content.spans.len();
                    last.content
                        .spans
                        .push(Span::styled(" \u{2026}".to_string(), body_style));
                    last.selectable = Selectable::Spans(0..content_end);
                }
                truncated
            } else {
                all_lines
            }
        } else {
            all_lines
        };

        if lines.is_empty() {
            BlockOutput {
                lines: vec![BlockLine::styled(Line::from("")).with_selection_range(Some(0))],
            }
        } else {
            BlockOutput { lines }
        }
    }

    fn accent(&self, _ctx: &BlockContext) -> Option<AccentStyle> {
        None
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

    fn is_selectable(&self) -> bool {
        false
    }

    fn is_groupable(&self) -> bool {
        self.event_id.is_none()
    }
}
