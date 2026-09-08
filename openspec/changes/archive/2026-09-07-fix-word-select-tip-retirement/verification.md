# Verification

## Observed failure
The diagnostic assertion printed `key=Some("clipboard_image_tip"), prompt="a", snapshot=None`. The word-select tip had already retired, and its prompt snapshot was already cleared. The failure was the broader `!ephemeral_tip.is_active()` assertion after `maintain_ui` polled the host clipboard and displayed a different tip.

Source checks: root `maintain_ui` calls `maintain_ephemeral_tip_clocks` before `poll_clipboard_focus_tip`. `AgentView::maintain_ephemeral_tip` clears the matching word-select tip and snapshot when the prompt differs. The clipboard poll gate checks `contextual_hints.image_input` before accessing the pasteboard. The test now disables that gate and checks the word-select key specifically, retaining the before/after Ctrl+Y routing assertions and snapshot-is-none assertion.

## Validation
Diagnostic run before correction: 1 failed with the exact state above.

After correction: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib app::root:: --quiet` returned 0: 1270 passed, 0 failed, 5825 filtered out, 2.09 seconds. Existing macOS compact-unwind-size warning remains.

This is a test-only correction, not a fix to product tip retirement. The original diagnostic run did read the system clipboard through existing production polling; the corrected individual test prevents that access. Other root tests were not converted to a new fixture framework. No clipboard write, user configuration edit, installed CLI update or additional feature removal occurred.
