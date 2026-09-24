## Context

`WorkflowEvent::Spawned` freezes the initial manifest; Ended/Closed/Resumed provide the lifecycle. `state.json` owns mutable phase, row, and usage presentation. The durable journal counts `spawn_agent` reservations and resume calls `reconcile_agents_used` before admitting further work. Thus a missing sidecar does not erase the budget safety boundary, but the seed alone cannot prove a displayed zero usage is complete.

## Decision

The restore resolver marks the selected seed projection `agent_usage_incomplete = true` before lifecycle reconciliation. Valid sidecars keep their value. This applies equally to the JSONL loader and Trajectory because both use the resolver. The repaired sidecar retains this marker until live execution can account for later calls; the journal remains the admission source on resume.

We accept loss of intermediate phase and row presentation when the sole progress sidecar is lost. Full parity would require durable, ordered, bounded checkpoints for every mutable UI transition, plus reconciliation of two progress stores. That extra write path is disproportionate to the recovery value and would create another authority boundary.

## Validation

Test a completed Timeline lifecycle with missing/invalid sidecar and assert the restored projection retains the terminal outcome while reporting incomplete usage. Test a valid sidecar remains complete. Verify existing journal-based resume budget tests and strict OpenSpec validation.
