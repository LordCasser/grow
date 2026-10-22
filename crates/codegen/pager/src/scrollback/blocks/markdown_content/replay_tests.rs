use super::MarkdownContent;

fn chunks_at_char_width(source: &str, target_bytes: usize) -> Vec<&str> {
    if source.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    for (index, ch) in source.char_indices() {
        if bytes >= target_bytes && index > start {
            chunks.push(&source[start..index]);
            start = index;
            bytes = 0;
        }
        bytes += ch.len_utf8();
    }
    chunks.push(&source[start..]);
    chunks
}

fn deferred(source: &str, target_bytes: usize) -> MarkdownContent {
    let mut content = MarkdownContent::streaming();
    for chunk in chunks_at_char_width(source, target_bytes) {
        content.push_chunk_deferred(chunk);
    }
    content.finish();
    content
}

fn prewrap_text(content: &MarkdownContent) -> Vec<String> {
    content
        .pre_wrap_lines()
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect()
        })
        .collect()
}

fn link_snapshot(content: &MarkdownContent) -> (Vec<String>, Vec<markdown::HyperlinkTarget>) {
    content.with_link_content(|lines, links| {
        let lines = lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect();
        (lines, links.to_vec())
    })
}

fn wrapped_text(content: &MarkdownContent, width: usize) -> Vec<(String, Option<String>)> {
    content
        .output(width)
        .lines
        .into_iter()
        .map(|line| (line.content.to_string(), line.joiner))
        .collect()
}

fn assert_same_presentation(expected: &MarkdownContent, actual: &MarkdownContent) {
    assert_eq!(actual.text(), expected.text(), "normalized source");
    assert_eq!(actual.is_empty(), expected.is_empty());
    assert_eq!(actual.rendered_plain_text(), expected.rendered_plain_text());
    assert_eq!(prewrap_text(actual), prewrap_text(expected));
    assert_eq!(actual.line_source_map(), expected.line_source_map());
    assert_eq!(link_snapshot(actual), link_snapshot(expected));

    let expected_mermaid = expected.mermaid_content();
    let actual_mermaid = actual.mermaid_content();
    assert_eq!(actual_mermaid.len(), expected_mermaid.len());
    for index in 0..expected_mermaid.len() {
        assert_eq!(actual_mermaid.source(index), expected_mermaid.source(index));
    }
    for width in [17, 40, 80] {
        let expected_wrapped = wrapped_text(expected, width);
        let actual_wrapped = wrapped_text(actual, width);
        assert_eq!(
            actual.mermaid_block_ranges(),
            expected.mermaid_block_ranges(),
            "Mermaid line ranges at width {width}"
        );
        assert_eq!(
            actual_wrapped, expected_wrapped,
            "wrapped output at width {width}"
        );
    }
}

#[test]
fn deferred_replay_matches_complete_markdown_presentation() {
    let source = concat!(
        "# Café 🌱\r\n\r\n",
        "A tab\there, Unicode: 雪 and e\u{301}. A [link](https://example.test/a?x=1&y=2)\r\n",
        "and a plain https://example.test/plain URL.\r\n\r\n",
        "| Name | Value |\r\n| --- | ---: |\r\n| alpha | １２ |\r\n\r\n",
        "- first item\r\n  - nested item\r\n\r\n",
        "> quoted text\r\n> second line\r\n\r\n",
        "Inline math $x^2$ and \\(\\alpha + \\beta\\).\r\n\r\n",
        "```rust\r\nlet greeting = \"こんにちは\";\r\n```\r\n\r\n",
        "> ```MeRmAiD title=Nested\r\n",
        "> graph TD\r\n",
        ">   A-->B\r\n",
        "> ```\r\n\r\n",
        "```mermaid\nsequenceDiagram\n  A->>B: hello\n```\n",
    );
    let complete = MarkdownContent::new(source);

    // Exercise boundaries before and inside UTF-8 code points, table syntax,
    // link delimiters, CRLF pairs, math delimiters, and nested fences.
    for target_bytes in [1, 2, 3, 5, 8, 13, 21, 34] {
        let replayed = deferred(source, target_bytes);
        assert_same_presentation(&complete, &replayed);
    }
}

#[test]
fn deferred_finish_flushes_partial_math_delimiters_and_empty_input() {
    for source in [
        "A trailing dollar $",
        "A trailing slash \\",
        "Partial inline opener \\(x +",
        "Partial display opener \\[x + y",
        "Complete inline math \\(x + \\)",
        "Complete display math \\[x + y\\]",
        "",
    ] {
        let complete = MarkdownContent::new(source);
        for target_bytes in [1, 2, 4, 7] {
            let replayed = deferred(source, target_bytes);
            assert_same_presentation(&complete, &replayed);
        }
    }
}

#[test]
fn deferred_finish_is_repeatable_and_late_chunks_match_a_fresh_document() {
    let prefix = "Before $x$ and [site](https://example.test).\n\n```mermaid\ngraph LR\nA-->B\n```";
    let suffix = "\n\nAfter the first finish: 雪, `code`, and \\(\\gamma\\).";
    let mut replayed = deferred(prefix, 3);
    let once = MarkdownContent::new(prefix);
    assert_same_presentation(&once, &replayed);

    replayed.finish();
    assert_same_presentation(&once, &replayed);

    replayed.push_chunk_deferred(suffix);
    replayed.finish();
    let complete = MarkdownContent::new(format!("{prefix}{suffix}"));
    assert_same_presentation(&complete, &replayed);
}

#[test]
fn raw_mode_width_changes_and_clones_keep_deferred_content_independent() {
    let source = concat!(
        "## Clone check\n\n",
        "A long line with **bold**, _emphasis_, [a link](https://example.test), and 雪.\n\n",
        "| left | right |\n| --- | --- |\n| one | two |\n\n",
        "```Mermaid theme=dark\ngraph TD\nA-->B\n```\n",
    );
    let mut replayed = deferred(source, 5);
    let mut complete = MarkdownContent::new(source);
    assert_same_presentation(&complete, &replayed);

    let mut clone = replayed.clone();
    for raw in [true, false] {
        replayed.set_raw_mode(raw);
        complete.set_raw_mode(raw);
        clone.set_raw_mode(raw);
        assert_same_presentation(&complete, &replayed);
        assert_same_presentation(&complete, &clone);
    }

    clone.push_chunk_deferred("\n\nclone-only tail");
    clone.finish();
    assert_eq!(replayed.text(), complete.text());
    assert_ne!(clone.text(), replayed.text());
    assert_same_presentation(
        &MarkdownContent::new(format!("{source}\n\nclone-only tail")),
        &clone,
    );
}

#[test]
fn completed_replay_defers_presentation_until_a_derived_view_is_requested() {
    let replayed = deferred(
        "**bold** and [link](https://example.test)\n\ntrailing \\",
        4,
    );
    assert!(replayed.state.borrow().final_render_pending);
    assert!(replayed.state.borrow().renderer.view().lines.is_empty());
    assert!(replayed.text().ends_with('\\'));
    assert!(!replayed.is_empty());
    assert!(replayed.mermaid_content().is_empty());
    assert!(replayed.state.borrow().final_render_pending);
    replayed.with_link_content(|_, links| {
        assert!(!links.is_empty());
        // Nested source/derived readers must not reborrow mutably.
        assert!(!replayed.rendered_plain_text().is_empty());
    });
    assert!(!replayed.state.borrow().final_render_pending);
}

#[test]
fn first_wrapped_view_and_raw_toggle_materialize_correctly_without_prior_copy() {
    let source = "## title\n\n| a | b |\n| --- | --- |\n| long value | longer value |\n\n[link](https://example.test)";
    for raw in [false, true] {
        let mut replayed = deferred(source, 3);
        let mut complete = MarkdownContent::new(source);
        replayed.set_raw_mode(raw);
        complete.set_raw_mode(raw);
        assert!(replayed.state.borrow().final_render_pending);
        assert_eq!(wrapped_text(&replayed, 17), wrapped_text(&complete, 17));
        assert!(!replayed.state.borrow().final_render_pending);
        assert_same_presentation(&complete, &replayed);
    }
}

#[test]
fn pending_clone_and_late_live_append_keep_final_source_and_links() {
    let mut replayed = deferred("[one](https://one.test)\n\nfirst", 2);
    let clone = replayed.clone();
    assert_same_presentation(&MarkdownContent::new(replayed.text()), &clone);
    replayed.push_chunk(" and [two](https://two.test)\n\nlast \\");
    replayed.finish();
    assert_same_presentation(&MarkdownContent::new(replayed.text()), &replayed);
}

#[test]
fn encoded_mermaid_fence_info_is_detected_at_finish() {
    for source in [
        "```m&#101;rmaid\ngraph LR\nA-->B\n```",
        "```m&#x65;rmaid\ngraph LR\nA-->B\n```",
    ] {
        let complete = MarkdownContent::new(source);
        assert_eq!(complete.mermaid_content().len(), 1);
        let replayed = deferred(source, 3);
        assert_eq!(replayed.mermaid_content().len(), 1);
        assert!(!replayed.state.borrow().final_render_pending);
        assert_same_presentation(&complete, &replayed);
    }
}
