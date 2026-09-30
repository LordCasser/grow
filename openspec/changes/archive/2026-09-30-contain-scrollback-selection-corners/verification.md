# Verification

## Geometry and call-site audit

- `ScrollbackState::selection_corner_insets` uses cached `gap_after` at the visible boundaries of a selected or hovered range; zero-height hidden thinking is transparent. `ScrollbackPane` applies it to singleton and group selections and keeps viewport-edge corners inside the scrollback area.
- `render_entry_hover` applies the same range and viewport boundary rule. The dense tool-row buffer test checks both selected and hovered frames against neighboring header rows.
- Pushed, pinned, and SingleTurn sticky-header `SelectionBox` construction now keeps top corners inside the scrollback area when the header starts at its first row. The sticky-header buffer test checks that a preceding pane row is untouched.
- Agent view side-pane chrome for tasks, catalog, todo, and queue receives corner insets from actual adjacent pane rectangles. The compact-layout buffer test checks that status and catalog rows are untouched, and that a spaced layout retains outboard corners.
- A one-row frame with both corners inset uses junction glyphs. A closable frame paints its close control last so its visual position still matches the hit rectangle.
- All `SelectionBox::new` call sites under Pager scrollback and agent view were inspected; no caller remains without a spacing or viewport-edge decision.

## Checks

- `rustfmt --edition 2024 --check` on the five changed Rust files: passed.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed, 16 items.
- Pager library tests, single-threaded with three unrelated failing tests filtered: **7413 passed, 0 failed, 12 ignored, 3 filtered**.
- Unfiltered Pager library tests, including a single-threaded rerun, consistently failed in `fragmented_paste_merged_with_keys`, `esc_clears_drill_anchor_after_spaced_drill`, and `rows_contain_categories_and_settings_through_pr_14`. These tests exercise paste handling, prompt file-search state, and settings list contents rather than the changed frame geometry. A parallel run with those three filtered also had a transient inline-media pending-limit failure; the single-threaded scoped run passed.
