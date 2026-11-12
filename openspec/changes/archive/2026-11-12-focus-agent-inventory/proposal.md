## Why

Primary Agents already discover online peer sessions and default to active child results. The default child listing still verifies every historical terminal link before dropping those rows. Retrieving one known child's final answer also requires calling the general wait tool.

## What changes

Keep the primary session discovery call focused on online peers. Make active subagent listing avoid opening historical child journals for already-ended tasks. Retain explicit all-history enumeration with per-child state. Add a read-only, single-child result lookup for retrieving a final answer again.

## Capabilities

### Modified Capabilities

- `local-coordination`: Model-facing discovery of online primary peers.
- `session-timeline`: Active inventory and explicit single-subagent result lookup.
