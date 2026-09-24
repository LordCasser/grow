## Context

`ScrollbackState::push_subagent_permission` already checks that its active group matches the state's current epoch, and reconnect merging filters tail groups by epoch. The remaining bypass is that block-level `push` and `extend` are public and do not encode or check the source epoch.

## Goals / Non-Goals

**Goals:** Make state-owned epoch checks unavoidable for production membership changes and retain a defensive epoch check inside the block mutation methods.

**Non-Goals:** Add event provenance to permission transport, change permission decisions, or alter detail redaction, ownership, or requester cancellation.

## Decisions

Keep the existing group epoch as the boundary. Make append and merge methods crate-internal, require the caller to provide the source epoch, and reject mismatches in the block itself. `ScrollbackState` supplies its current epoch for normal appends and checks both source and destination epochs during reconnect merge. This avoids adding another epoch field to every notification while preserving explicit checks at the two existing mutation paths.

## Risks / Trade-offs

- A crate-internal caller could still pass an incorrect epoch → keep the owning state as the only production caller and test mismatch rejection at the block boundary.

## Migration Plan

No persisted data migration is required. Existing blocks retain their epoch and reconnect behavior; the change only narrows the mutation API.
