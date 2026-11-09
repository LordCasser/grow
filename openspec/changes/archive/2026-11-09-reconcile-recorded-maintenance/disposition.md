# Recorded item disposition, 2026-11-08

Baseline: `8fbc52185ce30e38d97745dd6597242da1135c31`. The latest request covers recorded items not explicitly deferred. Explicit deferrals remain excluded unless the user specifically reauthorizes them. ACP measurement tasks expressly require future reauthorization, so that experiment is not part of this request. A requirement for reproducing an unconfirmed defect is still an evidence boundary, not permission to invent a production fix.

## Concrete maintenance

- The `acp_agent.rs` Clippy `never_loop` is still present at baseline: both `tokio::select!` branches break on their first evaluation. A value-returning select preserves the biased completion/ACK ordering. This change removes the redundant loop without changing prompt admission, roster updates or lifecycle semantics.
- Backlog's old readiness paragraph contradicts the already-archived readiness repair. [The final Shell verification](../2026-11-06-separate-finalization-readiness-budget/verification-shell.md) records 250 passing regressions, including both readiness cases. The stale paragraph is corrected; this is not a new production repair or a claim about the original Windows machine.
- The explicitly deferred ACP experiment proposal, source and tasks remain exactly at baseline, including its pre-existing preparation-link issue. No edit within that deferred change is part of this delivery.

## Measurements and independent owners

| Item | Disposition | Boundary before closing |
| --- | --- | --- |
| ACP-1 | Explicitly deferred in [the existing tasks](../../measure-acp-gateway-backlog/tasks.md), which require future reauthorization. The prematurely added measurement fixture and renewed-scope checklist have been removed without executing the experiment. | Retain the original preparation and static risk only; no measured queue/handler result or capacity repair is claimed. The proposal and its pre-existing preparation-link issue also remain untouched. |
| MD-1 | Optimized measurements and 502 output regressions passed; see [results](../2026-11-09-measure-open-fence-copy-costs/verification.md). | The 4,096-line sample has a 3.101 ms warm clone and 7.772 ms median Markdown growth frame. Cumulative copying remains quadratic; no production representation repair or whole-UI budget pass is claimed. |
| Workflow test isolation | Established actionable fixture defect, assigned to its own test-maintenance change. | Preserve the product 512 KiB manifest bound. Earlier 147-pass isolated control already established discovery-environment dependence, not an exact byte attribution. |
| Agent public entities | Current source audit confirms the only Rust matches for `ToolExecConfig`, `ToolRetryConfig` and the preset registration functions are declarations/internal field references. GitHub indexed search under LordCasser found only Grow declarations/docs. | The source explicitly documents out-of-tree presets. Search coverage cannot prove no supported external consumer; no API removal without supported-consumer confirmation. This is an API-use decision, not an established runtime failure. |

## Missing scene-specific evidence

- **macOS fsnotify retirement jitter:** the archived CLI record includes a 4747 ms watcher retirement and a process still alive after 6019 ms, alongside fast runs. Current source retains synchronous `FsNotifyHandle::drop` join. This worker has only a Linux cloud environment; macOS dynamic stacks/reproduction are not available. Linux tests would not verify or repair the reported FSEvents tail. No backend rewrite or detached cleanup is claimed.
- **Released delivery/live recovery:** [the production API repair and isolated full flow already passed](../2026-11-05-audit-released-delivery-finalization/verification.md). The remaining request is to resume the old specific user session after its writer retires and verify canonical result/replan. That host/session is not available here. No live session is restarted or storage edited; source correctness is not reported as old-host recovery.
- **Target Chat compatibility:** [the official contract comparison](../2026-11-06-compare-official-sampling-api-contracts/analysis.md) distinguishes synthetic runtime tool history and incomplete foreign reasoning. Actual target acceptance needs the specific provider/model/deployment and controlled paid/live requests. No production request was made; a local wire check cannot establish acceptance, invent missing native reasoning or justify rewriting accepted history.
- **Original blocking Q&A stall:** [the archived bounded investigation](../2026-11-07-settle-late-question-replies/verification.md) did not reproduce it. The later advisory mode has separate passing product evidence and does not close this report. The missing evidence remains source session/call, original Turn end reason and transport/canonical state, or a same-effect duplex/reconnect reproduction. No arbitrary timeout or cancellation protocol is introduced.

## Explicit deferrals preserved

The backlog's “明确暂缓” section remains unchanged: sampler concurrent log capture, ordinary prompt-export lifetime, glyph boundaries outside the shared layout, stronger cross-route reasoning/provenance, remote control/agent host, provider/cache affinity, recap cache sharing, multi-selection compaction, MCP multi-round elicitation, and ProcessScope PID/PGID reuse. The explicitly deferred ACP capacity experiment and separately gated model pool also remain unchanged. These are not counted as completed by this maintenance work.
