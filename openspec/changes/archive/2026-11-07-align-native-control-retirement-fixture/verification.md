# Verification

## Scope and evidence

Pure test maintenance (`skip_specs: true`). Only `crates/codegen/shell/src/agent/mvp_agent/tests/session_model_tests.rs` and the existing tests in `crates/codegen/shell/src/session/actor/model_switch.rs` changed. Production shutdown, Result ordering, model control identity/sequence allocation, manifests, and shared support remain unchanged.

The existing red evidence is preserved in `/tmp/grow-subagent-control-child-regress-native-{finished-during-lock,mailbox-model-after-finish,mailbox-effort-after-finish}.log`. All three fail at the obsolete `finalization keeps the exact actor alive` premise before the intended rejection assertions. `/tmp/grow-subagent-control-child-oldred-finished.log` independently reproduces that same failure with the host-intent change withdrawn. The two mailbox cases have no independent withdrawn-change baseline; their evidence is limited to the current-source premise failure.

## Boundaries asserted

- `finished-during-lock` first observes the real Shell handler waiting for the held catalog lock, then releases the provider to finish naturally and waits for the exact original actor mailbox to close. After releasing the catalog lock it requires the exact Shell target-check error `the target subagent execution is no longer open`.
- `mailbox-model-after-finish` and `mailbox-effort-after-finish` prepare the respective native command through the existing `enqueue_tool` handler while the child is live. After natural retirement, the exact original mailbox must reject `send`; the demonstrably unsent command is dropped before awaiting its closed admission receipt and the exact application-receipt failure. Neither mailbox preparation nor receipt closure is treated as actor admission or queued.
- The separate actor fixture test `native_tool_sampling_rejects_model_and_effort_after_execution_closes` prepares both variants before the production `foreground.begin_settling()` transition, then invokes the real `admit_tool_sampling` entry. Both admission and application receipts must contain the precise execution-closed rejection, with no pending controls, no Pending projection, and no route/effort change.
- Each natural retirement integration case reads the formal JSONL Timeline after actor exit, asserting one canonical Result as the final fact, unchanged durable model/effort and transition count, no native control projection, unchanged parent sampling, and exactly one original provider attempt. The held legitimate parent usage command is forwarded and the real child finalization receipt must succeed.

## Execution

Every Cargo invocation uses:

```text
env -u CARGO_TARGET_DIR -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u RUSTC -u RUSTUP_TOOLCHAIN PATH="$HOME/.cargo/bin:$PATH" RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2 cargo +1.93.1 test -p shell --lib --locked --offline ...
```

Each direct native case uses a fresh disposable `GROW_HOME`; wrapper subprocesses retain the existing per-case temporary homes. No real user configuration is written.

The first compile attempt exited 101 due to two fixture-only compile errors (TimelineEvent has no PartialEq; AgentTask::abort is private). They were fixed with Result sequence comparison and existing task handle cleanup. That diagnostic is preserved in `/tmp/grow-native-retirement-compile-failure.log`; it is not product red evidence.

Final Rust source was formatted with `rustfmt +1.93.1 --edition 2024 --config skip_children=true` scoped to the two permitted files; `--check` and `git diff --check` passed (exit 0), recorded in `/tmp/grow-native-retirement-check.log`.

| Check | Result | Evidence |
| --- | --- | --- |
| `finished-during-lock` direct case | PASS: 1 passed, 0 failed; actual exit 0 | `/tmp/grow-native-retirement-green.log` |
| `mailbox-model-after-finish` direct case | PASS: 1 passed, 0 failed | `/tmp/grow-native-retirement-green-mailbox-model-after-finish.log` |
| `mailbox-effort-after-finish` direct case | PASS: 1 passed, 0 failed; both-mailbox serial command actual exit 0 | `/tmp/grow-native-retirement-green-mailbox-effort-after-finish.log` |

The direct cases use the exact filter `agent::mvp_agent::tests::session_model_tests::native_subagent_model_tool_controls_the_child_without_waiting_for_its_step -- --exact --nocapture`, with `GROW_TEST_SUBAGENT_MODEL_CASE` set to the named case.

The original native wrapper (all 21 cases), owner wrapper (all 10 cases), and new actor test ran serially against this stable final Rust source. The wrappers explicitly unset both case-selector environment variables and used these filters with `-- --exact --nocapture`:

```text
agent::mvp_agent::tests::session_model_tests::native_subagent_model_tool_controls_the_child_without_waiting_for_its_step
agent::mvp_agent::tests::session_model_tests::subagent_model_owner_requires_exact_incarnation_and_actor_admission
session::actor::model_switch::tests::native_tool_sampling_rejects_model_and_effort_after_execution_closes
```

| Final regression | Result | Evidence |
| --- | --- | --- |
| Original native wrapper | PASS: all 21 subprocess cases checked; wrapper 1 passed, 0 failed, 192.24 s | `/tmp/grow-native-retirement-regress.log` |
| Original exact-owner wrapper | PASS: all 10 subprocess cases checked; wrapper 1 passed, 0 failed, 0.79 s | `/tmp/grow-native-retirement-regress-owner.log` |
| New actor Model/Effort rejection control | PASS: both selection variants; 1 passed, 0 failed, 0.01 s | `/tmp/grow-native-retirement-regress-actor.log` |

The serial regression command completed with actual exit 0 (task `01a1b267-b8c7-7c51-a1b7-d8dc96c3a2cb`). Each wrapper retains its entire original case list and asserts successful exit plus `1 passed` for every subprocess; no case is skipped. The pre-existing `input_admission.rs` irrefutable-pattern compiler warning remains outside this maintenance scope.

Final `git diff --check` passed for both Rust files and this record; SHA-256 source digests are in `/tmp/grow-native-retirement-check.log`. All owned Cargo/rustc/Shell test processes exited, and Cargo plus Rust source ownership was explicitly released to the parent before any parent build/PTY checks. No commit, push, release or real-user configuration change was performed.

## Parent acceptance and archive

Parent inspected the final diff, exact mailbox/receipt handling, formal retired Timeline evidence and the independent Model/Effort actor control. The direct three cases and original 21-case/10-case wrappers passed on the stable source; the fixture maintenance is accepted. The broader sampling UX still requires its separate real CLI/PTY milestone.

`openspec validate --all --strict --no-interactive` passed 22/0 before archive. `openspec archive align-native-control-retirement-fixture --yes` exited 0 and preserved `skip_specs`; no behavior spec was modified. Post-archive current validation passed 21/0 and `openspec validate --archived --strict --no-interactive` exited 0. Logs: `/tmp/grow-native-retirement-{final-strict,archive,post-strict,archived-strict}.log`.
