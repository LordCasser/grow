# Maintenance verification

## ACP prompt expression cleanup

The `acp_agent.rs` source in integrated tree `c9e8840b` matches Git blob `f5c8fc976199f2b51a4c6e3b46b3118346087f5f`. The production change removes only the never-repeating outer loop; the biased completion-first selection, durable ACK requirement for Working, receiver handling and subsequent lifecycle branches remain unchanged.

`cargo clippy --locked -p shell --lib --tests --no-deps` completed with **exit 0**, in 11 minutes 41 seconds. The former `clippy::never_loop` error is absent. This is a compilation/lint result, not execution of the prompt regression.

Warnings remain: the log reports 9 normal-library warnings and 51 library-test warnings including 8 duplicates, plus integration-test warnings. One warning refers to the newly added Workflow fixture (`session/workflow/test_support.rs:132`, `unnecessary_to_owned`); therefore this record does not describe every warning as pre-existing or claim warning-free validation.

## Pending runtime gate

The actual `test_prompt_admission_replay_real_acp_entry` regression in `shell --test test_input_admission_replay_e2e` requires the final CLI via `GROW_BINARY` and explicit `--ignored --exact` execution. It is **not yet reported as run** in this record. The final CLI/Shell integration build is still owned by integration validation.

Earlier full Shell code generation ended with external exit 137 twice, once with four build jobs and the default linker, once with one job and lld. No kernel/cgroup OOM evidence was available, so memory exhaustion is plausible but unproven. Neither attempt counts as a failed runtime assertion or a passed test suite. Clippy's success does not replace that pending runtime gate.

## Documentation

- Reconciled the old readiness paragraph with the already-archived repair and its original verification limits; no new readiness implementation is claimed.
- Restored the deferred ACP proposal to baseline as well as its source/tasks. Its existing preparation-link defect remains outside this delivery; no correction to that deferred document is claimed.
- The ACP experiment remains explicitly deferred. Its premature test/checklist additions were removed without execution; no ACP measurement or capacity result is claimed.
- [Disposition](disposition.md) preserves the missing scene-specific evidence for macOS watcher retirement, old-host delivery recovery, actual target Chat acceptance, external API consumers and the original blocking Q&A report.

Formatting, local links and OpenSpec validation are recorded in the checklist. Whole-repository CI is tracked separately by the integration owner; this maintenance record does not imply successful publication, merge or all-platform execution.

Final documentation checks on the isolated worktree: OpenSpec strict **23/23 passed**, all touched relative links resolved, and `git diff --check` passed. The production file had already passed direct rustfmt and workspace `cargo fmt --all -- --check`; this follow-up changes documentation only.
