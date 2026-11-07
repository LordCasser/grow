# Shell committed-image consumer verification

## Status and scope

Implementation source is ready for parent/exit-owner recompilation and review after the fixture correction below. This owner did not run Cargo, reconstruct another large history, or run ACP/CLI. The parent-run first targeted batch compiled successfully and reported 7 passed / 1 failed; the failure was an invalid ContextRebuild fixture before cleanup started, not an image-maintenance behavior failure. The parent confirmed the dynamic premise and authorized application after observing the real blocking stack `input_admission -> image_describe -> read_timeline -> scan/decode` while the exact reconciliation-worker join remained unfinished. The original B run returned unconfirmed at about 5002 ms. These runtime observations belong to the parent, not to this owner's local checks.

This increment changes only `session/actor/input_admission.rs::reconcile_user_image_assets`, `session/image_describe.rs` asset reconciliation and their inline tests. The actor file had substantial pre-existing dirty changes; those were preserved. ChatState, teardown, runtime_handoff, Cargo manifests and lockfile were not modified by this owner.

## Implemented behavior

- The existing artifact gate covers the live `committed_user_image_paths()` query through completion of the blocking contained-directory sweep. The actual actor session-directory capability is cloned for the sweep; existing writer ownership and exact service retirement remain intact.
- Authority failure returns before deletion. Cancellation can stop gate/query waiting, is rechecked after the query, and is checked while mapping paths and before each asset batch. Once blocking deletion is admitted, the actor awaits its actual JoinHandle without a cancellation branch that could detach it.
- Shell still interprets `image_batch_name`, published/staging batch grammar and contained-directory removal. The canonical path set is not treated as a full Timeline or as Shell grammar.
- No-assets is an ordinary no-op. Assets-present cleanup also avoids disk Timeline read/replay.
- The former disk-replay helper had no production caller except this actor, so it was replaced rather than retained as an unused compatibility path. The event-walking image-root extraction moved to the separate canonical provider owner. The old maintenance missing/broken-disk cases are replaced by a real owner-exit failure case that preserves published and staging assets. Unknown cold-source validation remains with the existing load owner; its code was not changed.

## Source-bound checks actually performed

Passed:

```text
rustfmt --edition 2024 --config skip_children=true --check crates/codegen/shell/src/session/actor/input_admission.rs crates/codegen/shell/src/session/image_describe.rs
git diff --check -- crates/codegen/shell/src/session/actor/input_admission.rs crates/codegen/shell/src/session/image_describe.rs
```

Call-site search confirms no remaining `committed_image_batches` or live image-maintenance `read_timeline_from_directory` / `Timeline::from_events` use in these two files. The actor method is also used by failed image admission; that caller automatically receives the same owner-authority behavior. No caller required another public API or state entity.

The full pre-existing actor dirty diff was reconstructed from the pre-edit inspection output and compared with the final file after removing this increment's newly inserted test block. It matched exactly, including the already-applied reconciliation-method hunk; formatting did not alter surrounding user changes.

Frozen source SHA-256 at handoff:

| File | SHA-256 |
| --- | --- |
| `crates/codegen/shell/src/session/actor/input_admission.rs` | `854bb95f5af0872283d3a024eb8e688c6174dddd35cca6073a9141431e48a93a` |
| `crates/codegen/shell/src/session/image_describe.rs` | `09eed434a710395f608d006bf29d230a51cfd5254fceb2af5da4f1edff412c11` |
| `crates/codegen/chat-state/src/handle.rs` (observed provider interface; not edited) | `52604317db500333918b0fa4493068de043f5788affe7886d234a88887f13b97` |

## Tests released to the serialized Cargo window

The first parent-run batch compiled and ran all 8 tests: 7 passed, and `retains_current_and_historical_owner_refs` failed at the original `input_admission.rs:920` with `Invalid(InvalidMessageShape)`. Raw evidence: `/Users/lordcasser/.grow/sessions/%2FUsers%2Flordcasser%2Fworkspace%2Fprojects%2Fgrow/01a1b4a2-108e-72e2-85f5-199a572526f4/terminal/call_e8599beb4253475d9ae45732b031083c.log`. The failed fixture omitted the existing stable System head from a full ContextRebuild replacement; canonical validation correctly rejected it before image cleanup was reached. This is retained as a fixture failure, not a behavior red.

The correction reads the actual Surface and revision together, retains its original System head, and replaces the remaining context with UserMeta. This follows `replacement_preserves_system_head` and existing pre-turn ContextRebuild behavior without changing production validation. Historical/current references, publication-gate competition, orphan/staging/unknown controls, disk no-replay evidence and pending-commit cancellation assertions are unchanged. Only this one test hunk changed; the corrected source has passed the same rustfmt and diff checks, but has not yet been compiled or executed.

Unified filter: `user_image_asset_reconciliation` (8 tests on Unix; the symlink test is Unix-only).

| Module / test suffix after `user_image_asset_reconciliation_` | Intended evidence |
| --- | --- |
| `session::actor::input_admission::tests::retains_current_and_historical_owner_refs` | Real canonical commit followed by Surface replacement retains historical assets; publication competing under the existing gate retains a later current batch; orphan/staging removed; unknown directory retained; invalid on-disk Timeline bytes unchanged, proving assets-present maintenance uses the live owner. |
| `session::actor::input_admission::tests::preserves_assets_when_owner_exits` | A real ChatState actor is first queried successfully, then cancelled; failed authority preserves both orphan published assets and staging. |
| `session::actor::input_admission::tests::cancellation_stops_before_authority_query` | Cancellation while waiting for the publication gate, and already-cancelled entry, preserve assets and leave the canonical owner usable. |
| `session::actor::input_admission::tests::cancellation_keeps_pending_owner_write` | A real ChatState owner has a manually delayed durable ACK; cancelling queued cleanup returns without deletion, then the original ACK completes its accepted write and the owner remains usable. |
| `session::image_describe::tests::uses_committed_paths_and_shell_grammar` | Retained batch kept, orphan and staging removed, invalid index/bare batch path do not grant retention, unknown names preserved, no Timeline file needed. |
| `session::image_describe::tests::without_assets_needs_no_timeline` | Missing assets and missing Timeline remain a no-op. |
| `session::image_describe::tests::stops_between_batch_deletions` | Deterministic cancellation allows the first accepted deletion to finish and preserves the remaining two batches. |
| `session::image_describe::tests::preserves_symlink_targets` | Symlinked assets authority rejected; unknown symlink preserved; recognized orphan symlink removed without touching its external target. |

Reuse the existing `session::image_describe::tests::` cases for publication, retry, rollback and symlinked-directory controls. Provider-owner coverage for Messages/Input::Consumed/Notification::Consumed and compaction/rewind is the separate ChatState delivery.

Existing cold load counterexamples were inspected, not executed by this owner: `session::storage::jsonl::tests::load_rejects_a_published_session_without_timeline` and `session::storage::jsonl::tests::malformed_timeline_control_bricks_session_load`. They continue to reject missing/invalid cold Timeline evidence through the real load API. No cold validation was removed or weakened in this increment.

## Remaining evidence and review

Parent/exit owner owns the next Shell compilation/test window, actual B ACP rerun, unchanged five-second deadline and durable checkpoint/writer-retirement/prefix/cold assertions, followed by CLI validation. This owner claims no latency improvement or passing execution of the corrected fixture before those results; the initial compiled 7-pass/1-fixture-failure evidence remains recorded above. Independent review and full OpenSpec validation/archive remain with the parent under the existing change plan.
