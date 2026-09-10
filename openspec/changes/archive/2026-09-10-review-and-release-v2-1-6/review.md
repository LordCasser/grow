# Release review

Baseline: published v2.1.5 (`a8dc9371`). Reviewed committed changes through `c03a62fa` and the three archived implementations present in the working tree: `improve-goal-status-and-usage`, `preserve-async-compaction-task-continuity`, and `unify-sampling-attempt-recovery`.

At the user's subsequent direction, the release also includes `audit-session-colon-stop` and `preserve-portable-tool-exchanges`, completed by another agent during validation. The follow-up review checked the complete implementation, callers, three backend encoders, token estimation, source-projection evidence, and regression changes before inclusion. The portable prefix now closes over appended results without crossing a new message or native span. Complete unambiguous exchanges retain structural tool roles; invalid/duplicate records and provider reasoning carriers remain excluded. Attachment eviction text is preserved by all three encoders. This fixes the reproduced request-pairing defect, without claiming that every valid provider end_turn will stop producing an action preamble.

The other agent subsequently completed `require-explicit-turn-completion` and `audit-provider-stop-provenance` before tagging. They are also included. Follow-up review checked strict independent declarations, durable result acknowledgement, visible-answer enforcement, mixed-call rejection, bounded protocol recovery, terminal priority, pending-input invalidation, and all three endpoint regressions. This supplies explicit completion evidence; it does not prove the truth of a model's completion claim. The process mock is aligned explicitly in ordinary final branches, retaining tool-free Sideband responses and all original process assertions.

## Findings

The independent-process regression found a native unoptimized debug crash on an ordinary prompt: the explicitly sized 8 MiB session thread overflows its stack before reaching the model. macOS crash reports identify the fixture binary and stack guard in the conversation-turn poll chain; this does not establish an optimized release failure. Following the user's direction not to reshape production logic around test stack constraints, `allow-debug-session-stack-overhead` adjusts only debug stack capacity. A heap-pinning experiment did not resolve debug temporary frames and was fully reverted. The unchanged process regression and shell tests remain validation gates.

The native process regression also exposed a stale fixture expectation: resident reload projects the latest approved inquiry state, whereas the fixture required the initial receipt. `align-coordination-reload-regression` now asserts the exact approved nonterminal state; all ten process scenario groups passed. The debug-only stack correction also passed the complete shell regression.

Subsequent Windows platform validation found nonportable actor/PTY/symlink fixtures and runtime manifest-replacement/observer-sharing failures. Failure diagnostics also exposed immutable artifact publication beyond MAX_PATH and ordinary files misclassified as operational directory-scan errors. They are addressed in the active `fix-windows-coordination-release-regressions` change; final Windows validation is required before release. No additional blocker was identified in the other reviewed paths. This is not proof of every provider or every possible interleaving.

| Area | Evidence checked |
| --- | --- |
| Retry admission | Sampler request task and RecoveryBudget share the attempt count and absolute deadline across session repairs. Explicit zero, irreversible output, provider-side operations and owner closure stop resampling. |
| Billing | `sampling_usage_sink` records immutable ordinary-ledger settlement before readmission, returns the Goal lease even on ordinary-ledger failure, and debits child output only once. Captured prompt identity uses `current_prompt_index`; accepted output only updates the context anchor. |
| Stream correctness | Three backend parsers preserve protocol conflicts, reject incomplete candidates and invalid tool siblings, and keep legitimate terminal outcomes distinct from transport recovery. Optional Chat usage-tail loss does not regenerate a completed answer. |
| Preview and replay | Ordered Started/Discarded/Accepted boundaries pass through shell, chunk merging, persistence and leader fanout. Durable response acknowledgement precedes Accepted/tool execution. Root and child reconnect tests exercise missed boundaries and failed reload ownership. |
| Goal and usage UI | Root owns shared Goal admission; descendants inherit the complete owner identity only when delegated. Late usage preserves stopped states. Get/CreateGoal project ACP Completed, while transient usage snapshots and weighted cache ratios share the existing ledger. |
| Parent-child communication | The coordinator derives routes from active direct ownership, rejects unrelated routes, and separates Sideband inquiry from durable parent guidance. Interventions do not become human authorization; final steering closure drains accepted messages. |
| Compaction and replay | Compaction continuity is appended only after the next ordinary Step is admitted. Historical summaries cannot claim authority over newer tail messages. Bulk replay validates a private fold; live append keeps its transactional path. Workflow resume waits for terminal persistence. |
| Release | Existing workflow builds an immutable annotated tag, verifies the exact ten-platform archive set and attestations, and publishes only after remote SHA256SUMS verification. |

Atlas was opened on this checkout and used for scoped sampler symbol and call-evidence queries. Its evidence was local to the focus closure; an empty method search was supplemented by direct source inspection and was not interpreted as absence of callers.

## Validation limits and maintenance

- Existing scoped rustfmt checks report formatting differences. An attempted partial formatting operation during this review was reverted completely after it broke module declarations; the saved pre-format and restored rustfmt outputs match exactly. No formatter-induced source change is retained, and final tests are run again on the restored source with version 2.1.6.
- Native macOS tests and mock HTTP streams do not prove real-provider billing or remote operation exactly-once behavior. Existing ignored cgroup/soak/environment tests remain explicitly ignored.
- Preview resource bounds and other unrelated debts remain in `openspec/backlog.md`. The user-requested portable tool-history correction closes its two documented items. A separate existing Messages ID-sanitization collision boundary is recorded for follow-up; the reproduced session uses safe IDs and does not exercise it.
- Disk space fell to 1.6 GiB. Cargo's package-scoped cleanup removed generated shell/Pager/chat-state/sampler/tools outputs, restoring about 29 GiB. Subsequent local builds use `CARGO_INCREMENTAL=0` and preserve external dependency caches.
