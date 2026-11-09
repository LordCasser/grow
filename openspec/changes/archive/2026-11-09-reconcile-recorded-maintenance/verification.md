# Maintenance verification

## ACP prompt expression cleanup

The `acp_agent.rs` source in integrated tree `c9e8840b` and final validated tree `ed2027c0` matches Git blob `f5c8fc976199f2b51a4c6e3b46b3118346087f5f`. The production change removes only the never-repeating outer loop; the biased completion-first selection, durable ACK requirement for Working, receiver handling and subsequent lifecycle branches remain unchanged.

`cargo clippy --locked -p shell --lib --tests --no-deps` completed with **exit 0**, in 11 minutes 41 seconds. The former `clippy::never_loop` error is absent. This is a compilation/lint result, not execution of the prompt regression.

Warnings remain: the log reports 9 normal-library warnings and 51 library-test warnings including 8 duplicates, plus integration-test warnings. One warning refers to the newly added Workflow fixture (`session/workflow/test_support.rs:132`, `unnecessary_to_owned`); therefore this record does not describe every warning as pre-existing or claim warning-free validation.

## Final real prompt runtime gate

The standalone `grow` CLI built successfully from integrated source `ed2027c0d4dab5a517f97971af5fee084ae244e0`, tree `bf0f734ad420b8fbe9d30a7d053ca9678ffeee22`, with **exit 0** after 8 minutes 35 seconds. The measured binary SHA-256 is `282b8118813e523ccb3d50c7f22cd0977fda291c4e920bbdae3aff164bd09884`.

With `GROW_BINARY` pointing to that actual build, the following exact test completed successfully:

```sh
cargo test --locked --config 'profile.test.package.shell.codegen-units=512' \
  -p shell --test test_input_admission_replay_e2e \
  test_prompt_admission_replay_real_acp_entry \
  -- --ignored --exact --test-threads=1 --nocapture
```

Result: **1 passed, 0 failed, 0 ignored, 0 filtered out**, exit 0; test runtime 1.35 seconds after a 2.85-second test build. The test drives the real ACP entry and canonical input admission/replay path. It verifies in-flight and post-completion replay identity, unchanged canonical submissions/hooks/turns, no additional inference request and no spurious Working/Idle roster deltas; the original foreground remains live and completes normally. The compile-time codegen-unit override changes build partitioning, not assertions or runtime admission policy.

The test build retains one `unused_assignments` warning for `timeline_path` in its existing fixture. This does not weaken the passing result or support a warning-free claim. The result closes this maintenance change's focused runtime gate, not all repository or platform testing.

Earlier full Shell code generation ended with external exit 137 twice, once with four build jobs and the default linker, once with one job and lld. No kernel/cgroup OOM evidence was available, so memory exhaustion is plausible but unproven. Those attempts remain unsuccessful builds, not runtime assertion failures or passed suites. The later actual CLI build and executed regression above are the final acceptance evidence.

## Documentation

- Reconciled the old readiness paragraph with the already-archived repair and its original verification limits; no new readiness implementation is claimed.
- Restored the deferred ACP proposal to baseline as well as its source/tasks. Its existing preparation-link defect remains outside this delivery; no correction to that deferred document is claimed.
- The ACP experiment remains explicitly deferred. Its premature test/checklist additions were removed without execution; no ACP measurement or capacity result is claimed.
- [Disposition](disposition.md) preserves the missing scene-specific evidence for macOS watcher retirement, old-host delivery recovery, actual target Chat acceptance, external API consumers and the original blocking Q&A report.

Formatting, local links and OpenSpec validation are recorded in the checklist. Whole-repository CI is tracked separately by the integration owner; this maintenance record does not imply successful publication, merge or all-platform execution.

Pre-archive documentation checks on the isolated worktree: OpenSpec strict **23/23 passed**, all touched relative links resolved, and `git diff --check` passed. The production file had already passed direct rustfmt and workspace `cargo fmt --all -- --check`; this follow-up changes documentation only.


## Final archive

`openspec archive reconcile-recorded-maintenance --yes --skip-specs` confirmed all intended maintenance tasks complete and archived this change as `2026-11-09-reconcile-recorded-maintenance`. Relative links in the disposition were repaired for the new location. No main behavior specification was changed.

Post-archive checks passed: all strict **22/22**, archived strict **925/925**, all archived-maintenance links and `git diff --check`. A direct diff against the original `8fbc5218` baseline confirms the entire deferred `measure-acp-gateway-backlog` directory and `acp-transport/src/gateway.rs` remain unchanged, including the old preparation-link issue. This closes the prompt expression cleanup and documentation maintenance only; the separately listed evidence gates and measured Markdown copying debt are not marked repaired.
