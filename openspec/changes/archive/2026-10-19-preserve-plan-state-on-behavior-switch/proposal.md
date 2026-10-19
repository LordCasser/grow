# Proposal: Preserve Plan state when selecting another Behavior

## Why

Behavior switching currently persists a newly selected snapshot from empty defaults. The live coordinator retains its session-scoped candidate and Plan submission counter, so the acknowledged Control can disagree with live state and a cold restore can lose the candidate or reuse a submission identity.

## What changes

Build the durable target snapshot by applying the coordinator's existing selection semantics to a copy of its current state. Persist that snapshot before applying the same selection to the live coordinator. Preserve the existing manual completion dismissal rule and the existing Plan runtime reset.

## Capabilities

- Update `behavior-goal`: successful Behavior selection carries retained Plan state and submission identity through the durable Control snapshot, while clearing a completed-plan reference under the established manual-selection rule.
