# Verification

Source review confirmed a documentation-only drift:

- `crates/codegen/shell/src/session/goal_tracker.rs` declares
  `GOAL_ARCHITECTURE_VERSION: u8 = 10` and rejects snapshots whose version
  differs from that constant.
- The current `GoalState` persists `usage_breakdown`,
  `usage_incomplete`, and `usage_incomplete_acknowledged` in addition to the
  fields already shown by the article. `GoalTokenUsage` carries cached input,
  uncached input, and output.
- `openspec/specs/behavior-goal/spec.md` matches the article's lifecycle,
  three-turn blocker audit, root-owned admission, delegated ownership, and
  cache-inclusive usage boundaries. No runtime or contract discrepancy was
  found that belongs in this small change.

Implementation and validation:

- Updated `docs/architecture/goal-continuation.md` from v9 to v10, changed the
  snapshot rule to state that another architecture version is rejected, and
  added the current categorized usage fields to the illustrative state shape.
- Removed only the resolved Goal documentation-drift bullet from
  `openspec/backlog.md`; existing unrelated dirty backlog edits were preserved.
- Focused newline/trailing-space checks for the edited documentation passed.
- `git diff --check -- docs/architecture/goal-continuation.md openspec/backlog.md`
  passed.
- `openspec validate align-goal-architecture-documentation --strict
  --no-interactive` passed before archive. No Cargo command was run because
  this change only updates documentation and backlog bookkeeping.
- `openspec archive align-goal-architecture-documentation --yes` completed as
  `2026-09-23-align-goal-architecture-documentation`.
- `openspec validate --archived --no-interactive` passed: 364 archived changes,
  0 failures.
- `openspec validate --all --strict --no-interactive` passed for 19/20 items.
  The one unrelated failure is the active `inventory-all-crate-features`
  change, whose existing `.openspec.yaml` sets `skip_specs` while its change
  directory still contains delta spec files. This documentation change itself
  and all other active items passed.

The change is archived; no runtime or specification behavior changed.
