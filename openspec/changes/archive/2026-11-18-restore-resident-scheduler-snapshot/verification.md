# Verification

## Two-client resident scheduler reproduction

Ran the isolated Python ACP harness against the rebuilt v2.6.16 binary:

```sh
GROW_BINARY="$PWD/target/debug/grow" python3 /tmp/grow_scheduler_repro.py
```

The harness created a temporary `GROW_HOME`, a loopback mock provider, and a loopback resident Leader. It connected two real ACP clients to the same Leader and session. Client A issued `/loop 60s repro scheduled prompt`; the mock model admitted the real scheduler tool call and the scheduler emitted a recurring fire with an admitted child. Client B then loaded the same session. No user or repository provider credentials/files were used.

Observed evidence:

- A's `scheduled_task_fired` reported task `01a1ea71-5e2e-7232-8743-814790e3b32b`, `next_fire_at` `2026-11-17T16:58:40.658755+00:00`, and `subagent_id` `01a1ea71-5e5a-7062-ab8d-7c3f97a6cee7`.
- B's historical replay contained only `scheduled_task_created` for that task, with the earlier `next_fire_at` `2026-11-17T16:57:40.654390+00:00` and no child link.
- B's `session/load` response `_meta.grow/schedulerSnapshot` had scheduler revision `2` and restored `nextFireAt` `2026-11-17T16:58:40.658755+00:00` plus `lastSubagentId` `01a1ea71-5e5a-7062-ab8d-7c3f97a6cee7`.
- The harness verified the snapshot values exactly matched A's latest fire and child. The loopback provider request count did not increase during or after B's load, so attaching B did not execute the scheduled task.
- The isolated run exited successfully (status 0); the harness process cleaned up its temporary state.

The final candidate repeated the two-client probe successfully. All 14 Pager scheduler tests passed, including a newer live fire arriving before an earlier durable create on a different task, snapshot adoption across scheduler generations, deletion tombstones and an absent one-shot fire.
