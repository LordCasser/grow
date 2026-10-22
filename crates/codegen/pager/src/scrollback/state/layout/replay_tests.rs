use super::*;
use pretty_assertions::assert_eq;

fn agent(chunks: &[&str], deferred: bool) -> RenderBlock {
    if !deferred {
        return RenderBlock::agent_message(chunks.concat());
    }
    let mut block = RenderBlock::agent_message_streaming();
    let message = block.as_agent_message_mut().expect("streaming agent block");
    for chunk in chunks {
        message.push_chunk_deferred(chunk);
    }
    message.finish();
    block
}

fn thinking(chunks: &[&str], deferred: bool) -> RenderBlock {
    if !deferred {
        return RenderBlock::thinking(chunks.concat());
    }
    let mut block = RenderBlock::thinking("");
    let thinking = block.as_thinking_mut().expect("thinking block");
    for chunk in chunks {
        thinking.push_chunk_deferred(chunk);
    }
    thinking.finish();
    block
}

fn rich_deferred_history() -> ScrollbackState {
    rich_history(true, false)
}

fn rich_history(deferred: bool, sticky: bool) -> ScrollbackState {
    let mut state = ScrollbackState::new();
    let mut appearance = crate::appearance::AppearanceConfig::default();
    appearance.show_timestamps = false;
    // Isolate the content anchor from a sticky prompt whose own row count
    // legitimately changes when its text re-wraps at the new width.
    appearance.scrollback.display.sticky_headers = sticky;
    state.set_appearance(appearance);
    state.begin_batch();
    for turn in 0..12 {
        state.push_block(RenderBlock::user_prompt(format!(
            "Review the historical result for turn {turn}"
        )));
        state.push_block(thinking(
            &[
                "I should compare the table and the Unicode marker ",
                "雪 before answering.\n\n",
                "The narrow layout will wrap this reasoning across rows.",
            ],
            deferred,
        ));
        state.push_block(agent(
            &[
                "## Result heading\n\n",
                "A longer paragraph with **emphasis**, Unicode 雪, and a ",
                "[reference link](https://example.test/history) that wraps at ",
                "narrow widths.\n\n",
                "- first list item with enough words to wrap\n",
                "  - nested item with more words\n\n",
                "| column | value |\n| --- | --- |\n| alpha | beta gamma delta |\n\n",
                "```rust\nfn historical_result() -> &'static str {\n    \"ready\"\n}\n```\n",
            ],
            deferred,
        ));
    }
    state.end_batch();
    state
}

fn assert_measured_heights_are_exact(state: &ScrollbackState, width: u16) {
    let cache = state.layout_cache.as_ref().expect("prepared layout");
    for (idx, entry) in cache.entries.iter().enumerate() {
        if cache.measured[idx] {
            assert_eq!(
                entry.height,
                exact_height(state, idx, width),
                "measured entry {idx} has its eager rendered height"
            );
        }
    }
}

#[test]
fn deferred_history_tail_is_exact_and_repeated_prepare_keeps_viewport_stable() {
    let _theme = pin_theme();
    let mut state = rich_deferred_history();
    let width = 34;
    let height = 11;

    state.prepare_layout(width, height);
    let first_range = state.visible_entry_range();
    let first_offset = state.scroll_offset();
    assert!(
        first_range.end == state.len(),
        "initial viewport is at history tail"
    );
    assert_measured_heights_are_exact(&state, width);
    assert!(
        measured_at(&state, state.len() - 1),
        "the last deferred agent message is measured in the tail viewport"
    );
    assert!(
        laid_out_count(&state) < state.len(),
        "a short tail viewport must not eagerly render the whole replay"
    );

    for _ in 0..3 {
        state.prepare_layout(width, height);
        assert_eq!(state.visible_entry_range(), first_range);
        assert_eq!(state.scroll_offset(), first_offset);
        assert_measured_heights_are_exact(&state, width);
    }
}

#[test]
fn deferred_markdown_navigation_resize_anchor_and_full_height_match_eager_oracle() {
    let _theme = pin_theme();
    let mut state = rich_deferred_history();
    let narrow = 28;
    let wide = 52;
    let height = 12;
    let anchor = 19;

    state.prepare_layout(narrow, height);
    state.scroll_to_entry_top(anchor);
    state.prepare_layout(narrow, height);
    let anchored_row = screen_row_of(&state, anchor);
    assert!(
        !state.is_follow_mode(),
        "explicit navigation leaves follow mode"
    );

    state.scroll_up(3);
    state.prepare_layout(narrow, height);
    let after_up = state.scroll_offset();
    assert!(
        after_up > 0,
        "scrolling up moves away from the history tail"
    );
    assert!(screen_row_of(&state, anchor) > anchored_row);

    state.scroll_down(3);
    state.prepare_layout(narrow, height);
    assert_eq!(screen_row_of(&state, anchor), anchored_row);

    // Pin the actual entry boundary, rather than a navigation target below
    // a preceding prompt whose own wrapped height may change on resize.
    let top = {
        let range = state.visible_entry_range();
        let cache = state.layout_cache.as_ref().unwrap();
        cache.virtual_y[anchor] - cache.virtual_y[range.start]
    };
    state.set_scroll_offset(top);
    state.prepare_layout(narrow, height);
    let before_resize_row = screen_row_of(&state, anchor);
    assert_eq!(before_resize_row, 0);
    state.prepare_layout(wide, height);
    assert_eq!(
        screen_row_of(&state, anchor),
        before_resize_row,
        "a width-only resize preserves the manually selected anchor row"
    );

    // A full-height prepare exact-renders every entry. Compare the resulting
    // total against independent EntryRenderer measurements at the same width.
    state.prepare_layout(wide, u16::MAX);
    let cache = state.layout_cache.as_ref().expect("full layout cache");
    assert!(cache.measured.iter().all(|measured| *measured));
    assert_eq!(
        state.total_height as u32,
        exact_total_oracle(&state, wide),
        "deferred Markdown history total equals the independent eager oracle"
    );
    for idx in 0..state.len() {
        assert_eq!(
            cache.entries[idx].height,
            exact_height(&state, idx, wide),
            "fully materialized entry {idx}"
        );
    }
}

#[test]
fn sticky_prompt_rewrap_matches_eager_content_anchors_and_rows() {
    let _theme = pin_theme();
    let mut replay = rich_history(true, true);
    let mut eager = rich_history(false, true);
    for state in [&mut replay, &mut eager] {
        state.prepare_layout(28, 12);
        state.scroll_to_entry_top(19);
        state.prepare_layout(28, 12);
    }
    for width in [28, 52, 23, 80, 120, 80, 28] {
        replay.prepare_layout(width, 12);
        eager.prepare_layout(width, 12);
        assert_eq!(
            replay.capture_scroll_anchor(),
            eager.capture_scroll_anchor()
        );
        assert_eq!(screen_row_of(&replay, 19), screen_row_of(&eager, 19));
        assert_eq!(replay.scroll_info(), eager.scroll_info());
        assert_measured_heights_are_exact(&replay, width);
    }
}
