# Design: Verify the scrollback layout precondition

## Audit method

Treat the `expect` sites as assertions of the renderer's prepare-before-read contract. Locate all production `ScrollbackPane` calls and prove whether they share the same `ScrollbackState` after `prepare_layout`. Check zero-sized frame handling, mutations between prepare and render, cache entry/virtual-y lengths, SingleTurn range maintenance during batching/removal, and `compute_paint_window` bounds. Existing targeted tests serve as dynamic coverage; this change does not weaken assertions or add a fallback that could display stale layout data.
