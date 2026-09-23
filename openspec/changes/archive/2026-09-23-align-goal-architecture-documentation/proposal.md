# Change: Align Goal architecture documentation

## Why

`docs/architecture/goal-continuation.md` still labels the durable Goal runtime
as architecture v9, while `GoalTracker` accepts only
`GOAL_ARCHITECTURE_VERSION = 10`. The article also omits the categorized usage
fields that the current `GoalState` persists, even though later sections
describe their cache-hit, cache-miss, output, and incomplete-usage semantics.
That drift can mislead maintainers about which snapshots are accepted and what
the durable accounting state contains.

## What Changes

- Align the article title and snapshot-version explanation with architecture
  version 10 and the current rejection of older snapshots.
- Show the current categorized usage fields in the article's illustrative
  `GoalState` shape, without changing implementation or specification files.
- Remove the resolved Goal documentation-drift entry from `openspec/backlog.md`
  after the source-backed documentation check.

This is documentation and backlog maintenance only. `skip_specs: true` is
intentional because no runtime behavior, persistence contract, or specification
changes.

## Impact

Only the developer-facing architecture article and resolved backlog bookkeeping
change. `crates/codegen/shell/src/session/goal_tracker.rs` and
`openspec/specs/behavior-goal/spec.md` remain the source and contract evidence.
