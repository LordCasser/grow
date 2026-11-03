# Review

## Scope and attribution

- The retiring capacity accounting, pre-admission retirement sweep, unique-ID accounting, and their initial regression were implemented in this change by the retiring-path agent.
- The completed-cache eviction duplicate-ID gap was found by the main agent and assigned as a follow-up. This change then added `finalizations`/`retiring` to the spawn duplicate guard and added the ChannelBackend regression `retiring_identity_cannot_be_reused_after_completed_cache_eviction`.
- The pager transcript reclamation work belongs to the main agent. It was reviewed read-only in a separate bounded pass; no pager implementation was changed here.

## Adversarial review

- Capacity is the identity union of `pending`, `active`, `finalizations`, and `retiring`; an overlapping retirement/finalization consumes one slot.
- Admission sweeps first. Only `suspension_retired == Ok(true)` releases an owner; errors and unfinished owners remain accounted and owned.
- Duplicate identity checks include `finalizations` and `retiring`, so eviction from the bounded completed presentation cache cannot allow a second owner to overwrite the original retirement responsibility.
- ChannelBackend regressions cover retirement backpressure, overlap deduplication, release and admission recovery, retirement observation errors, and duplicate rejection after completed-cache eviction. The over-capacity regression uses `tokio::select!` so accidental dispatch fails immediately.

## Verification and remaining status

- `rustfmt --edition 2024 --check` passed for the coordinator and coordinator tests after the final test adjustment.
- `git diff --check` passed after the final test adjustment.
- `openspec validate account-retiring-subagent-capacity --strict --no-interactive` passed after the final test-only adjustment and review record.
- Cargo tests were not run by this agent. The main agent owns coordinated verification and archive work.

The independent pager review found two gaps at the time of review: a successful history retry left the old error notice visible, and missing-source behavior lacked a dedicated regression. Both were fixed by the main agent; targeted read-only re-review found no unresolved production issue. No pager test run is claimed here.

## Background-task child-history replay follow-up

- The main agent added `TaskBackgrounded` and `TaskCompleted` to `stream_session_history_at`, replayed both through shared task apply functions, retained pre-existing task owners during replay, and added a pre-apply child-history hydration path for late notifications.
- Read-only review found a completion-order edge in `apply_task_completed`: when a retained task owner is still `Running`, history replay could add the terminal row but leave the state running; the subsequent late live completion would deduplicate against that row and leave the owner running. The main agent fixed this by allowing replay to advance a retained running task to its terminal state while preserving already-terminal state. Re-review confirms canonical replay now settles the owner before the late notification is deduplicated.
- The regression uses a non-empty live `MonitorEvent` payload and empty canonical/live `TaskCompleted.output`, matching the bridge projection. It checks completion after reclamation, repeated close/reclaim/open, and duplicate lifecycle notifications without losing stdout or resetting the pending-kill owner. The targeted test has only been rustfmt-formatted and checked with `git diff --check`; no Cargo test is claimed.
