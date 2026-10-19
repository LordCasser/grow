# Verification evidence

## Current verification — 2026-10-19

Pager library run `/tmp/grow-v2-4-0-pager31.log` completed with **7495 passed, 0 failed, 12 ignored**. It includes both focused cases:

- `app::agent_view::turn_completion::tests::viewer_finalize_stop_reason_to_marker_mapping ... ok` covers the success allowlist (`end_turn`, `max_tokens`, `max_turn_requests`, `refusal`), existing cancelled/error/rate-limit handling, and failure classification for interrupted, unknown, and missing reasons.
- `app::agent_view::turn_completion::tests::viewer_finalize_duplicate_terminal_is_noop ... ok` covers first-wins behavior and prevents duplicate terminal markers.

Strict OpenSpec validation `/tmp/grow-v2-4-0-openspec-all29.log` reports **31 passed, 0 failed**, including this change. The scoped implementation, tests, and recorded validation are complete; this does not claim the broader release suite is complete.

The focused tests in `crates/codegen/pager/src/app/agent_view/turn_completion/tests.rs` are:

- `viewer_finalize_stop_reason_to_marker_mapping` covers the success allowlist (`end_turn`, `max_tokens`, `max_turn_requests`, `refusal`), existing cancelled/error/rate-limit handling, and failure classification for interrupted, unknown, and missing reasons.
- `viewer_finalize_duplicate_terminal_is_noop` covers first-wins behavior and prevents a duplicate terminal from adding a second marker.

No additional scenario is inferred beyond the two named tests above.
