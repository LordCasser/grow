# Verification

## Evidence and scope

The reported stderr identifies `command activity admission is open while mailbox is serviced` in the session actor. Installed Grow is 2.4.0 (`cd5d88e`); the candidate at `dc7c02d8` retained the same assertion. The supplied session has a durable SuspendRequested at seq 8126 and CommandsSettled at 8127, but no completed checkpoint at inspection. This establishes incomplete suspension and the crash boundary; it does not establish that every preceding UI delay had the same cause. The live session was not controlled or replayed.

The fix uses the existing finite-activity tracker. Fourteen command branches handle closed admission instead of asserting it is open. AISuggest and SuggestPrompt return None; other finite RPCs keep their existing closed-responder semantics. Three configuration branches acquire their existing activity permit before mutations, so rejection has no partial side effects. The existing gate closes in request_node_suspension after operation checks and before spawning its persistence worker. Settlement, lifecycle queries and retirement stay on the same mailbox. No MCP feature, SDK, elicitation or ongoing MCP source changes are included.

## Red regression before production edits

Pinned Rust/Cargo 1.93.1, macOS arm64, isolated `/tmp/grow-suspension-admission-target`, current-thread runtime and real run_session mailbox:

`cargo test --locked -p shell --lib session::actor::tests::suspension_auxiliary_admission_tests -- --test-threads=1`

Both new tests failed as intended before production edits: late AISuggest reproduced the exact assertion panic and lost responder; immediate post-request admission was still open before the spawned worker ran. Log: `/tmp/grow-suspension-admission-red.log`.

## Additional failure found by deadline interleaving

After fixing admission, the same test exposed another failure: an existing owner drained after the five-second deadline, then a new checkpoint append used that already-expired deadline. ChatState correctly followed its existing strict writer-poison contract, so retry returned runtime authority unavailable. The Session now returns unconfirmed after physical owner drain, before dispatching checkpoint writes with an expired deadline. ChatState's exact-event, lost-ACK and writer-failure rules remain unchanged. The intermediate failing log is `/tmp/grow-suspension-admission-green.log`; it is not counted as green evidence.

The focused final two-test run passed, including real mailbox suggestions, live query, no Sideband attempt, original five-second caller expiry, retained owner, same-operation retry, matching checkpoint waiter and physical retirement. It performs no manual Idle reset. The early boundary test checks admission before the spawned suspension worker is first polled; it does not claim a separately fault-injected disk ACK test. Log: `/tmp/grow-suspension-admission-drain-green.log`.

## Why existing tests missed this

The prior direct suspension tests and normal PTY exit cases used a quiet auxiliary mailbox. They did not deliver AISuggest after closing admission while a physical owner still blocked the checkpoint. Persistence deadline tests separately verified writer failure, but did not combine a late physical drain with a healthy writer and fresh same-operation retry. Both invariants must hold in one real session loop.

## Post-fix validation

Broader results follow below.

A separately added already-expired request test also failed before its guard: the old admission spawned a worker which poisoned a healthy writer without ever dispatching its expired append. The final admission checks the remaining deadline after operation/runtime validation and before spawning that owner. Red log: `/tmp/grow-suspension-expired-admission-red.log`. This does not relax ChatState failure rules.

Final focused actor run: **3 passed, 0 failed** in 5.07 s, including expired admission, early gate closure, and the real mailbox/deadline/owner/checkpoint/retirement path. Log: `/tmp/grow-suspension-admission-final.log`.

## Broader local validation

Pinned Rust/Cargo 1.93.1 and isolated target on macOS arm64: the complete selected Shell/Tools library run passed **4,274 / 2,423 tests**, respectively, with no failures (6 / 7 ignored); the later three focused actor tests cover the final deadline guard and logs. Full Pager library passed **7,530 tests, 12 ignored, zero failures**; the final UI key/notice changes passed three focused regressions. Minimal passed **95 tests**, including shared notice viewport/painter coverage. CLI build passed with only the existing Pager dead-code warning. Logs: `/tmp/grow-suspension-shell-tools-final.log`, `/tmp/grow-suspension-pager.log`, `/tmp/grow-suspension-pager-final.log`, `/tmp/grow-suspension-minimal.log`, `/tmp/grow-suspension-cli-final-build.log`. Hosted Linux/Windows and publication were not run.

## Real process baseline

Using the just-built CLI as both GROW_BINARY and PAGER_BINARY, the exact Cargo runner verified four one-test passes: running-root command suspension/cold continuation without replay; root/child/grandchild tree suspension and cold restoration; normal frozen explicit Skill snapshot; and minimal frozen Skill snapshot. These are real independent Pager processes with mock provider fixtures. Log: `/tmp/grow-suspension-process-tests.log`. This baseline precedes only the final normal-frame overlay centralization; final rendering verification follows the UI change record.

## Final combined checks

The CLI was rebuilt from the final formatted renderer sources. The exact runner then passed both root/child/grandchild suspension with cold restoration and minimal frozen-Skill cold restoration against that executable. Together with the four earlier process cases, this verifies the shared runtime changes and final normal/minimal composition. Final logs: `/tmp/grow-suspension-render-cli-build.log`, `/tmp/grow-suspension-final-process-tests.log`. Full pinned formatting and diff whitespace checks passed; the selected documentation scan covered **20 Markdown files / 197 relative targets / zero missing**. Strict OpenSpec validation passed **20 active/main items** with zero failures. No hosted-platform or release-dist run is implied.

At the implementation-validation checkpoint, the work was isolated and uncommitted; the archive/integration follow-up is recorded below. Main's 34 pre-existing tracked modifications and MCP work were not changed. No live-session input/control, install, tag or publication was performed.

The task-owned Cargo target (about 10 GiB) was removed after process validation completed. Diagnostic logs and the small read-only process sample remain in /tmp; unrelated targets and user processes were left intact.

## Archive follow-up — 2026-10-19

The four completed changes were archived with OpenSpec 1.11.0. The two maintenance/test-only changes used --skip-specs; the behavioral changes updated model-sampling and client-surfaces. Current developer links now point to the main specifications or dated verification archives. The runtime sources were not changed during this finalization; the red/green, library, real-process and measurement evidence above remains the implementation evidence. Current release readiness is summarized in the sibling recover-unconfirmed-quit-input release-analysis.md.

### Finalization checks

After archiving, strict main/active OpenSpec validation passed **16 items** and strict archive validation passed **625 items**, with zero failures. The archive CLI's non-blocking proposal-header notices were addressed in the archived proposals; the two skip-specs changes still contain no invented behavioral delta. Pinned `cargo fmt --all -- --check`, `git diff --check`, and all **16** Python maintenance/release-runner tests passed. Locked Cargo metadata matched all **57** local package identities to Cargo.lock; the product version remains 2.4.0.

The final documentation scan covered **107 Markdown files / 583 relative targets / 329 anchors**, with zero missing targets or anchors. External links and 123 historical absolute local-file/diagnostic references were excluded from this relative-link check. Release/build shell syntax and six workflow YAML files parsed successfully; the extracted release static contract checker passed. This does not execute hosted workflows or the complete tag/version/clean-tree release preflight. Release readiness and final Git/MCP preservation evidence are in [the release review](../2026-10-19-recover-unconfirmed-quit-input/release-analysis.md).
