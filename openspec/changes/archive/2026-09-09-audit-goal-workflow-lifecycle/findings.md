# Goal / Workflow lifecycle audit — 2026-09-09

## Confirmed defects

| Finding | Root cause | Resolution and evidence |
| --- | --- | --- |
| Descendant startup closes root Goal admission | An empty local child tracker overwrote the inherited shared admission window | Already fixed in 0662a86c; archived `2026-09-09-fix-descendant-goal-window-sync`; failing baseline and descendant/root ownership regressions |
| Resume overtakes Workflow terminal commit | Tracker becomes resumable before watcher persists its terminal manifest and Timeline Ended | `2026-09-09-fix-workflow-resume-terminal-barrier`, code c3a7e345; deterministic withheld-ack baseline violated causal fold; resume now borrows the existing terminal receiver and propagates failure without advancing epoch |
| Late unknown usage rewrites a stopped Goal | Incomplete-usage pause accepted every status except Paused; actor applied budget preemption to unrelated stopped work | `2026-09-09-fix-late-goal-usage-lifecycle`, code cf88d393; baseline converted Complete to Paused; fix preserves all stopped states and durable accounting, while retaining retry fencing for the same retiring Goal turn |

## Connected source audit

Shell paths below are relative to `crates/codegen/shell/src/`, pager paths to `crates/codegen/pager/src/`, workflow crate paths to `crates/codegen/workflow/src/`; tool paths are shown from the repository root. Inspection follows entry point, state owner, durable commit and consumer; tests provide regression evidence, not proof of every possible interleaving.

| Area | Evidence inspected | Invariant / result |
| --- | --- | --- |
| Goal create, edit, restart, restore, budget | `crates/codegen/tools/src/implementations/grow_build/update_goal/mod.rs`; shell `session/slash_commands.rs`, `goal_tracker.rs`, `actor/goal.rs`, `actor/turn/admission.rs`, `actor/spawn.rs` | Tool create has optional budget and explicitly instructs omission unless requested; slash/implicit creation and restore retain None. Only Some enforces token spending. Objective edit preserves an earlier explicit budget; removal is explicit. No hidden default cap found. |
| Goal ledger and provider admission | `actor/goal_support.rs`, `goal_tracker.rs` | Stable Goal id, admitted attempt identity, exactly-once settlement and root-owned window. Unknown usage is lower-bound evidence; it only closes active explicit-budget admission. Late stopped-state bug fixed independently. |
| Descendants and auxiliary requests | `actor/sideband.rs`, `actor/goal_support.rs`, `tools/notification_bridge.rs` | Sideband captures owner/epoch, admits only at provider start after durable attempt, settles through root, and retains detached settlement on cancellation. Child local refresh cannot replace root admission. |
| Continuation, blockers, idle ordering | `actor/goal.rs`, `actor/idle_arbitration.rs`, `actor/notification_drain.rs`, `goal_tracker.rs` | Only Active continues; foreground, FIFO and receipts precede continuation. Identity/revision rechecked at admission. Blocker requires three consecutive distinct Goal turns. Stopped/old Goal receipts cannot autostart unrelated Normal work. |
| Behavior and Goal transaction | `actor/session_mode.rs`, `actor/model_switch.rs`, `actor/goal.rs` | Step-control and Goal transaction gates serialize control; manager lock serializes special Behavior and Workflow admission. Durable Control precedes visible Goal/Behavior state; failed commit restores prior snapshot. |
| Workflow definition/workspace/registry | `session/workflow/workspace.rs`, `registry.rs` | Draft validation tied to exact hash; preflight rechecks current hash and live Behavior after releasing lock. Publish uses intent recovery, baseline hash, directory-contained IO, exclusive publish lock and atomic write/no-clobber. Draft changes do not mutate existing run source. |
| Workflow entry points | `crates/codegen/tools/src/implementations/grow_build/workflow/mod.rs`; `actor/spawn.rs`, `actor/workflow_run.rs`, `actor/tool/authorization.rs` | Required action, argument bounds, captured and current Workflow Behavior, session ingress check; blocking preflight stays off async worker. Tool and slash resume use stored immutable script/args and central manager. |
| Execution and replay | workflow crate `engine.rs`, `journal.rs`; shell `session/workflow/manager.rs` | Bounded Rhai execution, journal sequence/kind/hash and operation intent, host cancellation/budget control. Resume now waits previous durable terminal. Journal error recovery retains operation identity; partial physical tail repair does not discard acknowledged history. |
| Host services, child calls and limits | `session/workflow/host_service.rs`, `tracker.rs`, `manager.rs` | Frozen per-run model/catalog route, normal model authorization, concurrent-call semaphore, budget reservation/release, acknowledged child cancellation, bounded drain and Interrupted on failed cleanup. Workflow agent-call quota is distinct from Goal token budget and remains intact. |
| Persistence and startup recovery | `session/workflow/store.rs`, `tracker.rs`, `actor/spawn.rs` | Timeline Spawn/lifecycle authority plus hash-checked immutable script/args; sidecar cannot change frozen route. Invalid run isolated. Unclean active execution becomes Interrupted; conflicting restored Goal/Workflow fails closed without deleting the run. |
| TUI state/error projection | pager `app/acp_handler/session_notification.rs`, `app/agent_view/goal.rs`, `workflow_ingest.rs`, `views/goal_detail.rs` | Event/run revisions suppress stale projections; cleared state is not resurrected by old events. Goal None displays usage without fabricated denominator. Controls project actionable stopped/complete/budget-limited reasons. |

## Verification

- Goal filtered Shell regression: 148 passed, zero failed.
- Workflow filtered Shell regression: 188 passed, zero failed.
- Cross-module regression: **12,317 passed, zero failed, 13 existing ignored**. chat-state 465; memory 302; pager 7,152 (10 ignored); pager-minimal 86; sampler 218; sampling-types 266; shell 3,763 (3 ignored); workflow 65.
- Command: `cargo test --locked --offline --lib -p chat-state -p sampling-types -p sampler -p memory -p workflow -p shell -p pager -p pager-minimal -- --test-threads=2`, with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`. Full local output: `/tmp/grow-goal-workflow-audit-core.log`.
- Final `openspec validate --all --strict --no-interactive`: 17 passed, zero failed; `openspec validate --archived --no-interactive`: 291 passed, zero failed, including both fixes and this audit.
- Each confirmed new defect has its own proposal, delta scenarios, design, regression and commit; existing descendant fix is referenced rather than duplicated.

## Limits and remaining uncertainty

This is a source-and-regression lifecycle audit on macOS, not an exhaustive proof of arbitrary scheduler or filesystem failure interleavings. No paid/live provider request was required; proxy/vendor response behavior and remote platform CI are outside this local run. The supplied session was examined read-only during the earlier descendant fix and has not been rewritten. Existing published v2.1.5 is unchanged.

No additional confirmed architecture defect from this pass is being hidden inside the two fixes. Speculative refactors of ledger, host task ownership or journal format were not implemented. The older OpenSpec baseline inventory remains separate work; this audit does not claim every inventoried contract has been canonicalized.
