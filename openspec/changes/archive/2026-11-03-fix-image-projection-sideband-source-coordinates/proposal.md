## Why

ImageProjection advances Surface identities for each shadow, projected tool call, and response carrier. Timeline replay and live application assign coordinates to all of those entries, but source ownership currently recognizes only shadow coordinates. A valid Sideband source referring to a projected tool call or carrier is therefore rejected during recovery.

## What Changes

- Make ImageProjection Surface-coordinate ownership match the existing live and replay allocation order: shadows, then one coordinate per projected tool call, then one per carrier source.
- Preserve the existing shadows-only boundary and reject coordinates beyond the event's allocated slots.

## Capabilities

### Modified Capabilities

- `session-timeline`: canonical ownership checks recognize all Surface coordinates produced by ImageProjection.
