# U7 · Follow-preserve coordinates across layout rebuilds

## Result and scope

Implementation and module verification are accepted by the parent after inspecting the final diff, effective old-red geometry, actual Pane Buffer regression, stable identity/removal control, no-render top-anchor control and final 250-pass logs. The original PTY owner's product acceptance remains pending. The fix is confined to `crates/codegen/pager/src/scrollback/state/{mod,layout,nav}.rs`, including three local regression tests. No input, sampling, settlement, advisory-card gesture, generic fold, sticky-rendering or PTY-test implementation changed. No commit or release was made. The pre-existing shared dirty worktree is retained; HEAD at verification is `8ab486a6e45870561b91dba872934a6e24a795f1`.

The production change reuses `ScrollAnchor`'s existing logical-line/sub-row mapping and gives its entry reference a stable `EntryId`. Entry-top anchors store no logical-line offset: capture and resolve use only the stable identity and current virtual-y, so ordinary prompt-top follow passes neither render the prompt nor collect its line mapping. Non-top anchors retain the original width-stable logical-line/sub-row mapping. Manual anchor restoration retains its `max_scroll_offset` clamp; the private offset resolver lets follow-preserve keep a legal prompt-top position above that maximum. A private transient `follow_anchor: Option<(ScrollAnchor, usize)>` retains the referent and its last resolved offset across cache invalidation. An explicit offset change, including existing fold anchoring, establishes a new anchor. The existing past-end retirement check runs before re-anchoring; actual overflow is evaluated afterward against the prompt's current position.

This transient referent is necessary because `remove_entry`, `mark_height_dirty`, appearance/width invalidation and other existing paths may discard the entire cache before `prepare_layout` can capture anything. It adds no public API, config, durable field, second transcript or pin manager. Preserve is armed at the existing `enable_follow_with_preserve`/follow entry, including callers without a prompt. An explicit bottom gesture clears it; fresh/cleared state and viewport-snapshot restoration discard unrelated transient identity. Prompt-less spacer positions that the manual content anchor cannot round-trip retain their previous absolute-offset policy. Existing fold code and its reading-intent decisions remain unchanged.

## Effective old red and green

`follow_pin_rebuild_keeps_prompt_and_first_reply_visible` uses `ScrollTestHarness`, the real completed collapsed `OtherToolCallBlock` carrying an `AdvisoryCard`, real literal `UserPromptBlock`/agent body entries, `remove_entry`, ordinary append, `prepare_layout`, and `ScrollbackPane` drawn through Ratatui `Terminal<TestBackend>`. No layout-cache vector is edited by the test. The completed card has output, so its real foldable estimate is one row while its collapsed exact output is two. The five-line source prompt uses the normal collapsed three content rows plus two padding rows, producing the observed exact height five and sticky-header height six.

Before the replacement, the test verifies the exact card height two, exact prompt height five, the prompt at viewport top and follow/preserve both enabled. A provisional reply row is then removed through the real API, causing full-cache invalidation, and the one-line real body is appended before the next frame. This is a minimal reproduction of the accepted invalidation mechanism; it does not identify the runtime caller that invalidated the original trace's cache.

The effective pre-fix run compiled and reached the final geometry/paint/Buffer assertion:

- Actual exit **101**, **0 passed / 1 failed**, 7625 filtered out.
- `/tmp/grow-follow-pin-oldred.log:7–8`: `prompt_y=10, offset=11, header=6, paint=6..6, body_idx=5, marker=false`.
- This matches the original diagnostic trace's one-row drift and first-body exclusion in `verification-scroll-state.md` and the raw RPC log at lines 414–430. The earlier two fixture-only failures (prompt padding, then absent completed tool output) are not counted as U7 red.

The initial minimal fix then passed the identical test: actual exit **0**, **1 passed / 0 failed**, 7625 filtered out, `/tmp/grow-follow-pin-green.log:5–8`. The final version additionally draws an inactive pane, checks the same prompt and body after the earlier card is measured back to two rows, and removes an earlier history entry so the prompt index shifts while its identity remains stable. Every phase checks prompt-top position, body paint inclusion, actual Buffer marker and retained follow/preserve. It passed in the final local regression run. `follow_without_prompt_preserves_a_gap_row` covers the prompt-less preserve spacer boundary without changing its existing scroll semantics. `follow_top_anchor_does_not_rebuild_evicted_render_output` evicts the real prompt's output, re-arms preserve and runs repeated normal layout frames; the output remains absent, proving that the ordinary top-pin path does not enter the renderer's logical-line mapping.

## Commands and final verification

All Cargo commands ran from the repository root with command-local removal of inherited `CARGO_TARGET_DIR`, `PATH="$HOME/.cargo/bin:$PATH"`, `RUST_MIN_STACK=16777216`, `CARGO_BUILD_JOBS=2`, Rust `+1.93.1`, `--locked --offline`. Each long command used background execution with no wrapper timeout, and its actual exit was collected. Toolchain identity: `rustc 1.93.1 (01f6ddf75 2026-02-11)`, `cargo 1.93.1 (083ac5135 2025-12-15)`.

The focused old-red and initial-green command was:

```sh
env -u CARGO_TARGET_DIR PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 test -p pager --lib --locked --offline \
  follow_pin_rebuild_keeps_prompt_and_first_reply_visible \
  -- --test-threads=1 --nocapture
```

The final regression command was:

```sh
env -u CARGO_TARGET_DIR PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 test -p pager --lib --locked --offline -- \
  scrollback::state::layout:: scrollback::state::nav:: \
  scrollback::state::selection:: scrollback::state::tests:: \
  scrollback::scrollback_pane::tests:: --test-threads=1 \
  > /tmp/grow-follow-pin-regress.log 2>&1
```

Actual exit **0**: **250 passed / 0 failed / 1 ignored / 7377 filtered out**, `/tmp/grow-follow-pin-regress.log:258`. The ignored item is the existing manual layout benchmark. The run covers layout/replay, navigation, selection/fold, state lifecycle and four adjacent pane tests, including original page-flip overflow, past-end shrink, explicit bottom gestures, manual folds, resize anchors, AllTurns/SingleTurn and viewport-snapshot controls. There was no whole-workspace or general renderer audit.

Final package check:

```sh
env -u CARGO_TARGET_DIR PATH="$HOME/.cargo/bin:$PATH" \
  RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 \
  cargo +1.93.1 check -p pager --lib --locked --offline \
  > /tmp/grow-follow-pin-check.log 2>&1
```

Actual exit **0**, `/tmp/grow-follow-pin-check.log:1–2`. This is a default production library check; it does not rebuild the CLI. Scoped `rustfmt +1.93.1 --edition 2024 --config skip_children=true --check` on the three modified source files and `git diff --check` on the state scope both completed with exit **0** and no diagnostics. `openspec validate --all --strict --no-interactive` completed with exit **0**, **22 passed / 0 failed**, `/tmp/grow-follow-pin-openspec.log:23`.

All owned Cargo commands have terminal results; none remains in flight. Cargo and the assigned source/document write scope are released for parent review and the next build owner.

## Remaining acceptance boundary

The original PTY owner must build a fresh uninstrumented CLI from the final source, then run the unchanged mouse late-reply main path: no Tab/PageDown/additional navigation, automatic body visibility, automatic Answered and cold read-only recovery; keyboard and Settings controls must use that same artifact. Unit/Pane verification does not replace that product acceptance.

`target/debug/grow` was not rebuilt by this task and remains the prior diagnostic artifact identified in `verification-scroll-state.md`; Cargo tests/check do not establish its identity as a final source artifact. No temporary production trace was added by this implementation. The precise upstream invalidation caller remains outside this fix's established evidence and scope.
