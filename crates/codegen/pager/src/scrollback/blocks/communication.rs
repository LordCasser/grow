//! Structured communication body projection for Pager blocks.
//!
//! Communication rows keep protocol metadata on the block, but render the
//! user-facing question, answer, message, and error as independent Markdown
//! sections.  Each section also retains its typed source so raw mode and copy
//! do not depend on the tab-expanded Markdown renderer source.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::render::line_utils::truncate_line;
use crate::scrollback::blocks::markdown_content::MarkdownContent;
use crate::scrollback::types::{BlockLine, BlockOutput};
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommunicationSectionKind {
    Question,
    Answer,
    Message,
    Error,
}

impl CommunicationSectionKind {
    fn short_label(self) -> &'static str {
        match self {
            Self::Question => "Q",
            Self::Answer => "A",
            Self::Message => "M",
            Self::Error => "Error",
        }
    }

    fn full_label(self) -> &'static str {
        match self {
            Self::Question => "Question",
            Self::Answer => "Answer",
            Self::Message => "Message",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone)]
struct CommunicationSection {
    kind: CommunicationSectionKind,
    raw: String,
    markdown: MarkdownContent,
}

/// Markdown-backed body sections shared by outgoing communication rows and
/// incoming coordination notices.
#[derive(Debug, Clone, Default)]
pub(crate) struct CommunicationBody {
    sections: Vec<CommunicationSection>,
    raw_mode: bool,
}

impl CommunicationBody {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn push(&mut self, kind: CommunicationSectionKind, text: impl Into<String>) {
        let raw = text.into();
        if raw.is_empty() {
            return;
        }
        self.sections.push(CommunicationSection {
            kind,
            markdown: MarkdownContent::new(raw.clone()),
            raw,
        });
    }

    #[cfg(test)]
    pub(crate) fn with_section(
        mut self,
        kind: CommunicationSectionKind,
        text: impl Into<String>,
    ) -> Self {
        self.push(kind, text);
        self
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    pub(crate) fn set_raw_mode(&mut self, raw: bool) {
        self.raw_mode = raw;
        for section in &mut self.sections {
            section.markdown.set_raw_mode(raw);
        }
    }

    pub(crate) fn raw_text(&self) -> String {
        self.sections
            .iter()
            .map(|section| section.raw.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(crate) fn rendered_text(&self) -> String {
        self.sections
            .iter()
            .map(|section| section.markdown.rendered_plain_text())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub(crate) fn fingerprint(&self) -> u64 {
        let mut hash = 0xcbf29ce484222325u64;
        for section in &self.sections {
            hash ^= section.kind as u64;
            hash = hash.wrapping_mul(0x100000001b3);
            for byte in section.raw.as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
        }
        hash
    }

    /// Render the compact body budget used by scrollback rows.
    pub(crate) fn preview(&self, width: usize) -> BlockOutput {
        let mut lines = Vec::new();
        let has_answer = self
            .sections
            .iter()
            .any(|section| section.kind == CommunicationSectionKind::Answer);
        let has_error = self
            .sections
            .iter()
            .any(|section| section.kind == CommunicationSectionKind::Error);
        for section in &self.sections {
            let budget = match section.kind {
                CommunicationSectionKind::Question if has_error => 1,
                CommunicationSectionKind::Question if has_answer => 1,
                CommunicationSectionKind::Question => 2,
                CommunicationSectionKind::Answer => 2,
                CommunicationSectionKind::Message => 2,
                CommunicationSectionKind::Error => 2,
            };
            lines.extend(self.render_section(section, width, budget, false));
        }
        BlockOutput { lines }
    }

    /// Render all sections for inline expansion and the fullscreen viewer.
    pub(crate) fn expanded(&self, width: usize) -> BlockOutput {
        if self.raw_mode {
            return BlockOutput {
                lines: self
                    .raw_viewer_lines()
                    .into_iter()
                    .map(BlockLine::styled)
                    .collect(),
            };
        }
        let mut lines = Vec::new();
        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                lines.push(Line::from("").into());
            }
            lines.extend(self.render_section(section, width, usize::MAX, true));
        }
        BlockOutput { lines }
    }

    pub(crate) fn viewer_lines(&self, width: usize) -> Vec<Line<'static>> {
        self.expanded(width)
            .lines
            .into_iter()
            .map(|line| line.content)
            .collect()
    }

    pub(crate) fn raw_viewer_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                format!("  {}", section.kind.full_label()),
                Style::default()
                    .fg(Theme::current().gray)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.extend(
                section
                    .raw
                    .split('\n')
                    .map(|line| Line::raw(format!("  {line}"))),
            );
        }
        lines
    }

    fn render_section(
        &self,
        section: &CommunicationSection,
        width: usize,
        budget: usize,
        full: bool,
    ) -> Vec<BlockLine> {
        let theme = Theme::current();
        let label = if section.kind == CommunicationSectionKind::Message {
            "  ".to_owned()
        } else if full {
            format!("  {}: ", section.kind.full_label())
        } else {
            format!("  {}: ", section.kind.short_label())
        };
        let continuation = " ".repeat(label.chars().count());
        let body_width =
            width.saturating_sub(unicode_width::UnicodeWidthStr::width(label.as_str()));
        let mut rendered = section.markdown.output(body_width.max(1)).lines;
        if rendered.is_empty() {
            rendered.push(Line::from("").into());
        }
        if budget != usize::MAX && rendered.len() > budget {
            rendered.truncate(budget);
            if let Some(last) = rendered.last_mut() {
                last.content.spans.push(Span::styled("…", theme.muted()));
                last.content = truncate_line(last.content.clone(), body_width.max(1));
            }
        }
        rendered
            .into_iter()
            .enumerate()
            .map(|(index, mut line)| {
                let prefix = if index == 0 {
                    label.as_str()
                } else {
                    continuation.as_str()
                };
                let mut spans = vec![Span::styled(
                    prefix.to_owned(),
                    if section.kind == CommunicationSectionKind::Error {
                        Style::default().fg(theme.accent_error)
                    } else {
                        theme.muted()
                    },
                )];
                spans.append(&mut line.content.spans);
                line.content = truncate_line(Line::from(spans), width.max(1));
                line
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn communication_preview_uses_answer_budget() {
        let body = CommunicationBody::new()
            .with_section(CommunicationSectionKind::Question, "question")
            .with_section(CommunicationSectionKind::Answer, "answer");
        let rendered = body
            .preview(80)
            .lines
            .iter()
            .map(|line| line.content.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("Q: question"), "{rendered}");
        assert!(rendered.contains("A: answer"), "{rendered}");
    }

    #[test]
    fn communication_preview_holds_width_budget_across_grow_themes() {
        let _guard = crate::theme::cache::pin_theme();
        let body = CommunicationBody::new()
            .with_section(
                CommunicationSectionKind::Question,
                "中文问题 👩‍💻 with **bold** and `code`\n\n```rust\n\tlet value = 1;\n```",
            )
            .with_section(
                CommunicationSectionKind::Answer,
                "A terminal answer\n\n| Field | Value |\n| --- | --- |\n| 状态 | 收到 ✅ |\n\n> quoted\n\n- item\n\n```mermaid\ngraph LR\nA --> B\n```",
            );
        for theme in [
            crate::theme::ThemeKind::GrowNight,
            crate::theme::ThemeKind::GrowDay,
        ] {
            crate::theme::cache::set(theme);
            for width in [40, 60, 100] {
                assert!(
                    body.preview(width)
                        .lines
                        .iter()
                        .all(|line| line.content.width() <= width)
                );
            }
        }
        crate::theme::cache::set(crate::theme::ThemeKind::GrowNight);
    }

    #[test]
    fn same_length_answer_changes_are_visible_to_viewer_generation() {
        let first = CommunicationBody::new()
            .with_section(CommunicationSectionKind::Question, "same")
            .with_section(CommunicationSectionKind::Answer, "AAAA");
        let second = CommunicationBody::new()
            .with_section(CommunicationSectionKind::Question, "same")
            .with_section(CommunicationSectionKind::Answer, "BBBB");
        assert_ne!(first.fingerprint(), second.fingerprint());
    }

    #[test]
    fn raw_source_preserves_tabs_and_newlines() {
        let mut body = CommunicationBody::new();
        body.push(CommunicationSectionKind::Message, "first\tcolumn\r\nsecond");
        assert_eq!(body.raw_text(), "first\tcolumn\r\nsecond");
        body.set_raw_mode(true);
        let long = body.viewer_lines(4);
        assert!(long.iter().any(|line| line.width() > 4));
        let rendered: String = body
            .viewer_lines(80)
            .into_iter()
            .flat_map(|line| line.spans)
            .map(|span| span.content.into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("first\tcolumn\r"), "{rendered:?}");
    }
}
