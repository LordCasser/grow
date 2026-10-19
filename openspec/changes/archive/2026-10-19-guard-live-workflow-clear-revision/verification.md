# Verification evidence

## Current verification — 2026-10-19

Pager library run `/tmp/grow-v2-4-0-pager31.log` completed with **7495 passed, 0 failed, 12 ignored**. Both focused regressions passed:

- `app::agent_view::workflow_ingest::tests::stale_positive_clear_does_not_remove_newer_live_run ... ok` confirms an older positive-revision clear leaves the current run, revision, tombstone, and live block unchanged.
- `app::agent_view::workflow_ingest::tests::newer_clear_and_revisionless_compatibility_preserve_run_history ... ok` covers newer clears, equal-revision resurrection rejection, and revisionless clear/history compatibility.

Strict OpenSpec validation `/tmp/grow-v2-4-0-openspec-all29.log` reports **31 passed, 0 failed**, including this change. The scoped implementation, regression tests, and recorded validation are complete; this does not claim the broader release suite is complete.

The focused tests in `crates/codegen/pager/src/app/agent_view/workflow_ingest.rs` are:

- `stale_positive_clear_does_not_remove_newer_live_run` covers rejection of an older positive-revision clear without changing the current run, revision, tombstone, or live block.
- `newer_clear_and_revisionless_compatibility_preserve_run_history` covers accepting a newer clear, rejecting equal-revision resurrection, and preserving the revisionless clear and tombstone behavior alongside separate run history.

No additional scenario is inferred beyond the two named tests above.
