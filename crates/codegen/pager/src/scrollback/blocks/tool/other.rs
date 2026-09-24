//! OtherToolCallBlock - unknown/generic tool types.

use ratatui::text::{Line, Span};

use crate::appearance::AppearanceConfig;
use crate::render::wrapping::word_wrap_lines;
use crate::scrollback::block::BlockContent;
use crate::scrollback::blocks::communication::CommunicationBody;
use crate::scrollback::types::{
    AccentStyle, BlockBackground, BlockContext, BlockLine, BlockOutput, DisplayMode,
};
use crate::theme::Theme;

/// Identity of a passive inquiry row rendered with the normal tool chrome.
/// Its durable start/approval/end events are not three separate UI rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoordinationPhase {
    Received,
    Approved,
    Terminal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinationRow {
    pub source_peer_id: String,
    pub inquiry_id: String,
    pub phase: CoordinationPhase,
}

/// Other/unknown tool call.
#[derive(Debug, Clone)]
pub struct OtherToolCallBlock {
    /// Tool name.
    pub name: String,
    /// Summary/target.
    pub summary: String,
    /// Error message if the tool call failed (None = success).
    pub error: Option<String>,
    /// Optional output.
    pub output: Option<String>,
    /// When the tool started running (Phase 2: time tracking).
    pub started_at: Option<std::time::Instant>,
    /// Elapsed time in ms after completion (Phase 2: time tracking).
    pub elapsed_ms: Option<i64>,
    /// Present only on the receiving side's passive inquiry presentation.
    pub coordination: Option<CoordinationRow>,
    /// Bounded, readable input preview for an outgoing communication tool.
    /// The complete message/question remains in `output` for expansion and
    /// copy, while this field keeps the collapsed row useful at a glance.
    communication_preview: Option<String>,
    /// Markdown-backed communication sections. The typed source is retained
    /// separately from the renderer's expanded-tab source for raw/copy views.
    communication_body: Option<CommunicationBody>,
    /// Complete protocol result/input data shown only in the detail viewer.
    communication_data: Option<String>,
    /// Image references detected in the tool output.
    image_refs: Vec<crate::prompt_images::ScrollbackImageRef>,
}

impl OtherToolCallBlock {
    /// Wrap a communication body to at most two display lines, preserving
    /// Unicode cell widths and adding an ellipsis when more content follows.
    /// Notice blocks call this associated helper through the re-exported
    /// `OtherToolCallBlock`, so both surfaces share the same projector.
    pub(crate) fn communication_preview_lines(
        preview: &str,
        width: usize,
        text_style: ratatui::style::Style,
        ellipsis_style: ratatui::style::Style,
    ) -> Vec<BlockLine> {
        let wrap_width = width.saturating_sub(2).max(1);
        let mut wrapped = word_wrap_lines(
            preview
                .lines()
                .map(|line| Line::from(Span::styled(line.to_owned(), text_style))),
            wrap_width,
        );
        let truncated = wrapped.len() > 2;
        wrapped.truncate(2);
        if truncated && let Some(last) = wrapped.last_mut() {
            last.spans.push(Span::styled("…", ellipsis_style));
            *last = crate::render::line_utils::truncate_line(last.clone(), wrap_width);
        }
        wrapped
            .into_iter()
            .map(|line| {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(line.spans);
                BlockLine::styled(crate::render::line_utils::truncate_line(
                    Line::from(spans),
                    width,
                ))
            })
            .collect()
    }

    /// Create a new other tool block.
    ///
    /// Pre-completed blocks have no meaningful local timing — `started_at`
    /// is `None`. Timing is only set for blocks that enter a running UI
    /// state (via `set_last_running(true)` in `ScrollbackState`).
    pub fn new(name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            summary: summary.into(),
            error: None,
            output: None,
            started_at: None,
            elapsed_ms: None,
            coordination: None,
            communication_preview: None,
            communication_body: None,
            communication_data: None,
            image_refs: Vec::new(),
        }
    }

    /// Set error (marks as failed).
    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self
    }

    /// Set output (builder).
    pub fn with_output(mut self, output: impl Into<String>) -> Self {
        self.set_output_text(output.into());
        self
    }

    /// Attach the original message/question used by a communication tool.
    /// It is intentionally separate from `summary`: the latter belongs in
    /// the one-line tool header, while this value is rendered as two bounded
    /// lines below it and the full text stays available in `output`.
    pub(crate) fn with_communication_preview(mut self, preview: impl Into<String>) -> Self {
        let preview = preview.into();
        if !preview.is_empty() {
            self.communication_preview = Some(preview);
        }
        self
    }

    pub(crate) fn with_communication_body(mut self, body: CommunicationBody) -> Self {
        if !body.is_empty() {
            self.communication_body = Some(body);
        }
        self
    }

    pub(crate) fn with_communication_data(mut self, data: impl Into<String>) -> Self {
        let data = data.into();
        if !data.is_empty() {
            self.communication_data = Some(data);
        }
        self
    }

    pub(crate) fn communication_body(&self) -> Option<&CommunicationBody> {
        self.communication_body.as_ref()
    }

    pub(crate) fn communication_data(&self) -> Option<&str> {
        self.communication_data.as_deref()
    }

    pub(crate) fn set_raw_mode(&mut self, raw: bool) {
        if let Some(body) = &mut self.communication_body {
            body.set_raw_mode(raw);
        }
    }

    /// Set or replace the output text.
    pub fn set_output_text(&mut self, text: String) {
        self.image_refs = crate::prompt_images::extract_image_refs(&text);
        self.output = Some(text);
    }

    /// Check if successful (no error).
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }

    /// Path of the first image reference for the filepath
    /// line of an inline-media block, independent of inline-graphics support.
    pub(crate) fn media_ref_path(&self) -> Option<std::path::PathBuf> {
        if self.prefers_text_output() {
            return None;
        }
        self.image_refs.first().map(|image| image.path.clone())
    }

    /// Coordination carries structured text, sometimes quoting an image path.
    /// Such a reference is an attachment, not a replacement for the result.
    fn prefers_text_output(&self) -> bool {
        self.is_communication()
            || self.coordination.is_some()
            || matches!(
                self.name.as_str(),
                "list_active_sessions" | "ask_session" | "get_inquiry"
            )
    }

    fn is_communication(&self) -> bool {
        self.communication_body.is_some()
            || self.communication_preview.is_some()
            || self.name.starts_with("ask_session → ")
            || self.name.starts_with("get_inquiry → ")
            || self.name.starts_with("ask_parent → ")
            || self.name.starts_with("ask_subagent → ")
            || self.name.starts_with("send_subagent_message → ")
    }

    /// Set error (mutable) — compute elapsed time if not already set (Phase 2).
    pub fn set_error(&mut self, error: Option<String>) {
        if self.elapsed_ms.is_none()
            && let Some(start) = self.started_at
        {
            self.elapsed_ms = Some(start.elapsed().as_millis() as i64);
        }
        self.error = error;
    }

    /// Finalize elapsed time from `started_at`.
    ///
    /// Idempotent: no-op if `started_at` is `None` (pre-completed block)
    /// or if `elapsed_ms` is already set (already finalized).
    pub fn finish(&mut self) {
        if self.elapsed_ms.is_some() {
            return;
        }
        if let Some(start) = self.started_at {
            self.elapsed_ms = Some(start.elapsed().as_millis() as i64);
        }
    }

    /// Get elapsed time in ms (Phase 2).
    pub fn elapsed_ms(&self) -> Option<i64> {
        match self.elapsed_ms {
            Some(ms) => Some(ms),
            None => self
                .started_at
                .map(|start| start.elapsed().as_millis() as i64),
        }
    }

    /// Build the styled title: **`Label`** `content` or **`Name`**.
    ///
    /// If the name contains `: `, splits into bold label + muted/primary content
    /// (e.g. "Ask: What is your favorite language?"). Otherwise renders
    /// the full name in bold.
    ///
    /// When `muted` is true (collapsed state), all text uses dim styles to
    /// match other collapsed blocks. The label ("Ask") stays bold.
    fn title_spans(&self, theme: &Theme, muted: bool) -> Vec<Span<'static>> {
        let text_style = if muted {
            theme.muted()
        } else {
            theme.primary()
        };
        let bold_style = text_style.add_modifier(ratatui::style::Modifier::BOLD);

        if let Some((label, content)) = self.name.split_once(": ") {
            vec![
                Span::styled(format!("{} ", label), bold_style),
                Span::styled(content.to_string(), text_style),
            ]
        } else {
            vec![Span::styled(self.name.clone(), bold_style)]
        }
    }

    fn collapsed_line(&self, theme: &Theme, muted: bool, width: Option<usize>) -> Line<'static> {
        let mut spans = self.title_spans(theme, muted);

        if !self.summary.is_empty() {
            if let Some(w) = width {
                // Only include summary if there's room.
                let used: usize = spans
                    .iter()
                    .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                    .sum();
                let summary = format!("  {}", self.summary);
                if used + unicode_width::UnicodeWidthStr::width(summary.as_str()) <= w {
                    spans.push(Span::styled(summary, theme.muted()));
                }
            } else {
                spans.push(Span::styled(format!("  {}", self.summary), theme.muted()));
            }
        }

        let line = Line::from(spans);
        if let Some(w) = width {
            crate::render::line_utils::truncate_line(line, w)
        } else {
            line
        }
    }

    /// Render a communication title and status as separate wrapped rows when
    /// they cannot fit together. The title and status each get at most two
    /// rows; narrow rows retain an explicit ellipsis rather than hiding one
    /// of the two pieces behind the other.
    fn communication_header_lines(
        &self,
        theme: &Theme,
        muted: bool,
        width: usize,
    ) -> Vec<BlockLine> {
        let width = width.max(1);
        let title = Line::from(self.title_spans(theme, muted));
        let summary = (!self.summary.is_empty())
            .then(|| Line::from(Span::styled(format!("  {}", self.summary), theme.muted())));
        let total_width = title
            .spans
            .iter()
            .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
            .sum::<usize>()
            + summary.as_ref().map_or(0, |summary| {
                summary
                    .spans
                    .iter()
                    .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
                    .sum::<usize>()
            });
        if total_width <= width {
            let mut spans = title.spans;
            if let Some(summary) = summary {
                spans.extend(summary.spans);
            }
            return vec![BlockLine::styled(crate::render::line_utils::truncate_line(
                Line::from(spans),
                width,
            ))];
        }

        let mut title_lines = word_wrap_lines(std::iter::once(title), width);
        let title_truncated = title_lines.len() > 2;
        title_lines.truncate(2);
        if title_truncated && let Some(last) = title_lines.last_mut() {
            last.spans.push(Span::styled("…", theme.muted()));
            *last = crate::render::line_utils::truncate_line(last.clone(), width);
        }

        if let Some(summary) = summary {
            let mut status_lines = word_wrap_lines(std::iter::once(summary), width);
            let status_truncated = status_lines.len() > 2;
            status_lines.truncate(2);
            if status_truncated && let Some(last) = status_lines.last_mut() {
                last.spans.push(Span::styled("…", theme.muted()));
                *last = crate::render::line_utils::truncate_line(last.clone(), width);
            }
            title_lines
                .into_iter()
                .chain(status_lines)
                .map(|line| {
                    BlockLine::styled(crate::render::line_utils::truncate_line(line, width))
                })
                .collect()
        } else {
            title_lines
                .into_iter()
                .map(|line| {
                    BlockLine::styled(crate::render::line_utils::truncate_line(line, width))
                })
                .collect()
        }
    }
}

impl BlockContent for OtherToolCallBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        let theme = Theme::current();
        let width = ctx.width as usize;
        let muted_collapsed =
            ctx.mute_when_collapsed(ctx.appearance.scrollback.blocks.tool.muted_collapsed);

        // Inline image blocks render the header and a filepath line on every terminal.
        if let Some(media_path) = self.media_ref_path() {
            let header = self.collapsed_line(&theme, muted_collapsed, Some(ctx.content_width()));
            let max_w = ctx.content_width();
            // Percent-decode for display only (e.g. `%2F` → `/`); the stored
            // path is unchanged so Open / copy-path still target the file.
            let raw_path = media_path.display().to_string();
            let path_str = urlencoding::decode(&raw_path)
                .map(|s| s.into_owned())
                .unwrap_or(raw_path);
            // Char-boundary middle-ellipsis (decoded paths may be multibyte).
            let path_display = if path_str.chars().count() > max_w {
                let keep = max_w.saturating_sub(3) / 2;
                let end_keep = max_w.saturating_sub(3) - keep;
                let chars: Vec<char> = path_str.chars().collect();
                let head: String = chars[..keep].iter().collect();
                let tail: String = chars[chars.len() - end_keep..].iter().collect();
                format!("{head}...{tail}")
            } else {
                path_str
            };
            let path_line = Line::from(Span::styled(
                path_display,
                ratatui::style::Style::default().fg(theme.gray_dim),
            ));
            let mut lines: Vec<BlockLine> = vec![header.into(), path_line.into()];

            // No inline graphics: centered "[Open]" button between blank
            // spacers (its click target is registered in render.rs).
            if self.inline_open_button().is_some() {
                let label = crate::scrollback::render::media_open_button_label();
                let col =
                    crate::scrollback::render::media_open_button_col(ctx.content_width() as u16);
                let open_line = Line::from(vec![
                    Span::raw(" ".repeat(col as usize)),
                    Span::styled(
                        label.to_string(),
                        ratatui::style::Style::default()
                            .fg(theme.md_code)
                            .add_modifier(ratatui::style::Modifier::BOLD),
                    ),
                ]);
                lines.push(Line::from("").into());
                lines.push(open_line.into());
                lines.push(Line::from("").into());
            }

            return BlockOutput { lines };
        }

        match ctx.mode {
            DisplayMode::Collapsed => {
                let content_width = ctx.content_width();
                let mut lines = if self.is_communication() {
                    self.communication_header_lines(&theme, muted_collapsed, content_width)
                } else {
                    vec![
                        self.collapsed_line(&theme, muted_collapsed, Some(content_width))
                            .into(),
                    ]
                };
                if let Some(preview) = &self.communication_preview {
                    if self.communication_body.is_none() {
                        lines.extend(Self::communication_preview_lines(
                            preview,
                            content_width,
                            theme.primary(),
                            theme.muted(),
                        ));
                    }
                }
                if let Some(body) = &self.communication_body {
                    lines.extend(body.preview(content_width).lines);
                }
                BlockOutput { lines }
            }
            DisplayMode::Truncated | DisplayMode::Expanded => {
                let mut lines: Vec<BlockLine> =
                    vec![self.collapsed_line(&theme, false, None).into()];

                if let Some(body) = &self.communication_body {
                    lines.extend(body.expanded(width).lines);
                    return BlockOutput { lines };
                }

                if let Some(error) = &self.error {
                    lines.extend(
                        word_wrap_lines(
                            error.lines().map(|line| Line::from(line.to_owned())),
                            width,
                        )
                        .into_iter()
                        .map(BlockLine::styled),
                    );
                }

                if let Some(output) = &self.output {
                    // Try to render as structured Q&A (AskUserQuestion output).
                    let qa_lines = parse_ask_user_qa_pairs(output);
                    if !qa_lines.is_empty() {
                        for (i, (question, answer)) in qa_lines.iter().enumerate() {
                            // "  1. question text"
                            let q_line = Line::from(vec![
                                Span::styled(format!("  {}. ", i + 1), theme.muted()),
                                Span::styled(question.clone(), theme.primary()),
                            ]);
                            lines.push(BlockLine::styled(q_line));

                            // "     → answer" or "     (no answer)"
                            let a_line = if answer.is_empty() {
                                Line::from(Span::styled(
                                    "     (no answer)".to_string(),
                                    theme.dim(),
                                ))
                            } else {
                                Line::from(vec![
                                    Span::styled(
                                        "     \u{2192} ".to_string(),
                                        theme.fg(theme.accent_user),
                                    ),
                                    Span::styled(answer.clone(), theme.fg(theme.accent_user)),
                                ])
                            };
                            lines.push(BlockLine::styled(a_line));
                        }
                    } else {
                        // Generic output rendering (non-Q&A tools).
                        lines.push(Line::from("").into());

                        let styled_lines: Vec<Line<'static>> = output
                            .lines()
                            .map(|line| Line::from(Span::styled(line.to_string(), theme.muted())))
                            .collect();

                        let wrapped =
                            word_wrap_lines(styled_lines, width.saturating_sub(2).max(20));

                        for wrapped_line in wrapped {
                            lines.push(BlockLine::styled(wrapped_line));
                        }
                    }
                }

                BlockOutput { lines }
            }
        }
    }

    fn accent(&self, ctx: &BlockContext) -> Option<AccentStyle> {
        // Passive inquiry rows use Run-like state chrome even when collapsed.
        // Ordinary Other tools keep their existing dense-group appearance.
        if ctx.mode == DisplayMode::Collapsed && self.coordination.is_none() {
            return None;
        }
        let theme = Theme::current();
        if self.error.is_some() {
            Some(AccentStyle::static_color(theme.accent_error))
        } else if ctx.is_running {
            Some(AccentStyle::animated(theme.accent_running))
        } else if self
            .coordination
            .as_ref()
            .is_some_and(|row| row.phase == CoordinationPhase::Terminal)
        {
            Some(AccentStyle::static_color(theme.accent_success))
        } else {
            Some(AccentStyle::static_color(theme.accent_tool))
        }
    }

    fn bullet(&self, ctx: &BlockContext) -> Option<AccentStyle> {
        // Failed: red bullet. Running/expanded: accent color. Collapsed: default.
        if self.error.is_some() {
            let theme = Theme::current();
            Some(AccentStyle::static_color(theme.accent_error))
        } else if self.coordination.is_some() {
            self.accent(ctx)
        } else if ctx.mode == DisplayMode::Collapsed {
            None // default gray
        } else {
            self.accent(ctx) // inherit from accent when expanded/running
        }
    }

    fn has_vpad_for(&self, _appearance: &AppearanceConfig) -> bool {
        false
    }

    fn background(&self, _ctx: &BlockContext) -> BlockBackground {
        BlockBackground::None
    }

    fn has_raw_mode(&self) -> bool {
        self.communication_body.is_some()
    }

    fn is_foldable(&self) -> bool {
        self.communication_body.is_some() || self.output.is_some() || self.error.is_some()
    }

    fn default_display_mode(&self) -> DisplayMode {
        DisplayMode::Collapsed
    }

    fn next_fold_mode(&self, current: DisplayMode, is_running: bool) -> DisplayMode {
        if is_running && self.coordination.is_none() && !self.is_communication() {
            match current {
                DisplayMode::Truncated => DisplayMode::Expanded,
                _ => DisplayMode::Truncated,
            }
        } else {
            match current {
                DisplayMode::Collapsed => DisplayMode::Expanded,
                _ => DisplayMode::Collapsed,
            }
        }
    }

    fn collapse_mode(&self, is_running: bool) -> DisplayMode {
        if is_running && self.coordination.is_none() && !self.is_communication() {
            DisplayMode::Truncated
        } else {
            DisplayMode::Collapsed
        }
    }

    fn image_references(&self) -> &[crate::prompt_images::ScrollbackImageRef] {
        &self.image_refs
    }

    fn inline_media(&self) -> Option<crate::prompt_images::InlineMediaInfo> {
        if self.prefers_text_output() {
            return None;
        }
        if let Some(img) = self.image_refs.first() {
            let (w, h) = img.dimensions?;
            return Some(crate::prompt_images::InlineMediaInfo {
                path: img.path.clone(),
                width: w,
                height: h,
                alt_text: img.alt_text.clone(),
            });
        }
        None
    }

    fn inline_open_button(&self) -> Option<std::path::PathBuf> {
        if self.prefers_text_output() {
            return None;
        }
        // Only used when there is no inline-graphics overlay to host the button
        // row. When the overlay is active it draws its own button row instead.
        if crate::terminal::image::scrollback_inline_overlay_active() {
            return None;
        }
        if let Some(img) = self.image_refs.first() {
            return Some(img.path.clone());
        }
        None
    }
}

#[cfg(test)]
mod coordination_tests {
    use super::*;
    use crate::scrollback::block::RenderBlock;
    use crate::scrollback::blocks::ToolCallBlock;
    use crate::scrollback::state::ScrollbackState;

    #[test]
    fn coordination_image_reference_does_not_replace_text_details() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proof.png");
        image::RgbaImage::new(2, 2).save(&path).unwrap();
        for name in [
            "Answering session peer",
            "list_active_sessions",
            "ask_session",
            "get_inquiry",
        ] {
            for markdown in [false, true] {
                let reference = if markdown {
                    format!("![proof]({})", path.display())
                } else {
                    path.display().to_string()
                };
                let mut block = OtherToolCallBlock::new(name, "").with_output(format!(
                    "Question: progress?\nAnswer: tests passed\nProof: {reference}\nStatus: answered"
                ));
                if name.starts_with("Answering") {
                    block.coordination = Some(CoordinationRow {
                        source_peer_id: "peer".into(),
                        inquiry_id: "one".into(),
                        phase: CoordinationPhase::Received,
                    });
                }
                assert!(
                    !block.image_references().is_empty(),
                    "test must exercise an actual detected image"
                );
                assert!(block.media_ref_path().is_none());
                assert!(block.inline_media().is_none());
                assert!(block.inline_open_button().is_none());
                let mut state = ScrollbackState::new();
                state.push_block(RenderBlock::ToolCall(ToolCallBlock::Other(block)));
                state.set_selected(Some(0));
                state.expand_selected();
                let entry = state.entry(0).unwrap();
                let ctx = entry.context(120, &AppearanceConfig::default(), None);
                let rendered = format!("{:?}", entry.block.output(&ctx));
                for text in [
                    "Question: progress?",
                    "Answer: tests passed",
                    "Status: answered",
                ] {
                    assert!(rendered.contains(text), "missing {text}: {rendered}");
                }
            }
        }
        // Unrelated image tools retain their existing inline-media behavior.
        let ordinary =
            OtherToolCallBlock::new("image_tool", "").with_output(path.display().to_string());
        assert_eq!(ordinary.media_ref_path(), Some(path));
        assert!(ordinary.inline_media().is_some());
        assert_eq!(
            ordinary.next_fold_mode(DisplayMode::Collapsed, true),
            DisplayMode::Truncated
        );
    }

    #[test]
    fn communication_preview_wraps_unicode_to_two_lines() {
        let lines = OtherToolCallBlock::communication_preview_lines(
            "请保留消息正文和来源\n第二行内容\n第三行必须截断",
            16,
            ratatui::style::Style::default(),
            ratatui::style::Style::default(),
        );
        assert_eq!(lines.len(), 2);
        let rendered = format!("{lines:?}");
        assert!(rendered.contains("请保留"), "{rendered}");
        assert!(rendered.contains('…'), "truncated preview: {rendered}");
    }

    #[test]
    fn communication_header_wraps_tool_status_and_preview_at_real_widths() {
        for width in [24, 40, 80, 8] {
            let block = OtherToolCallBlock::new(
                "send_subagent_message → subagent「child-with-a-long-id」",
                "Delivery status unknown · may have been received",
            )
            .with_communication_preview("请保留消息正文和来源\n第二行内容");
            let mut state = ScrollbackState::new();
            state.push_block(RenderBlock::ToolCall(ToolCallBlock::Other(block)));
            let entry = state.entry(0).unwrap();
            let ctx = entry.context(width, &AppearanceConfig::default(), None);
            let content_width = ctx.content_width();
            let output = entry.block.output(&ctx);
            let lines: Vec<String> = output
                .lines
                .iter()
                .map(|line| {
                    line.content
                        .spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<Vec<_>>()
                        .concat()
                })
                .collect();
            let rendered = lines.join("\n");
            assert!(!lines.is_empty(), "width={width}");
            assert!(
                lines.len() <= 6,
                "communication header and preview grew beyond their row budgets at width={width}: {lines:?}"
            );
            for line in &output.lines {
                let line_width = line
                    .content
                    .spans
                    .iter()
                    .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
                    .sum::<usize>();
                assert!(
                    line_width <= width as usize,
                    "width={width}, content_width={content_width}, line={line:?}"
                );
            }
            if width >= 24 {
                assert!(
                    rendered.contains("send_subagent_message"),
                    "tool name missing at width={width}: {rendered}"
                );
                assert!(
                    rendered.contains("subagent"),
                    "target prefix missing at width={width}: {rendered}"
                );
                assert!(
                    rendered.contains("Delivery") && rendered.contains("unknown"),
                    "status missing at width={width}: {rendered}"
                );
                assert!(
                    rendered.contains("请保留"),
                    "preview missing at width={width}: {rendered}"
                );
            }
        }
    }
}

// ── AskUserQuestion output parser ────────────────────────────────────

/// Parse Q&A pairs from an AskUserQuestion tool result string.
///
/// Recognizes all three accepted output formats:
///
/// **Path A (accepted):** `User has answered your questions: "Q1"="A1", "Q2"="A2". You can now...`
/// **Path D (cancelled):** `User declined to answer...`
/// **Paths B/C (plan mode):** `- "Q1"\n  Answer: A1\n- "Q2"\n  (No answer provided)`
///
/// Returns `Vec<(question, answer)>`. Empty vec means the output is not a
/// recognized Q&A format and should be rendered generically.
fn parse_ask_user_qa_pairs(output: &str) -> Vec<(String, String)> {
    // Path A: "User has answered your questions: "Q"="A", "Q"="A". You can now..."
    if let Some(rest) = output.strip_prefix("User has answered your questions: ") {
        // Strip the trailing ". You can now continue with the user's answers in mind."
        let body = rest
            .strip_suffix(". You can now continue with the user's answers in mind.")
            .unwrap_or(rest);

        if body.is_empty() {
            return vec![];
        }

        // Parse "Q1"="A1", "Q2"="A2" pairs.
        // Split on `", "` that appears between pairs (after `"="value"`).
        let mut pairs = Vec::new();
        let mut remaining = body;

        while !remaining.is_empty() {
            // Expect: "question"="answer" [optional annotations...]
            if !remaining.starts_with('"') {
                break;
            }
            remaining = &remaining[1..]; // skip opening "

            // Find the closing " before =
            let Some(q_end) = remaining.find("\"=\"") else {
                break;
            };
            let question = remaining[..q_end].to_string();
            remaining = &remaining[q_end + 3..]; // skip "="

            // Find the end of the answer: next `", "` pair start or end of string.
            // The answer value continues until we hit `, "` (next pair) or end.
            let answer_end = remaining.find(", \"").unwrap_or(remaining.len());

            let mut answer_text = remaining[..answer_end].to_string();
            // Strip trailing quote if present (answer is quoted)
            if answer_text.ends_with('"') {
                answer_text.pop();
            }

            // Remove annotation suffixes (selected preview:..., user notes:...)
            // for display — keep just the label.
            if let Some(ann_start) = answer_text.find(" selected preview:") {
                answer_text.truncate(ann_start);
            }
            if let Some(ann_start) = answer_text.find(" user notes:") {
                answer_text.truncate(ann_start);
            }

            pairs.push((question, answer_text));

            // Advance past the separator
            remaining = &remaining[answer_end..];
            if remaining.starts_with(", ") {
                remaining = &remaining[2..];
            }
        }

        return pairs;
    }

    // Path D: cancelled
    if output.starts_with("User declined to answer") {
        return vec![]; // No Q&A to show
    }

    // Paths B/C: plan mode — bullet format
    // - "Q1"\n  Answer: A1\n- "Q2"\n  (No answer provided)
    if output.contains("Questions asked") && output.contains("- \"") {
        let mut pairs = Vec::new();
        let lines: Vec<&str> = output.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let line = lines[i].trim_start_matches([' ', '-']).trim();
            // Check for "question text"
            if line.starts_with('"') && line.ends_with('"') {
                let question = line[1..line.len() - 1].to_string();
                let answer = if i + 1 < lines.len() {
                    let next = lines[i + 1].trim();
                    if let Some(a) = next.strip_prefix("Answer: ") {
                        i += 1;
                        a.to_string()
                    } else if next == "(No answer provided)" {
                        i += 1;
                        String::new()
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };
                pairs.push((question, answer));
            }
            i += 1;
        }
        if !pairs.is_empty() {
            return pairs;
        }
    }

    vec![]
}
