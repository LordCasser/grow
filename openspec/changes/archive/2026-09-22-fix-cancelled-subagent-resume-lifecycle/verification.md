# Verification

## Incident baseline

- Grow `2.1.11 (3024dad)` session `01a0c7c4-70eb-7890-8d17-ddfbde5061cc` paused the root Goal after a provider billing/quota failure while two differently routed children were active.
- Parent facts for `01a0c8ce-b7e4-79e2-a0a0-bfa350139872` and `01a0c8d2-d4d5-7381-a20c-252b06828f79` contain canonical Spawned/Ended(cancelled) identities; the corresponding child Timelines contain cancelled SubagentResult facts and validate independently.
- The incident log records a later attempt-ledger write rejected because the child Timeline had already been closed by its SubagentResult. Both subsequent resume requests exited immediately with the old generic `no completed canonical lifecycle was found` message.
- Source inspection confirmed that `durable_resume_source_for` had no completed-outcome gate; the generic message came from collapsing every storage, identity, security, Timeline and exact-link rejection into `Option::None`. The cancellation race came from returning a synthetic cancelled result as soon as the outer token fired and sending child Shutdown at the same time as Cancel.

## Implementation evidence

- `ShellChildRuntime::cancel` now sends only Cancel. `await_subagent_turn_or_cancellation` treats token cancellation as a request to keep waiting for the real child prompt terminal; the result path no longer closes the child Timeline from a synthetic token-only outcome.
- Durable source resolution returns typed parent, lifecycle, security, child, identity, Timeline and result-link errors. The caller checks live pending/active ownership before durable acceptance and logs the internal typed cause while keeping missing/security failures externally non-enumerable.
- Exact cancelled Spawned/Ended/Seed/Result linkage remains valid resume authority; no outcome whitelist was introduced.
- Missing non-worktree cwd falls back only on `NotFound`; existing files, unverifiable paths and canonical paths outside the parent workspace reject.
- Resumed and verbatim-forked contexts use their validated inherited System head without invoking the current renderer. New and normalized contexts still require a rendered child head.
- Derived child launch now fails closed on control publication, Goal snapshot mailbox delivery, QueuePrompt delivery and first-prompt durable `persist_ack`. If prompt admission may have begun but its ACK is lost, the child is cancelled and the runner still waits for its real terminal before closing the derived Timeline.
- Historical completion output artifacts remain presentation/diagnostic data, not resume-context authority. Timeline Surface and directly referenced immutable prompt blobs remain authoritative.

## Automated checks

All commands ran from the repository root on 2026-09-22.

| Command / scope | Result |
| --- | --- |
| `cargo test --locked -p shell --lib agent::subagent::tests:: -- --test-threads=2` | 155 passed, 0 failed before the final exact-cancelled-link addition; all changed test code subsequently recompiled in the focused runs below. |
| `cargo test --locked -p shell --lib cancellation_waits_for_the_child_prompt_terminal -- --nocapture` | 1 passed; token cancellation remained pending until a real cancelled PromptTurnOk arrived. |
| `cargo test --locked -p shell --lib durable_resume_projection_requires_parent_terminal_fact -- --nocapture` | 1 passed; missing spawn and missing terminal preserve distinct typed causes. |
| `cargo test --locked -p shell --lib cancelled_canonical_link_remains_resume_eligible -- --nocapture` | 1 passed; exact cancelled Spawned/Ended/Seed/Result link validates and projects as a resume source. |
| `cargo test --locked -p shell --lib session::actor::tests::turn_pipeline_v2_tests::completed_runner_keeps_foreground_fenced_until_terminal_settlement -- --nocapture` | 1 passed. |
| `cargo test --locked -p shell --lib session::actor::tests::record_response_token_usage_tests:: -- --test-threads=2` | 12 passed, 0 failed. |
| `cargo test --locked -p chat-state --lib -- --test-threads=2` | 507 passed, 0 failed, 1 ignored read-only performance benchmark. |
| `cargo test --locked -p sampler --lib -- --test-threads=2` | 241 passed, 0 failed. Includes cancellation-before-admission, cancellation-during-stream and usage-settlement ordering coverage. |
| `cargo test --locked -p tools --lib implementations::grow_build::task::coordinator::tests:: -- --test-threads=2` | 37 passed, 0 failed. Includes Goal/session cancellation ownership and source-liveness security boundaries. |
| changed-file `rustfmt --edition 2024` | Passed. |
| `openspec validate --all --strict --no-interactive` before archive | 22 items passed, 0 failed. |
| `git diff --check` before archive | Passed. |
| `openspec archive fix-cancelled-subagent-resume-lifecycle --yes` | Completed; added 1 model-sampling and 3 session-timeline requirements. |
| `openspec validate --all --strict --no-interactive` after archive | 21 active specs/changes passed, 0 failed. |
| `openspec validate --archived --no-interactive` after archive | 353 archived changes passed, 0 failed. |
| `git diff --check` after archive | Passed. |

The macOS linker emitted its existing large `__eh_frame` performance warning for the shell lib-test binary; it did not affect compilation or test results.

## Disk cleanup

- `du -sh target` reported approximately 14 GiB before cleanup.
- `cargo clean` completed successfully and reported 63,085 files / 17.8 GiB removed.
- The post-archive check confirmed that the `target` directory remained absent.

## Scenario audit

- Cancellation settlement: covered by the new child-terminal wait test plus sampler cancellation/settlement and Shell turn-settlement tests.
- Canonical cancelled resume: covered by the exact cancelled link test and existing route/cwd/worktree resume tests.
- Typed rejection: direct missing-spawn/terminal assertions plus existing Timeline seed/result-link, identity, security, model/transport, context-window and worktree matrix tests.
- Live overlap: production ordering now checks coordinator ownership before any durable source materialization or derived side effect; coordinator liveness/security tests passed.
- First-prompt admission: production path now uses existing actor FIFO and durable QueuePrompt `persist_ack`; chat-state durable user-message and Shell settlement suites passed.
- Retry safety: all post-resolution failures create or close only the new derived epoch; source facts, source snapshot and source transcript are never mutated by the resume path.

No real provider call, billing failure or timing sleep was used. The regression suite controls cancellation and acknowledgement boundaries deterministically.
