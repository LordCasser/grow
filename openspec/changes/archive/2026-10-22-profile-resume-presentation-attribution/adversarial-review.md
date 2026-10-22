# P0 adversarial review

Independent read-only review (Luna) plus primary-agent checks identified and resolved:

1. `source_bytes` sampled before final normalizer flush would miss pending delimiter bytes. Sample after `renderer.finish`.
2. A timer at the `handle_session_loaded` entry could emit a completion for deferred/missing-agent returns. Create it only after the reload deferral guard on the existing real completion path.
3. Legacy `end_batch` is an early proxy, not complete readiness. A separate test-only `GROW_PTY_END_BATCH_PROXY` handles pre-P0 executables without accidentally exporting an incompatible session format. Keep proxy labels separate from real finalize and stable prompt.
4. Whole-test frame totals contain post-ready typing. Retain complete pre-key traces plus a timestamp-clipped loading subset, including the completion event and excluding later equal-timestamp events.
5. PTY cast time starts inside harness construction; visible observations start before spawn. Label cast as PTY receive clock and record the bounded origin offset; do not equate cast output to writer completion or user-visible pixels.
6. Derive body/notification counts from the actual modern typed projector before timing. Name Timeline+updates bytes explicitly; this is not a complete session directory measurement. Legacy dense tools are separately labelled and are not modern benchmark evidence.

Interpretation limits retained: log mode only (`timer()` has an existing Chrome-mode limitation; fixing diagnostics infrastructure is outside scope); disabled timers still take an Instant but do not allocate diagnostic fields; nested timers are not additive. ACP batch timing includes synchronous UI maintenance/effects as well as handlers. Markdown wrap timing includes renderer work and wrapping. Probe includes failed candidates. Finalize is synchronous reducer completion, not every subsequent async effect or stable pixels. Memory-release timer is latency, not a memory reduction assertion.

Formal timing/check results and the P1 decision are recorded after execution in verification.md.

Release-only validation mismatch: `insert_block_before_rejects_an_already_committed_anchor` unconditionally expects a panic from a `debug_assert!` (state/mod.rs:943,4066). Initial broad run retained: 1004 passed,1 failed,1 ignored. The unchanged production caller invariant is outside P0; explicitly exclude only this debug-only expectation in release, retain other insertion/commit-frontier/selection tests. This is not a claim the debug-only case ran or a change to runtime rejection semantics.

Independent data review reconciled all canonical/no-draw/PTY body counts and bytes and supported a P1 experiment. It confirmed no direct subtraction between different controls and no ready comparison for defaults-off or legacy end_batch. One terminology correction: ~92ms at2048 is outer `message.image_refs` source scanning, not actual image-file probes; the text fixtures have no resolved image refs. Per-run diagnostic on/off ranges overlap, so do not claim an identified observer-overhead magnitude or guaranteed penalty.
