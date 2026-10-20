# Preserve Plan identity in automatic Goal snapshots

## Why

Automatic Goal activation and Goal definition reactivation build a durable Behavior snapshot from empty defaults. The live `BehaviorCoordinator` keeps a deferred Plan candidate and its monotonic submission counter, so the acknowledged Control can disagree with memory and a cold restore can lose the candidate or reuse an identity.

## What changes

- Derive the selected Goal snapshot by restoring a temporary `BehaviorCoordinator` from the current snapshot and applying the coordinator's existing selection operation.
- Persist that derived snapshot before changing the live coordinator.
- Cover first Goal activation, Goal definition reactivation, cold restoration, and the next Plan submission identity.

## Capabilities

- Modify `behavior-goal`: automatic Goal lifecycle transitions preserve independent retained/completed Plan state and the submission counter while clearing Plan runtime state.

## Scope

The implementation is limited to `crates/codegen/shell/src/session/actor/goal.rs` and this change record. It adds no new persistence format or coordinator behavior.
