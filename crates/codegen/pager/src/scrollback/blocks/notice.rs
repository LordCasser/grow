//! Immutable UI-only notices.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::fmt;

use crate::appearance::AppearanceConfig;
use crate::render::wrapping::word_wrap_lines;
use crate::scrollback::block::BlockContent;
use crate::scrollback::types::{
    AccentStyle, BlockContext, BlockLine, BlockOutput, DisplayMode, Selectable,
};
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
    /// Causal command identity, not a dedup key: separate durable facts may
    /// share it. Used only to suppress stale progress/RPC fallback feedback.
    pub command_invocation_id: Option<String>,
    pub tone: NoticeTone,
    pub category: NoticeCategory,
    pub text: String,
    pub details: Option<String>,
    /// Optional bounded body shown below the title while collapsed. The full
    /// body belongs in `details` so expansion and copying retain the source.
    communication_preview: Option<String>,
}

impl NoticeBlock {
    pub fn has_details(&self) -> bool {
        matches!(
            self.category,
            NoticeCategory::Coordination | NoticeCategory::Command
        ) && self
            .details
            .as_ref()
            .is_some_and(|details| !details.is_empty())
    }

    pub fn detail_text(&self) -> String {
        format!(
            "{}\n\n{}",
            self.text,
            self.details.as_deref().unwrap_or_default()
        )
    }
    /// Create a compact terminal UI notice for existing command/lifecycle
    /// call sites. Domain events should prefer [`Self::terminal`].
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            event_id: None,
            command_invocation_id: None,
            tone: NoticeTone::Info,
            category: NoticeCategory::Ui,
            text,
            details: None,
            communication_preview: None,
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
            command_invocation_id: None,
            tone,
            category,
            text: text.into(),
            details,
            communication_preview: None,
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
            command_invocation_id: None,
            tone,
            category,
            text: text.into(),
            details,
            communication_preview: None,
        }
    }

    /// Attach a bounded communication body preview without changing the
    /// title or the full detail text used by expanded/copy views.
    pub fn with_communication_preview(mut self, preview: impl Into<String>) -> Self {
        let preview = preview.into();
        self.communication_preview = (!preview.is_empty()).then_some(preview);
        self
    }

    pub fn set_communication_preview(&mut self, preview: Option<String>) {
        self.communication_preview = preview.filter(|preview| !preview.is_empty());
    }
}

impl BlockContent for NoticeBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        let theme = Theme::current();
        let body_style = if self.communication_preview.is_some() {
            theme.primary()
        } else {
            theme.muted()
        };
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
        if let Some(details) = self.details.as_deref()
            && !(self.has_details() && ctx.mode == DisplayMode::Collapsed)
        {
            styled_lines.extend(
                details
                    .lines()
                    .map(|line| Line::from(Span::styled(format!("  {line}"), body_style))),
            );
        }
        let wrapped = word_wrap_lines(styled_lines, ctx.width as usize);
        let mut all_lines: Vec<BlockLine> = wrapped
            .into_iter()
            .map(|line| BlockLine::styled(line).with_selection_range(Some(0)))
            .collect();
        if ctx.mode == DisplayMode::Collapsed
            && let Some(preview) = self.communication_preview.as_deref()
        {
            all_lines.extend(
                crate::scrollback::blocks::tool::OtherToolCallBlock::communication_preview_lines(
                    preview,
                    ctx.content_width(),
                    body_style,
                    body_style,
                ),
            );
        }

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
        self.has_details()
    }

    fn is_selectable(&self) -> bool {
        self.has_details()
    }

    fn default_display_mode(&self) -> DisplayMode {
        // Cause and recovery belong in the visible body, not in metadata.
        if self.has_details() {
            DisplayMode::Collapsed
        } else {
            DisplayMode::Expanded
        }
    }

    fn is_groupable(&self) -> bool {
        self.event_id.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_metadata_is_foldable_without_hiding_actionable_errors() {
        for (tone, mode) in [
            (NoticeTone::Info, DisplayMode::Collapsed),
            (NoticeTone::Success, DisplayMode::Collapsed),
            (NoticeTone::Warning, DisplayMode::Collapsed),
            (NoticeTone::Error, DisplayMode::Collapsed),
        ] {
            let notice = NoticeBlock::terminal(
                "event",
                tone,
                NoticeCategory::Command,
                "Command result",
                Some("Command: /goal clear\nReason and recovery details".into()),
            );
            assert!(notice.is_foldable());
            assert!(notice.is_selectable());
            assert_eq!(notice.default_display_mode(), mode);
            assert!(notice.detail_text().contains("Reason and recovery details"));
        }
    }

    #[test]
    fn coordination_details_are_selectable_and_compact_by_default() {
        let notice = NoticeBlock::terminal(
            "event",
            NoticeTone::Error,
            NoticeCategory::Coordination,
            "Inquiry failed",
            Some("Inquiry ID: id\npermission_denied".into()),
        );
        assert!(notice.is_foldable());
        assert!(notice.is_selectable());
        assert_eq!(notice.default_display_mode(), DisplayMode::Collapsed);
        assert!(notice.detail_text().contains("permission_denied"));
        let mut context = BlockContext {
            width: 100,
            mode: DisplayMode::Collapsed,
            is_running: false,
            raw: false,
            max_lines: None,
            appearance: AppearanceConfig::default(),
            is_selected: false,
            cwd: None,
        };
        let collapsed = notice.output(&context);
        context.mode = DisplayMode::Expanded;
        assert!(notice.output(&context).lines.len() > collapsed.lines.len());
        assert!(!NoticeBlock::new("ordinary lifecycle message").is_foldable());
    }

    #[test]
    fn collapsed_command_error_keeps_cause_and_recovery_visible() {
        let notice = NoticeBlock::terminal(
            "error",
            NoticeTone::Error,
            NoticeCategory::Command,
            "Goal edit rejected: no Goal exists.\nUse /goal set to create one.",
            Some("Command: /goal edit revised\nCatalog metadata and diagnostic detail".into()),
        );
        let output = notice.output(&BlockContext {
            width: 100,
            mode: notice.default_display_mode(),
            is_running: false,
            raw: false,
            max_lines: None,
            appearance: AppearanceConfig::default(),
            is_selected: false,
            cwd: None,
        });
        let text = output
            .lines
            .iter()
            .map(|line| line.content.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("no Goal exists"));
        assert!(text.contains("Use /goal set"));
        assert!(!text.contains("Catalog metadata"));
        assert!(notice.detail_text().contains("Catalog metadata"));
    }

    #[test]
    fn communication_receipt_is_readable_in_light_and_dark_themes() {
        let _guard = crate::theme::cache::pin_theme();
        for kind in [
            crate::theme::ThemeKind::GrowNight,
            crate::theme::ThemeKind::GrowDay,
        ] {
            crate::theme::cache::set(kind);
            let notice = NoticeBlock::terminal(
                "parent-message:theme",
                NoticeTone::Info,
                NoticeCategory::Coordination,
                "Received message from parent agent",
                Some("Message:\n继续检查接收路径 /tmp/image.png".into()),
            )
            .with_communication_preview("继续检查接收路径 /tmp/image.png");
            for width in [24, 40, 80] {
                let output = notice.output(&BlockContext {
                    width,
                    mode: DisplayMode::Collapsed,
                    is_running: false,
                    raw: false,
                    max_lines: None,
                    appearance: AppearanceConfig::default(),
                    is_selected: false,
                    cwd: None,
                });
                assert!(
                    output
                        .lines
                        .iter()
                        .all(|line| line.content.width() <= width as usize)
                );
                let text = output
                    .lines
                    .iter()
                    .map(|line| line.content.to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(text.contains("Received"));
                assert!(text.contains("继续检查"));
            }
        }
        crate::theme::cache::set(crate::theme::ThemeKind::GrowNight);
    }

    #[test]
    fn communication_preview_is_bounded_and_full_body_only_appears_expanded() {
        let notice = NoticeBlock::terminal(
            "parent-message:receipt-1",
            NoticeTone::Info,
            NoticeCategory::Coordination,
            "Parent guidance received",
            Some("Source: parent agent\n\nMessage:\nfirst\nsecond\nthird".into()),
        )
        .with_communication_preview("first\nsecond\nthird");
        let mut context = BlockContext {
            width: 40,
            mode: DisplayMode::Collapsed,
            is_running: false,
            raw: false,
            max_lines: None,
            appearance: AppearanceConfig::default(),
            is_selected: false,
            cwd: None,
        };
        let collapsed = notice.output(&context);
        let collapsed_text = collapsed
            .lines
            .iter()
            .map(|line| line.content.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(collapsed_text.contains("first"));
        assert!(collapsed_text.contains("second"));
        assert!(!collapsed_text.contains("third"));

        context.mode = DisplayMode::Expanded;
        let expanded = notice.output(&context);
        let expanded_text = expanded
            .lines
            .iter()
            .map(|line| line.content.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(expanded_text.contains("Message:"));
        assert!(expanded_text.contains("third"));
        assert_eq!(expanded_text.matches("first").count(), 1);
    }
}
