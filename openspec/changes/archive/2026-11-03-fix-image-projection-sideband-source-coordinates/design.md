## Context

`Timeline::owns_surface_id` validates historical source coordinates used by Sideband selections. `apply_event` assigns ImageProjection coordinates in `timeline.rs` in the same order as `fold_branch_provenance`: each shadow gets its enumeration index, then each `ImageToolCallShadow` gets one slot, followed by one slot for each `carrier_source`.

The ownership predicate currently bounds ImageProjection IDs by `shadows.len()`. This rejects valid coordinates emitted by the existing producers after tool-call redaction, including Sideband source references to projected tool results.

## Decision

Compute the canonical slot count from the already validated event shape: shadow count plus one slot per tool-call shadow plus one per carrier source. Accept only item indices below that count. Keep allocation and projection semantics unchanged.

The regression will retain the existing shadows-only acceptance/rejection assertions and add multiple tool calls and carriers, upper-bound rejection, and a replayed Timeline check.

## Risks

The predicate must use the same per-tool-call and per-carrier accounting as both producers. Existing Timeline event validation and replay parity remain authoritative for event validity; this change only fixes historical coordinate ownership.
