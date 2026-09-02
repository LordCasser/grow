//! Plain-URL detection over rendered display ratatui Lines.

use linkify::{LinkFinder, LinkKind};
use ratatui::text::Line;
use std::ops::Range;
use std::sync::OnceLock;

use crate::buffers::unicode_display_width;
use crate::output::HyperlinkTarget;

/// Infer bare URLs in prose, returning UTF-8 byte ranges into the unchanged input.
/// Explicit Markdown destinations must never pass through these heuristics.
/// Unicode whitespace and sentence wrappers delimit prose. CJK list punctuation
/// delimits paths, but is retained inside query/fragment data unless it introduces
/// another URL or ends the token. ASCII commas/semicolons split adjacent URLs only
/// outside query/fragment data (a redirect or URL-list parameter is one URL).
pub fn plain_url_ranges(text: &str) -> Vec<Range<usize>> {
    static FINDER: OnceLock<LinkFinder> = OnceLock::new();
    let finder = FINDER.get_or_init(|| {
        let mut finder = LinkFinder::new();
        finder.kinds(&[LinkKind::Url]);
        finder
    });
    let mut ranges = Vec::new();
    let mut offset = 0;
    let prose_boundary = |c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '\u{200b}'
                    | '。'
                    | '！'
                    | '？'
                    | '（'
                    | '）'
                    | '“'
                    | '”'
                    | '‘'
                    | '’'
                    | '「'
                    | '」'
                    | '『'
                    | '』'
            )
    };
    for token in text.split_inclusive(prose_boundary) {
        let token_text = token.trim_end_matches(prose_boundary);
        for link in finder.links(token_text) {
            let candidate = link.as_str();
            let base = offset + link.start();
            let mut start = 0;
            let mut in_data = false;
            let mut emit = |from: usize, to: usize| {
                let bounded = candidate[from..to].trim_end_matches(['，', '、', '；', '：']);
                for part in finder.links(bounded) {
                    ranges.push(base + from + part.start()..base + from + part.end());
                }
            };
            // Partition each candidate in one pass. Repeatedly asking linkify
            // to scan the remaining suffix is quadratic for comma-joined lists.
            for (i, c) in candidate.char_indices() {
                if matches!(c, '?' | '#') {
                    in_data = true;
                }
                if !matches!(c, '，' | '、' | '；' | '：' | ',' | ';') {
                    continue;
                }
                let after = &candidate[i + c.len_utf8()..];
                let next_url = ["https://", "http://"].iter().any(|scheme| {
                    after
                        .get(..scheme.len())
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
                });
                let split = match c {
                    '，' | '、' | '；' | '：' => !in_data || next_url || after.is_empty(),
                    ',' | ';' => !in_data && next_url,
                    _ => false,
                };
                if split {
                    emit(start, i);
                    start = i + c.len_utf8();
                    in_data = false;
                }
            }
            if start == 0 {
                ranges.push(base..offset + link.end());
            } else {
                emit(start, candidate.len());
            }
        }
        offset += token.len();
    }
    ranges
}

/// Scan `lines` for plain URLs and return new `HyperlinkTarget` entries
/// that don't overlap any existing target in `existing`.
///
/// `next_id` is the first id to assign; the returned `u32` is the
/// post-scan counter, suitable for stuffing back into
/// `FrozenState::next_link_id`.
pub(crate) fn detect_plain_urls(
    lines: &[Line<'_>],
    existing: &[HyperlinkTarget],
    next_id: u32,
) -> (Vec<HyperlinkTarget>, u32) {
    detect_plain_urls_with_offset(lines, 0, existing, next_id)
}

/// Like [`detect_plain_urls`] but scans `lines` whose first element
/// represents document line `line_index_offset` (caller passes a tail
/// slice of `self.output.lines` and the index of its first element).
///
/// Lines fully inside `0..line_index_offset` are assumed to be in
/// `existing` already and are not re-scanned.  The dedup overlap check
/// still works correctly because emitted targets use document-absolute
/// `line_index = line_index_offset + i`, matching the indices already
/// present in `existing`.
pub(crate) fn detect_plain_urls_with_offset(
    lines: &[Line<'_>],
    line_index_offset: usize,
    existing: &[HyperlinkTarget],
    next_id: u32,
) -> (Vec<HyperlinkTarget>, u32) {
    let mut result = Vec::new();
    let mut current_id = next_id;
    for (i, line) in lines.iter().enumerate() {
        let line_index = line_index_offset + i;
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        for range in plain_url_ranges(&text) {
            let before = &text[..range.start];
            let matched = &text[range];

            let col_start = unicode_display_width(before);
            let col_end = col_start + unicode_display_width(matched);
            let url = matched.to_string();

            // Dedup: skip if any existing or already-added target overlaps
            // on the same line. Overlap: cand.start < ex.end && ex.start < cand.end.
            let overlaps = existing.iter().chain(result.iter()).any(|h| {
                h.line_index == line_index
                    && col_start < h.column_range.end
                    && h.column_range.start < col_end
            });

            if !overlaps {
                result.push(HyperlinkTarget {
                    line_index,
                    column_range: col_start..col_end,
                    url,
                    id: current_id,
                    provenance: crate::HyperlinkProvenance::Inferred,
                });
                current_id += 1;
            }
        }
    }

    (result, current_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StreamingMarkdownRenderer;
    use crate::style::test_style;

    #[test]
    fn prose_boundaries_preserve_original_targets() {
        for separator in [
            "，", "、", ",", ";", "， ", "\u{a0}", "\u{3000}", "\u{200b}", "\n", "\t",
        ] {
            let text = format!("https://a.test/x{separator}https://b.test/y");
            let urls: Vec<_> = plain_url_ranges(&text)
                .into_iter()
                .map(|r| &text[r])
                .collect();
            assert_eq!(
                urls,
                ["https://a.test/x", "https://b.test/y"],
                "{separator:?}"
            );
        }
        for text in [
            "（https://a.test/x）",
            "“https://a.test/x”",
            "https://a.test/x。下一句",
            "https://a.test/x，解释",
        ] {
            let urls: Vec<_> = plain_url_ranges(text)
                .into_iter()
                .map(|r| &text[r])
                .collect();
            assert_eq!(urls, ["https://a.test/x"], "{text}");
        }
        for text in [
            "https://a.test/a,b",
            "https://a.test/?ids=1,2",
            "https://a.test/a;version=2",
            "https://a.test/?next=https://b.test/x",
            "https://a.test/?urls=https://b.test/x,https://c.test/y",
            "https://a.test/中文路径?q=天气，交通",
            "https://a.test/a%2Cb%EF%BC%8Cc",
            "https://a.test/x(foo)",
        ] {
            assert_eq!(plain_url_ranges(text), [0..text.len()], "{text}");
        }
    }

    #[test]
    fn plain_url_crosses_style_spans() {
        let lines = [Line::from(vec![
            ratatui::text::Span::raw("中文 https://"),
            ratatui::text::Span::styled(
                "a.test/",
                ratatui::style::Style::default().fg(ratatui::style::Color::Red),
            ),
            ratatui::text::Span::raw("path，说明"),
        ])];
        let (links, _) = detect_plain_urls(&lines, &[], 0);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://a.test/path");
        assert_eq!(links[0].column_range, 5..24);
    }

    #[test]
    fn explicit_destination_bypasses_prose_heuristics() {
        let url = "https://a.test/中文，路径?q=天气，交通";
        let links = finish_and_get_hyperlinks(&format!("[文档]({url})"));
        assert!(links.iter().any(
            |link| link.provenance == crate::HyperlinkProvenance::Explicit && link.url == url
        ));
    }

    #[test]
    fn streaming_chunk_boundaries_do_not_freeze_partial_urls() {
        let text = "中文 https://a.test/path，https://b.test/?next=https://inner.test/x\n";
        let expected = finish_and_get_hyperlinks(text);
        for split in text.char_indices().map(|(i, _)| i) {
            let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
            renderer.push_and_render(&text[..split], None);
            renderer.push_and_render(&text[split..], None);
            let actual = renderer.view().hyperlinks;
            assert_eq!(actual.len(), expected.len(), "split={split}");
            for (actual, expected) in actual.iter().zip(&expected) {
                assert_eq!(actual.url, expected.url, "split={split}");
                assert_eq!(actual.column_range, expected.column_range);
            }
        }
    }

    #[test]
    fn compact_prose_and_table_links_use_the_same_boundaries() {
        let first = "https://a.test/a-very-long-directory/another-long-directory/report-one.pdf";
        let second = "https://b.test/a-very-long-directory/another-long-directory/report-two.pdf";
        for text in [
            format!("{first}、{second}，说明"),
            format!("| 文档 |\n|---|\n| {first}、{second}，说明 |\n"),
        ] {
            let links = finish_and_get_hyperlinks(&text);
            for url in [first, second] {
                assert!(links.iter().any(|link| link.url == url), "{links:?}");
            }
            assert!(
                links
                    .iter()
                    .all(|link| !link.url.contains('、') && !link.url.contains('，'))
            );
        }
    }

    #[test]
    fn punctuation_runs_after_queries_do_not_enter_destinations() {
        for separator in ["，", "，、", "，，", "， "] {
            let text = format!("https://a.test/?ids=1,2{separator}HTTPS://b.test/path");
            let urls: Vec<_> = plain_url_ranges(&text)
                .into_iter()
                .map(|r| &text[r])
                .collect();
            assert_eq!(urls, ["https://a.test/?ids=1,2", "HTTPS://b.test/path"]);
        }
    }

    #[test]
    fn many_adjacent_urls_preserve_each_range() {
        let urls: Vec<_> = (0..500)
            .map(|i| format!("https://example.test/article/{i}"))
            .collect();
        let text = urls.join("，");
        let actual: Vec<_> = plain_url_ranges(&text)
            .into_iter()
            .map(|range| &text[range])
            .collect();
        assert_eq!(actual, urls);
    }

    #[test]
    fn entity_separators_do_not_become_compact_url_data() {
        let first = "https://a.test/a-very-long-directory/another-long-directory/report-one.pdf";
        let second = "https://b.test/a-very-long-directory/another-long-directory/report-two.pdf";
        let links = finish_and_get_hyperlinks(&format!("{first}&nbsp;{second}"));
        assert!(links.iter().any(|link| link.url == first), "{links:?}");
        assert!(links.iter().any(|link| link.url == second), "{links:?}");
    }

    /// Helper: render markdown via StreamingMarkdownRenderer::finish() and
    /// return the hyperlinks from the finalized output.
    fn finish_and_get_hyperlinks(text: &str) -> Vec<HyperlinkTarget> {
        let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
        renderer.push_and_render(text, None);
        let view = renderer.finish(None);
        view.hyperlinks.to_vec()
    }

    fn line_to_string(line: &Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn plain_url_in_prose_produces_target() {
        let text = "See https://example.com for details.\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        assert_eq!(hyperlinks.len(), 1, "exactly one hyperlink expected");
        let h = &hyperlinks[0];
        assert_eq!(h.url, "https://example.com");

        // Verify column range covers only the URL
        let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
        renderer.push_and_render(text, None);
        let view = renderer.finish(None);
        let rendered = line_to_string(&view.lines[h.line_index]);
        let slice: String = rendered
            .chars()
            .skip(h.column_range.start)
            .take(h.column_range.len())
            .collect();
        assert_eq!(slice, "https://example.com");
    }

    #[test]
    fn multiple_urls_one_line_distinct_ids() {
        let text = "See https://a.example and https://b.example.\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        assert_eq!(hyperlinks.len(), 2, "two hyperlinks expected");
        assert_ne!(hyperlinks[0].id, hyperlinks[1].id, "ids must differ");
        assert_eq!(hyperlinks[0].url, "https://a.example");
        assert_eq!(hyperlinks[1].url, "https://b.example");
        // Column ranges must be disjoint
        assert!(
            hyperlinks[0].column_range.end <= hyperlinks[1].column_range.start,
            "column ranges must be disjoint, got {:?} vs {:?}",
            hyperlinks[0].column_range,
            hyperlinks[1].column_range,
        );
    }

    #[test]
    fn markdown_link_with_url_text_does_not_double_link() {
        let text = "Visit [https://example.com](https://example.com).\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        // NOTE: one might expect 1 target, but in pretty mode
        // `[url](url)` renders as `url (url)`, showing the URL in two
        // distinct positions: once as the link text (covered by the parser's
        // HyperlinkTarget) and once in the `(url)` suffix at a disjoint
        // column range. Dedup prevents a *third* entry at the same column
        // range as the parser-produced target; the second entry (at the
        // suffix position) is correctly detected as a separate target.
        assert_eq!(
            hyperlinks.len(),
            2,
            "expected 2 hyperlinks (parser link text + URL in pretty-mode suffix), got {}",
            hyperlinks.len()
        );
        // Both should reference the same URL.
        assert!(hyperlinks.iter().all(|h| h.url == "https://example.com"));
        // Column ranges must be disjoint (dedup working correctly).
        assert!(
            hyperlinks[0].column_range.end <= hyperlinks[1].column_range.start
                || hyperlinks[1].column_range.end <= hyperlinks[0].column_range.start,
            "column ranges must be disjoint, got {:?} and {:?}",
            hyperlinks[0].column_range,
            hyperlinks[1].column_range,
        );
    }

    #[test]
    fn autolink_does_not_double_link() {
        let text = "Visit <https://example.com>.\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        // All entries should share the same URL — autolinks may produce
        // multiple HyperlinkTarget fragments sharing the same id.
        let autolink_count = hyperlinks
            .iter()
            .filter(|h| h.url == "https://example.com")
            .count();
        assert!(autolink_count >= 1, "expected at least one autolink target");
        // The total count should match the autolink fragments only — no
        // extra plain-URL duplicates.
        assert_eq!(
            hyperlinks.len(),
            autolink_count,
            "plain-URL scan should not add duplicates on top of autolink targets"
        );
    }

    #[test]
    fn trailing_period_excluded_from_url() {
        let text = "See https://example.com.\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        assert_eq!(hyperlinks.len(), 1);
        assert_eq!(
            hyperlinks[0].url, "https://example.com",
            "trailing dot should be excluded by linkify"
        );
    }

    #[test]
    fn cjk_neighbors_preserve_correct_columns() {
        use crate::buffers::unicode_display_width;

        let text = "日本語 https://example.com 日本語\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        assert_eq!(hyperlinks.len(), 1);
        let h = &hyperlinks[0];
        assert_eq!(h.url, "https://example.com");

        // "日本語 " has 3 CJK chars (2 cells each) + 1 space = 7 display cells
        let prefix = "日本語 ";
        let expected_start = unicode_display_width(prefix);
        assert_eq!(expected_start, 7, "prefix should be 7 display cells");
        assert_eq!(h.column_range.start, expected_start);

        let url_width = unicode_display_width("https://example.com");
        assert_eq!(h.column_range.end, expected_start + url_width);
    }

    #[test]
    fn empty_document_returns_empty() {
        let hyperlinks = finish_and_get_hyperlinks("");
        assert!(
            hyperlinks.is_empty(),
            "empty document should produce no hyperlinks"
        );
    }

    /// Behavior pin: URL inside inline code.
    ///
    /// This test pins the *current* behavior — if linkify matches inside
    /// the code-styled span, the URL becomes a HyperlinkTarget. If we
    /// later decide to skip code-styled spans, this test will fail loudly
    /// and force the change to be intentional.
    #[test]
    fn url_inside_inline_code_documented_behavior() {
        let text = "Use `https://example.com` carefully.\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        // Pin observed behavior: linkify finds the URL inside the
        // code-styled span, producing a HyperlinkTarget.
        assert!(
            !hyperlinks.is_empty(),
            "behavior pin: URL inside inline code currently produces a HyperlinkTarget"
        );
        assert_eq!(hyperlinks[0].url, "https://example.com");
    }

    /// Behavior pin: URL inside a fenced code block.
    ///
    /// Same rationale as `url_inside_inline_code_documented_behavior` —
    /// this pins current behavior, not product spec.
    #[test]
    fn url_inside_code_fence_documented_behavior() {
        let text = "```\nsee https://example.com\n```\n";
        let hyperlinks = finish_and_get_hyperlinks(text);

        // Pin observed behavior: linkify finds the URL in the code block's
        // rendered text. Whether this is desirable is a product decision;
        // this test ensures any change is intentional.
        let has_url = hyperlinks.iter().any(|h| h.url == "https://example.com");
        assert!(
            has_url,
            "behavior pin: URL inside fenced code block currently produces a HyperlinkTarget"
        );
    }

    /// URL detection must run from `render()` too, not only `finish()`.
    /// Otherwise width changes or other state resets (e.g. via
    /// `set_max_table_width`) drop the URL hyperlinks that pretty-mode
    /// rendering adds for the `(url)` suffix of markdown links.
    ///
    /// Also pins the OSC 8 grouping invariant: the link-text and URL
    /// hyperlinks must have DISTINCT ids and DISJOINT column ranges, so
    /// terminals group them as two separate hyperlinks (not one merged
    /// underline across the brackets).
    #[test]
    fn render_detects_pretty_mode_url_suffix() {
        let text = "[link](https://example.com/some/long/path)\n";
        let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
        renderer.push_and_render(text, None);
        // No finish() call: after render() alone, both the parser-produced
        // (link text) and the url_scan-produced (URL in `(url)` suffix)
        // hyperlinks must be present.
        let view = renderer.view();
        assert_eq!(
            view.hyperlinks.len(),
            2,
            "render() must produce both the link-text and URL-suffix hyperlinks; \
             got {:?}",
            view.hyperlinks,
        );
        assert!(
            view.hyperlinks
                .iter()
                .all(|h| h.url == "https://example.com/some/long/path")
        );
        assert_ne!(
            view.hyperlinks[0].id, view.hyperlinks[1].id,
            "link-text and URL-suffix hyperlinks must have distinct OSC 8 ids",
        );
        let (a, b) = (&view.hyperlinks[0], &view.hyperlinks[1]);
        assert!(
            a.column_range.end <= b.column_range.start
                || b.column_range.end <= a.column_range.start,
            "column ranges must be disjoint, got {:?} and {:?}",
            a.column_range,
            b.column_range,
        );
    }

    /// Snapshot helper used by survival tests below.
    fn snapshot(view: &crate::output::MarkdownRenderView<'_>) -> Vec<HyperlinkTarget> {
        let mut snap: Vec<HyperlinkTarget> = view.hyperlinks.to_vec();
        snap.sort_by_key(|h| (h.line_index, h.column_range.start));
        snap
    }

    fn assert_url_suffix_preserved(
        before: &[HyperlinkTarget],
        after: &[HyperlinkTarget],
        url: &str,
    ) {
        let before_suffix = before
            .iter()
            .find(|h| h.url == url && h.column_range.start > 5)
            .expect("URL-suffix hyperlink must be present BEFORE reset");
        let after_suffix = after
            .iter()
            .find(|h| h.url == url && h.column_range.start > 5)
            .expect("URL-suffix hyperlink must be present AFTER reset");
        assert_eq!(
            before_suffix.column_range, after_suffix.column_range,
            "URL-suffix column range must be stable across the reset",
        );
        assert_eq!(
            before_suffix.line_index, after_suffix.line_index,
            "URL-suffix line index must be stable across the reset",
        );
    }

    /// After `finish()`, re-rendering (e.g. triggered by a width change
    /// via `set_max_table_width`) must NOT drop the URL hyperlinks that
    /// pretty-mode adds for the `(url)` suffix.
    ///
    /// Snapshots the full hyperlink list before/after the reset and
    /// asserts that the URL-suffix entry survives with its column range
    /// intact (the OSC 8 id may be re-assigned by the post-reset
    /// re-render — that's expected — but the location must not move).
    #[test]
    fn url_hyperlinks_survive_re_render_after_finish() {
        let url = "https://example.com/some/long/path";
        let text = format!("[link]({url})\n");
        let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
        renderer.push_and_render(&text, None);
        renderer.finish(None);
        let before = snapshot(&renderer.view());

        // Simulate a width change which resets renderer state.
        renderer.set_max_table_width(Some(40));
        renderer.render(None);
        let after = snapshot(&renderer.view());

        assert_eq!(
            before.len(),
            after.len(),
            "hyperlink count must be stable across the reset; before={before:?} after={after:?}",
        );
        assert_url_suffix_preserved(&before, &after, url);
    }

    /// Identical contract to `url_hyperlinks_survive_re_render_after_finish`
    /// but exercising the `set_pretty` reset path (production:
    /// `MarkdownContent::set_raw_mode` toggle).
    #[test]
    fn url_hyperlinks_survive_re_render_after_set_pretty_toggle() {
        let url = "https://example.com/some/long/path";
        let text = format!("[link]({url})\n");
        let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
        renderer.push_and_render(&text, None);
        renderer.finish(None);
        let before = snapshot(&renderer.view());

        // Toggle pretty off then back on — both transitions reset state.
        renderer.set_pretty(false);
        renderer.set_pretty(true);
        renderer.render(None);
        let after = snapshot(&renderer.view());

        assert_eq!(
            before.len(),
            after.len(),
            "hyperlink count must be stable across the set_pretty toggle",
        );
        assert_url_suffix_preserved(&before, &after, url);
    }

    /// Identical contract to `url_hyperlinks_survive_re_render_after_finish`
    /// but exercising the `set_style` reset path (production: theme change
    /// via `MarkdownContent::ensure_wrapped` when theme cache kind shifts).
    #[test]
    fn url_hyperlinks_survive_re_render_after_set_style() {
        let url = "https://example.com/some/long/path";
        let text = format!("[link]({url})\n");
        let mut renderer = StreamingMarkdownRenderer::new(test_style::STYLE, true);
        renderer.push_and_render(&text, None);
        renderer.finish(None);
        let before = snapshot(&renderer.view());

        // `set_style` unconditionally resets state, even with same style.
        renderer.set_style(test_style::STYLE);
        renderer.render(None);
        let after = snapshot(&renderer.view());

        assert_eq!(
            before.len(),
            after.len(),
            "hyperlink count must be stable across the set_style reset",
        );
        assert_url_suffix_preserved(&before, &after, url);
    }
}
