# Design

## Layout invariants

The optimization preserves each entry's measured or estimated height, `gap_after`, fold/group projection, and `virtual_y`; `total_height` continues to describe only `visible_entry_range()`, including the trailing gap. Width changes and structural mutations remain full-rebuild boundaries. Exact rendering remains limited to the viewport and warm pages. Follow behavior, scroll anchors, sticky prompt geometry, selection fixup, and AllTurns/SingleTurn navigation retain their current semantics.

## Maintenance strategy

`LayoutCache::virtual_y` is already a cumulative prefix over all cached entry heights and gaps. Derive the visible-range total from its first and last entry instead of summing the range on every clean frame. Empty ranges remain zero, and a non-empty range includes its final entry's `gap_after`.

When lazy measurement or streaming changes heights without changing structure, retain the old heights long enough to form `(entry_index, delta)` pairs. Sort those changes once, then patch virtual positions from the earliest changed index forward. Merge the sorted changes with the naturally ordered prompt descriptors: a prompt's y receives only deltas from preceding entries, and its `full_height` changes only when that prompt itself was measured. This avoids the current nested dirty-by-prompt search and handles multiple changes at different positions.

Do not introduce a new tree or prefix-sum data structure in this change. Full rebuild continues to own gap recomputation, group folding/truncation, and prompt descriptor reconstruction after structural or width invalidation.

## Baseline and verification

The ignored layout benchmark covers clean repeated `prepare_layout` calls and a height-only update near the beginning of histories with many prompts, at 128, 512, and 2048 entries. An independent exact renderer oracle checks every cached height, virtual offset, prompt descriptor position/height, and total height. The existing bounded replay benchmark remains the owning task's end-to-end replay measurement.

After the baseline is recorded, add differential coverage against a fresh full rebuild for multiple dirty prompts and entries, view-range changes, folded/group-truncated content, resize, follow mode, and preserved manual scroll anchors. Run the focused Pager layout tests and the full OpenSpec validation before marking tasks complete.

## Loading input and visible progress

User-approved loading behavior is exit-only: intercept ordinary keys, paste and mouse before view dispatch while the visible root/attached/child session is loading. Keep global Quit handling before that gate and retain resize. Preserve drafts that existed before loading; do not manufacture new queued input. Clear an armed quit confirmation on another key as in normal input. On load completion the gate disappears with the existing flag, and the final frame uses normal cadence.

Retain the existing 100 ms automatic paint cadence to avoid terminal throughput dominating replay. Consume history without intentional animation delays. Verify actual streaming markers as well as final history, ignored loading input and confirmed Ctrl+D; keep post-load input latency probes as a separate ready-state check.
