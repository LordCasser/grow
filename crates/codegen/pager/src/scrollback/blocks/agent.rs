//! AgentMessageBlock - displays agent responses with markdown.

use shell::sampling::conversation::IMAGE_PROJECTION_PLACEHOLDERS;

use crate::scrollback::block::BlockContent;
use crate::scrollback::types::{AccentStyle, BlockContext, BlockOutput};

use super::markdown_content::MarkdownContent;
use super::mermaid_content::{self, MermaidContent};
use crate::appearance::AppearanceConfig;

/// Block displaying an agent message with streaming markdown support.
///
/// This block uses [`MarkdownContent`] for incremental markdown rendering
/// with cached word-wrapping.  When text arrives in chunks, call
/// `push_chunk()` to append without re-rendering everything.
///
/// When `ctx.raw` is false, renders pretty markdown (hiding syntax).
/// When `ctx.raw` is true, renders the source markdown as-is.
#[derive(Debug, Clone)]
pub struct AgentMessageBlock {
    content: MarkdownContent,
    /// A possible leading internal image-projection marker, withheld until the
    /// stream either diverges from every marker or finishes.
    pending_image_projection_prefix: Option<String>,
    /// Prefix filtering only applies until the first non-marker byte arrives.
    filter_image_projection_prefix: bool,
    /// An exact internal marker is retained as an empty, non-rendering entry.
    hidden_image_projection_placeholder: Option<&'static str>,
    /// Cached image references extracted from the markdown source.
    image_refs: Vec<crate::prompt_images::ScrollbackImageRef>,
    /// Detected ` ```mermaid ` diagrams + render skeleton, populated at
    /// construction/finish (never per streaming chunk) like the media refs.
    mermaid: MermaidContent,
}

impl AgentMessageBlock {
    /// Create a new agent message block with complete text.
    pub fn new(text: impl Into<String>) -> Self {
        let mut text = text.into();
        let hidden_image_projection_placeholder = Self::image_projection_placeholder(&text);
        if hidden_image_projection_placeholder.is_some() {
            text.clear();
        }
        let image_refs = crate::prompt_images::extract_image_refs(&text);
        let content = MarkdownContent::new(text);
        let mermaid = content.mermaid_content();
        Self {
            content,
            pending_image_projection_prefix: None,
            filter_image_projection_prefix: false,
            hidden_image_projection_placeholder,
            image_refs,
            mermaid,
        }
    }

    /// Create an empty block for streaming.
    pub fn streaming() -> Self {
        Self {
            content: MarkdownContent::streaming(),
            pending_image_projection_prefix: None,
            filter_image_projection_prefix: true,
            hidden_image_projection_placeholder: None,
            image_refs: Vec::new(),
            mermaid: MermaidContent::default(),
        }
    }

    /// Push a streaming chunk of markdown text.
    pub fn push_chunk(&mut self, chunk: &str) {
        self.push_filtered_chunk(chunk, false);
    }

    /// Push a chunk without rendering immediately.
    pub fn push_chunk_deferred(&mut self, chunk: &str) {
        self.push_filtered_chunk(chunk, true);
    }

    fn push_filtered_chunk(&mut self, chunk: &str, deferred: bool) {
        if chunk.is_empty() {
            return;
        }
        // Some child/replay paths finalize blocks before late chunks arrive.
        // With a suffix this is ordinary text, so restore the complete body.
        if let Some(marker) = self.hidden_image_projection_placeholder.take() {
            self.push_content_chunk(marker, deferred);
        }
        if !self.filter_image_projection_prefix {
            self.push_content_chunk(chunk, deferred);
            return;
        }
        let Some(pending) = self.pending_image_projection_prefix.take() else {
            if Self::could_be_image_projection_prefix(chunk) {
                self.pending_image_projection_prefix = Some(chunk.to_owned());
            } else {
                self.filter_image_projection_prefix = false;
                self.push_content_chunk(chunk, deferred);
            }
            return;
        };

        let max_len = Self::max_image_projection_placeholder_len();
        if chunk.len() > max_len.saturating_sub(pending.len()) {
            self.filter_image_projection_prefix = false;
            self.push_content_chunk(&pending, deferred);
            self.push_content_chunk(chunk, deferred);
            return;
        }

        let mut candidate = pending;
        candidate.push_str(chunk);
        if Self::could_be_image_projection_prefix(&candidate) {
            self.pending_image_projection_prefix = Some(candidate);
        } else {
            self.filter_image_projection_prefix = false;
            self.push_content_chunk(&candidate, deferred);
        }
    }

    fn push_content_chunk(&mut self, chunk: &str, deferred: bool) {
        if deferred {
            self.content.push_chunk_deferred(chunk);
        } else {
            self.content.push_chunk(chunk);
        }
    }

    fn max_image_projection_placeholder_len() -> usize {
        IMAGE_PROJECTION_PLACEHOLDERS
            .iter()
            .map(|placeholder| placeholder.len())
            .max()
            .unwrap_or(0)
    }

    fn image_projection_placeholder(text: &str) -> Option<&'static str> {
        IMAGE_PROJECTION_PLACEHOLDERS
            .into_iter()
            .find(|placeholder| *placeholder == text)
    }

    fn could_be_image_projection_prefix(text: &str) -> bool {
        text.len() <= Self::max_image_projection_placeholder_len()
            && IMAGE_PROJECTION_PLACEHOLDERS
                .iter()
                .any(|placeholder| placeholder.starts_with(text))
    }

    /// Get the source markdown text.
    pub fn text(&self) -> String {
        self.content.text()
    }

    /// Whether the source markdown is empty (zero-alloc, unlike `text()`).
    pub fn is_empty(&self) -> bool {
        self.content.is_empty() && self.pending_image_projection_prefix.is_none()
    }

    /// Finish streaming and do a full re-render for safety.
    pub fn finish(&mut self) {
        if let Some(pending) = self.pending_image_projection_prefix.take() {
            if let Some(marker) = Self::image_projection_placeholder(&pending) {
                self.hidden_image_projection_placeholder = Some(marker);
            } else {
                self.content.push_chunk_deferred(&pending);
            }
        }
        self.filter_image_projection_prefix = false;
        self.content.finish();
        let text = self.content.text();
        self.image_refs = crate::prompt_images::extract_image_refs(&text);
        // Detection runs once the render is final, after the renderer freezes —
        // never per streaming chunk.
        self.mermaid = self.content.mermaid_content();
    }

    /// The detected Mermaid diagrams for this message (empty until finished or
    /// constructed from complete text).
    pub fn mermaid(&self) -> &MermaidContent {
        &self.mermaid
    }

    /// Set the raw mode, re-rendering if it changed.
    pub fn set_raw_mode(&mut self, raw: bool) {
        self.content.set_raw_mode(raw);
    }

    /// Access the underlying markdown content (for viewer item building).
    pub fn content(&self) -> &MarkdownContent {
        &self.content
    }

    /// Mutable access to the underlying markdown content.
    pub fn content_mut(&mut self) -> &mut MarkdownContent {
        &mut self.content
    }

    /// Get copyable text for this block.
    ///
    /// When `raw` is true, returns the raw markdown source.
    /// When `raw` is false, returns the rendered text (styles stripped).
    pub fn copy_text(&self, raw: bool) -> String {
        if raw {
            self.content.text()
        } else {
            self.content.rendered_plain_text()
        }
    }
}

impl AgentMessageBlock {
    /// Resolve the diagram display mode from the user setting without building
    /// `output()` — cheap enough to gate the per-frame affordance path.
    fn mermaid_display_mode(&self) -> mermaid_content::MermaidDisplay {
        // Minimal mode commits static text with no draw loop to paint the
        // clickable affordance row, so suppress it there (the diagram art still
        // renders; its source stays natively selectable). The inline-overlay
        // force-off flag is set iff minimal.
        mermaid_content::mermaid_display_static(
            crate::appearance::cache::load_render_mermaid(),
            crate::terminal::image::scrollback_inline_overlay_forced_off(),
        )
    }

    /// Build the block's output and the diagram affordance rows together so the
    /// inserted rows (in the output) and the anchored placements (their offsets)
    /// are always derived from the same layout.
    ///
    /// [`output`](Self::output) and [`diagram_affordances`](Self::diagram_affordances)
    /// each call this independently (so it runs twice per frame for a diagram
    /// message); it is deterministic for a given `ctx`, so the two calls produce
    /// matching rows + offsets without a shared cache that could drift.
    ///
    /// Only callers that have already confirmed there are diagrams and we are
    /// not in raw mode should reach here (so the common diagram-free path never
    /// pays this build).
    fn rendered_output(
        &self,
        ctx: &BlockContext,
    ) -> (BlockOutput, Vec<mermaid_content::DiagramAffordance>) {
        let mut out = self.content.output(ctx.width as usize);
        // Diagram pre-wrap ranges in document order. The fence count and order
        // are width-invariant, so range index `idx` pairs positionally with the
        // diagram's source (`self.mermaid.source(idx)`).
        let ranges = self.content.mermaid_block_ranges();

        match self.mermaid_display_mode() {
            mermaid_content::MermaidDisplay::SourceOnly => (out, Vec::new()),
            mermaid_content::MermaidDisplay::Affordances => {
                let affordances =
                    mermaid_content::apply_affordance_rows(&mut out, &ranges, |idx| {
                        self.mermaid.source(idx).unwrap_or_default().to_string()
                    });
                (out, affordances)
            }
        }
    }
}

impl BlockContent for AgentMessageBlock {
    fn output(&self, ctx: &BlockContext) -> BlockOutput {
        if self.hidden_image_projection_placeholder.is_some() {
            return BlockOutput::new();
        }
        // Common path: no diagrams (or raw mode) → plain markdown, no affordance
        // machinery and no extra output rebuild.
        if ctx.raw || self.mermaid.is_empty() {
            return self.content.output(ctx.width as usize);
        }
        self.rendered_output(ctx).0
    }

    fn diagram_affordances(&self, ctx: &BlockContext) -> Vec<mermaid_content::DiagramAffordance> {
        // Affordance rows exist only under the affordance display with diagrams;
        // for every other (much more common) case, return without building
        // output().
        if ctx.raw
            || self.mermaid.is_empty()
            || self.mermaid_display_mode() != mermaid_content::MermaidDisplay::Affordances
        {
            return Vec::new();
        }
        self.rendered_output(ctx).1
    }

    fn estimate_extra_rows(&self) -> u16 {
        // Each detected diagram inserts one treatment row (affordance row or
        // fallback caption) into output() that the source-text estimate can't
        // see. Count one per diagram (a safe over-estimate if a range is empty)
        // so the off-screen estimate never under-reserves; raw mode and the
        // `off` setting add no such row.
        if self.mermaid.is_empty()
            || self.content.is_raw()
            || self.mermaid_display_mode() == mermaid_content::MermaidDisplay::SourceOnly
        {
            return 0;
        }
        self.mermaid.len() as u16
    }

    fn accent(&self, _ctx: &BlockContext) -> Option<AccentStyle> {
        None
    }

    fn has_vpad_for(&self, _appearance: &AppearanceConfig) -> bool {
        false
    }

    fn has_raw_mode(&self) -> bool {
        true
    }

    fn is_foldable(&self) -> bool {
        false
    }

    fn image_references(&self) -> &[crate::prompt_images::ScrollbackImageRef] {
        &self.image_refs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{AppearanceConfig, RenderMermaid};
    use crate::scrollback::types::Selectable;

    fn ctx(width: u16, raw: bool) -> BlockContext {
        BlockContext {
            mode: crate::scrollback::DisplayMode::Expanded,
            is_running: false,
            width,
            raw,
            max_lines: None,
            appearance: AppearanceConfig::default(),
            is_selected: false,
            cwd: None,
        }
    }

    #[test]
    fn agent_markdown_body_uses_single_logical_range() {
        let block = AgentMessageBlock::new("hello world this should wrap across lines");
        let out = block.output(&ctx(10, false));
        assert!(out.lines.len() > 1);
        assert!(out.lines.iter().all(|line| line.selection_range == Some(0)));
        assert!(
            out.lines
                .iter()
                .all(|line| !matches!(line.selectable, Selectable::None))
        );
    }

    #[test]
    fn agent_copy_text_preserves_raw_semantics() {
        let block = AgentMessageBlock::new("**bold** text");
        assert_eq!(block.copy_text(true), "**bold** text");
        assert_eq!(
            block.copy_text(false),
            block.content().rendered_plain_text()
        );
    }

    #[test]
    fn exact_image_projection_placeholders_are_hidden_at_every_chunk_boundary() {
        for placeholder in IMAGE_PROJECTION_PLACEHOLDERS {
            assert!(AgentMessageBlock::new(placeholder).is_empty());
            for split in 0..=placeholder.len() {
                let mut block = AgentMessageBlock::streaming();
                block.push_chunk(&placeholder[..split]);
                block.push_chunk(&placeholder[split..]);
                assert!(
                    !block.is_empty(),
                    "possible marker remains buffered until finish: {placeholder:?} split at {split}"
                );
                block.finish();
                assert!(block.is_empty(), "{placeholder:?} split at {split}");
                assert_eq!(block.text(), "");
                assert_eq!(block.copy_text(true), "");
                assert_eq!(block.copy_text(false), "");
                assert!(block.output(&ctx(80, false)).lines.is_empty());
                assert!(block.output(&ctx(80, true)).lines.is_empty());
            }
        }
    }

    #[test]
    fn deferred_streaming_hides_exact_image_projection_placeholder() {
        let placeholder = IMAGE_PROJECTION_PLACEHOLDERS[1];
        let mut block = AgentMessageBlock::streaming();
        block.push_chunk_deferred(&placeholder[..9]);
        block.push_chunk_deferred(&placeholder[9..]);
        assert!(!block.is_empty());
        block.finish();
        assert!(block.is_empty());
        assert_eq!(block.text(), "");
    }

    #[test]
    fn image_projection_prefix_divergence_and_incomplete_finish_are_preserved() {
        let placeholder = IMAGE_PROJECTION_PLACEHOLDERS[0];

        let mut divergent = AgentMessageBlock::streaming();
        divergent.push_chunk(&placeholder[..5]);
        assert!(!divergent.is_empty());
        divergent.push_chunk("ordinary text");
        assert_eq!(
            divergent.text(),
            format!("{}ordinary text", &placeholder[..5])
        );
        divergent.finish();
        assert_eq!(
            divergent.copy_text(true),
            format!("{}ordinary text", &placeholder[..5])
        );

        let mut incomplete = AgentMessageBlock::streaming();
        incomplete.push_chunk(&placeholder[..placeholder.len() - 1]);
        assert!(!incomplete.is_empty());
        assert_eq!(incomplete.text(), "");
        incomplete.finish();
        assert_eq!(incomplete.text(), &placeholder[..placeholder.len() - 1]);
        assert!(!incomplete.is_empty());
    }

    #[test]
    fn image_projection_placeholder_inside_explanation_is_preserved() {
        let placeholder = IMAGE_PROJECTION_PLACEHOLDERS[1];
        let explanation =
            format!("The internal marker is {placeholder} and should remain visible.");
        let block = AgentMessageBlock::new(&explanation);
        assert_eq!(block.text(), explanation);

        let mut streamed = AgentMessageBlock::streaming();
        streamed.push_chunk(placeholder);
        streamed.push_chunk(" This suffix makes it an explanation.");
        assert_eq!(
            streamed.text(),
            format!("{placeholder} This suffix makes it an explanation.")
        );
        streamed.finish();
        assert_eq!(
            streamed.copy_text(true),
            format!("{placeholder} This suffix makes it an explanation.")
        );
    }

    #[test]
    fn image_projection_hidden_body_is_restored_for_late_explanatory_chunks() {
        for marker in IMAGE_PROJECTION_PLACEHOLDERS {
            for deferred in [false, true] {
                for constructed in [false, true] {
                    let mut block = if constructed {
                        AgentMessageBlock::new(marker)
                    } else {
                        let mut block = AgentMessageBlock::streaming();
                        block.push_chunk(marker);
                        block.finish();
                        block
                    };
                    block.finish(); // completion is idempotent
                    assert!(block.output(&ctx(120, true)).lines.is_empty());
                    block.push_filtered_chunk(" Explanation.", deferred);
                    block.finish();
                    assert_eq!(block.text(), format!("{marker} Explanation."));
                    assert_eq!(block.copy_text(true), block.text());
                    assert!(!block.output(&ctx(120, true)).lines.is_empty());
                    assert!(!block.output(&ctx(120, false)).lines.is_empty());
                }
            }
        }
    }

    #[test]
    fn mermaid_detected_at_construction() {
        let block = AgentMessageBlock::new("```mermaid\nflowchart TD\n  A --> B\n```\n");
        assert_eq!(block.mermaid().len(), 1);
        assert_eq!(block.mermaid().source(0), Some("flowchart TD\n  A --> B\n"));
    }

    #[test]
    fn mermaid_not_detected_during_streaming_until_finish() {
        let mut block = AgentMessageBlock::streaming();
        block.push_chunk("```mermaid\nflowchart TD\n");
        // Fence still open mid-stream → no detection.
        assert!(block.mermaid().is_empty());
        block.push_chunk("A --> B\n```\n");
        assert!(
            block.mermaid().is_empty(),
            "detection runs at finish(), not per chunk"
        );
        block.finish();
        assert_eq!(block.mermaid().len(), 1);
    }

    const MERMAID_MD: &str = "```mermaid\nflowchart TD\n  A --> B\n```\n";

    #[test]
    fn mermaid_treatment_row_shown_in_auto_not_off_not_raw() {
        let non_selectable = |o: &BlockOutput| {
            o.lines
                .iter()
                .filter(|l| matches!(l.selectable, Selectable::None))
                .count()
        };
        // off → plain code block, no extra row.
        crate::appearance::cache::set_render_mermaid(RenderMermaid::Off);
        let off = AgentMessageBlock::new(MERMAID_MD).output(&ctx(40, false));
        assert_eq!(non_selectable(&off), 0, "off mode must not add a row");

        // auto → exactly one extra non-selectable row beneath the diagram (the
        // affordance row). The row is blank in `output()` — the draw loop paints
        // its `◇ mermaid [Open Image] [Copy Image Path] [Copy Source]` buttons.
        crate::appearance::cache::set_render_mermaid(RenderMermaid::Auto);
        let auto = AgentMessageBlock::new(MERMAID_MD).output(&ctx(40, false));
        assert_eq!(
            auto.lines.len(),
            off.lines.len() + 1,
            "auto mode adds one treatment row"
        );
        assert_eq!(non_selectable(&auto), 1);

        // raw → verbatim source, no extra row even in auto.
        crate::appearance::cache::set_render_mermaid(RenderMermaid::Auto);
        let raw = AgentMessageBlock::new(MERMAID_MD).output(&ctx(40, true));
        assert_eq!(non_selectable(&raw), 0, "raw mode shows the fence verbatim");
    }

    #[test]
    fn mermaid_treatment_row_preserves_hyperlink_line_mapping() {
        // The inserted treatment row (caption or affordance) is a joiner-
        // continuation line, so it must NOT add a logical (pre-wrap) line —
        // otherwise the hyperlink overlay walk desyncs for the paragraph after
        // the diagram.
        let md = "before\n\n```mermaid\nA-->B\n```\n\n[link](https://example.com) trailing\n";
        crate::appearance::cache::set_render_mermaid(RenderMermaid::Off);
        let off = AgentMessageBlock::new(md).output(&ctx(60, false));
        crate::appearance::cache::set_render_mermaid(RenderMermaid::Auto);
        let block = AgentMessageBlock::new(md);
        let auto = block.output(&ctx(60, false));

        let logical = |o: &BlockOutput| o.lines.iter().filter(|l| l.joiner.is_none()).count();
        assert_eq!(
            logical(&auto),
            logical(&off),
            "treatment row must not introduce a new logical line",
        );

        // The renderer's hyperlinks are pre-wrap and unchanged by the inserted
        // row (it lives in the BlockOutput, not the renderer). Walk the output's
        // joiners to recover each row's pre-wrap index and confirm the link's
        // pre-wrap line still maps to its row — i.e. the row did not shift it.
        let link_line = block
            .content()
            .with_hyperlinks(|hs| hs.iter().map(|h| h.line_index).min())
            .expect("the trailing link must be detected");
        let mut prewrap = 0usize;
        let mut mapped_text = String::new();
        for (row, line) in auto.lines.iter().enumerate() {
            if row > 0 && line.joiner.is_none() {
                prewrap += 1;
            }
            if prewrap == link_line {
                mapped_text = line
                    .content
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect();
                break;
            }
        }
        assert!(
            mapped_text.contains("example.com"),
            "link pre-wrap line {link_line} must still map to the link row, got {mapped_text:?}",
        );
    }

    // -- diagram affordance rows ---------------------------------------------

    mod affordances {
        use super::*;

        fn shown_text(out: &BlockOutput) -> String {
            out.lines
                .iter()
                .flat_map(|l| l.content.spans.iter())
                .map(|s| s.content.as_ref())
                .collect()
        }

        /// A detected diagram keeps its source code block on screen and exposes a
        /// single affordance row carrying the diagram source (the data every
        /// lazy `[Open]`/`[Copy path]`/`[Copy source]` button acts on). Rendering
        /// is lazy, so no path/state is tracked on the row.
        #[test]
        fn diagram_exposes_affordance_carrying_source_and_keeps_source_block() {
            crate::appearance::cache::set_render_mermaid(RenderMermaid::On);
            let block = AgentMessageBlock::new("intro\n\n```mermaid\nA-->B\n```\n\nbye\n");

            let affs = block.diagram_affordances(&ctx(60, false));
            assert_eq!(affs.len(), 1, "one diagram → one affordance row");
            assert_eq!(affs[0].source, "A-->B\n");

            // The diagram is shown as its source code block (never an image), and
            // the affordance row sits at its reported (non-selectable) offset.
            let out = block.output(&ctx(60, false));
            assert!(
                shown_text(&out).contains("A-->B"),
                "the source code block stays on screen",
            );
            assert!(matches!(
                out.lines[affs[0].row_offset as usize].selectable,
                Selectable::None
            ));
        }

        #[test]
        fn raw_mode_suppresses_affordances_and_shows_source() {
            crate::appearance::cache::set_render_mermaid(RenderMermaid::On);
            let block = AgentMessageBlock::new("```mermaid\nA-->B\n```\n");

            assert!(
                block.diagram_affordances(&ctx(60, true)).is_empty(),
                "raw mode suppresses affordances",
            );
            assert!(shown_text(&block.output(&ctx(60, true))).contains("A-->B"));

            // Toggling back to pretty restores the affordance row.
            assert_eq!(block.diagram_affordances(&ctx(60, false)).len(), 1);
        }

        #[test]
        fn off_setting_shows_source_with_no_affordances() {
            crate::appearance::cache::set_render_mermaid(RenderMermaid::Off);
            let block = AgentMessageBlock::new("```mermaid\nA-->B\n```\n");
            assert!(block.diagram_affordances(&ctx(60, false)).is_empty());
            assert!(shown_text(&block.output(&ctx(60, false))).contains("A-->B"));
        }

        #[test]
        fn copy_over_diagram_yields_fence_body() {
            use crate::scrollback::block::RenderBlock;
            crate::appearance::cache::set_render_mermaid(RenderMermaid::On);
            // Drive the real whole-block copy path (`copy_visible_text_in_state`
            // → `plain_text_from_output`) rather than re-implementing the
            // selectable filter, so the test tracks production: the source code
            // block is selectable, the blank affordance row is excluded.
            let block = RenderBlock::AgentMessage(AgentMessageBlock::new(
                "```mermaid\nA-->B\nC-->D\n```\n",
            ));
            let copied = block
                .copy_visible_text_in_state(&ctx(60, false))
                .expect("the source code block yields selectable copy text");
            assert!(copied.contains("A-->B"), "copy yields source: {copied:?}");
            assert!(copied.contains("C-->D"), "copy yields source: {copied:?}");
        }

        /// With two diagrams, each affordance row anchors at its OWN
        /// (non-selectable) row in the final output, in document order, and
        /// carries that diagram's own source.
        #[test]
        fn two_diagrams_each_anchor_at_their_own_row() {
            crate::appearance::cache::set_render_mermaid(RenderMermaid::On);
            let md = "intro line\n\n```mermaid\nAAA-->BBB\n```\n\nmid line\n\n```mermaid\nCCC-->DDD\n```\n\nbye line\n";
            let block = AgentMessageBlock::new(md);
            assert_eq!(block.mermaid().len(), 2, "two diagrams");

            let out = block.output(&ctx(60, false));
            let affs = block.diagram_affordances(&ctx(60, false));
            assert_eq!(affs.len(), 2);
            assert!(
                affs[0].row_offset < affs[1].row_offset,
                "diagram order preserved: {} < {}",
                affs[0].row_offset,
                affs[1].row_offset,
            );
            assert_eq!(affs[0].source, "AAA-->BBB\n");
            assert_eq!(affs[1].source, "CCC-->DDD\n");
            for aff in &affs {
                assert!(matches!(
                    out.lines[aff.row_offset as usize].selectable,
                    Selectable::None
                ));
            }
            // Both diagrams' sources remain visible as code blocks.
            assert!(shown_text(&out).contains("AAA-->BBB"));
            assert!(shown_text(&out).contains("CCC-->DDD"));
        }
    }
}
