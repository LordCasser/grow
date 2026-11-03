# Verification

| Boundary | Result | Evidence |
| --- | --- | --- |
| SessionActor wait/intervention, terminal ordering and durable cancel receipts | 3 passed / 0 failed | Final Shell core run `/tmp/grow-260-final-core5.log`: `actual_session_wait_settles_intervention_on_both_sides_of_child_terminal`, `expired_source_timeline_queries_drain_after_chat_state_ack_is_released`, and `kill_task_session_dispatch_persists_backend_cancel_receipts` all report `ok`. The full Shell crate result is 4576 passed / 0 failed / 9 ignored. |
| Coordinator observation and output readers | 4 passed / 0 failed | Final Tools core result `/tmp/grow-260-final-core5.log`; covers active forgotten/retired/lost-owner distinctions, bounded reader permits and reuse, panic settlement, and public active discovery. Full Tools crate result is 2441 passed / 0 failed / 7 ignored. |

The active-discovery tests preserve the distinction between an individual forgotten ID and an unavailable or empty child inventory. They do not synthesize an empty inventory from a single missing result. The panic case proves unwind recovery in the test binary only; release profiles use `panic=abort` as recorded in the [panic containment verification](../2026-11-04-catch-subagent-output-worker-panics/verification.md).

The local final core run is not the required macOS/Linux/Windows native CI matrix. Earlier fixture failures are retained in the [umbrella verification](../2026-11-04-close-v2-6-0-release-blockers/verification.md). Strict OpenSpec validation and archive review remain outside this record.

## Native platform gate on the repaired source

[Native run 41689811331](https://github.com/LordCasser/grow/actions/runs/41689811331) completed successfully on `fccd91b10e141d59d4ed87ca85629015a2949e55`. macOS job [126105107008](https://github.com/LordCasser/grow/actions/runs/41689811331/job/126105107008), Linux job [126105107454](https://github.com/LordCasser/grow/actions/runs/41689811331/job/126105107454), and Windows job [126105107534](https://github.com/LordCasser/grow/actions/runs/41689811331/job/126105107534) each passed the four required groups: Tools observation 4, cancellation scopes 21, Shell cross-layer observations 3, ProjectDiscovery startup ownership 4. These are real native runner conclusions, separate from earlier local evidence. The persisted-reader panic case validates unwinding test builds; release abort policy is unchanged. The same source's complete local 18-crate suite passed 19233 / 0 / 51 ignored (`/tmp/grow-260-candidate3-core18.log`).

Core PTY publication remains tracked by the umbrella change; the native observation gate is complete even though an unrelated Minimal key-sequence fixture failed in Core.
