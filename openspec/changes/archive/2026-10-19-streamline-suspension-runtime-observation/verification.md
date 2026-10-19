# Verification

The exit path previously fetched `(runtime, events)` and discarded events for requested-root selection and retired-node state checks. The new runtime_state_before retains the existing deadline and live/cold authority paths while omitting that unused clone. Every child-fact snapshot and cold-history validation remains in place. Stage elapsed logs carry only session/operation identity and timing, never prompt, image or command content.

The two inspected histories contained approximately 8,128 / 5,958 events and 20.6 / 8.2 MB of JSONL. These sizes justify inspecting long-ledger costs but do not prove that a copy caused the observed multi-second stall.

An initial local synthetic fixture used a single 32 MiB Arc-backed message. Across ten calls it measured full-query 616 µs versus state-only 1502 µs; this did not establish a speedup and is not used to claim one. The fixture was corrected to represent 8,000 independent observation events with 4 KiB JSON payloads. The benchmark asserts equal runtime results and reports costs without a flaky timing threshold; its final results are recorded below. It is explicitly ignored in default CI and run as a local measurement, not a cross-platform SLA.

The existing release gate includes Shell/Pager/minimal default library tests. This change does not weaken evidence checks, add a cache, parallelize tree retirement or promise that native I/O can be forcibly interrupted. Remaining broad performance work requires measurements beyond this bounded optimization.

## Long-ledger result

Pinned Rust/Cargo 1.93.1, debug build on macOS arm64: 8,000 independent 4 KiB JSON observations, 33,990,891 serialized ledger bytes, ten queries. Old full-copy path: **69,833 µs total**; new state-only query: **392 µs total**, with identical runtime results. This isolates the removed query cost (about 6.98 ms to 0.039 ms per call), not whole-exit time. It does not explain a multi-second physical drain or promise a 178× faster application. Benchmark log: `/tmp/grow-suspension-observation-long-cost.log`. The fixture setup and measurement completed in 0.75 s.

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
