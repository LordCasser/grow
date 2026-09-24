# Verification

The recorded `expect` sites are renderer precondition assertions, not a reachable stale-cache path in the current production draw pipeline:

- The sole production `ScrollbackPane::new()` call in `AgentView::draw` immediately follows `self.scrollback.prepare_layout(content.width, content.height)` on the same state. Between them, `rewind_dim_from_entry`, search-highlight lookup, and `ensure_media_link_paths` only read scrollback; there is no await or scrollback mutation.
- `render_with_scratch_and_selection_boundaries` returns before cache access for a zero-width or zero-height area. The playground call sites also call `prepare_layout` before rendering.
- `ensure_layout_cache` rebuilds when width or entry count changes. A rebuild pushes one entry layout and one `virtual_y` per scrollback entry. `visible_entry_range` is `0..len` or a turn range; removals rebuild turns even during a batch, while batched pushes only append. `compute_paint_window` partitions within that visible range and clamps group-run extension to its end, so the renderer's `all_layouts.get(paint_range)` stays in bounds.
- `CARGO_INCREMENTAL=0 cargo test --locked --offline -p pager --lib scrollback:: -- --quiet`: 981 passed.
- `git diff --check` and strict OpenSpec validation passed before archive.

This audit does not claim arbitrary external calls to `ScrollbackPane` are safe without `prepare_layout`; the public renderer deliberately asserts that caller precondition. No production trigger remains for the backlog item.
