# Design

Use `session_load_perf::full_session_load_e2e` without modifying production code. Set `GROW_PERF_*` to the PTY fixture values. Compare its `session/load` round-trip and replay subphases to PTY resume-to-visible-history, noting that they run in different harnesses and their totals are not directly subtractable as an exact Pager duration. If Shell dominates, inspect replay; if it does not, investigate Pager frame/layout and terminal write phases in a separate change. Keep the backlog open until real transcript shapes and resident/cursor paths are sampled.
