# Verification

## Implementation
AppView::draw now brackets the existing minimal draw hook with enabled-only Instant timing and feeds the existing HUD. The minimal API exposes owned overlay parameters and the shared reservation policy. compute_target reserves two rows from terminal capacity before invoking unchanged content sizing; enabled content is floored at three rows. draw_live clears the full viewport, paints the top panel, and passes the returned smaller content Rect through startup, panel, modal, tail and prompt branches. The existing viewport shrink/clear path is reused when disabled.

FpsOverlay::minimal_rows is the shared threshold (at least five available rows). No simulation/animation/UI timer was added. Old nonexistent FrameMetrics overlay claims and the always-true environment gate indirection were corrected; GROW_FPS behavior remains unchanged.

## Tests and checks
- CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib fps_hud --quiet: 8 passed.
- Same environment, cargo test --locked --offline -p pager-minimal --lib --quiet: 86 passed. All current minimal unit tests passed, including existing viewport/content, overlay and transcript tests.
- New Buffer regression uses nonzero origin and heights 0 through 8, checks returned content geometry, visible fps:100 for sufficient height, and sentinel cells across all content rows remain unchanged. Disable/enable and sample-bound tests remain.
- New timing-hook and viewport-wrapper wiring were inspected directly; unit tests do not claim a real terminal resize or cursor capture.
- git diff --check passed. OpenSpec all strict before archive: 16 passed. Existing macOS compact-unwind-size linker warning remains.
- target 13 GiB; disk available 64 GiB.

## Limits
No PTY end-to-end session, screenshot or CLI relink ran. Samples measure synchronous hook/draw work including writer handoff, not completed PTY output or terminal paint rate. Existing demand-driven frames remain; idle diagnostics do not refresh without a draw. Tiny viewports hide HUD to protect content. Existing fixed-width HUD formatting is unchanged.
