## Why

The Pager's Behavior availability is a cached projection of Shell admission state. Foreground changes and queued step controls can change a choice's disposition while a Picker remains open, but some of those transitions do not publish a new projection. Concurrent projection builds can also arrive out of order and replace a newer view with an older one.

## What Changes

- Publish a Behavior availability update when prompt or Goal-turn promotion changes the foreground admission facts, a regular turn enters terminal settlement, terminal/compaction/cancel arbitration releases a session to idle, a manual compaction claims foreground, and a step or Behavior control changes foreground admission.
- Give each Shell projection a monotonic revision and have Pager ignore a projection older than its latest accepted revision.
- Keep Shell admission authoritative; every requested Behavior transition continues to be revalidated against live facts.

## Capabilities

### Modified Capabilities
- `behavior-goal`: specify fresh, ordered Behavior availability projections.

## Impact

The Shell's projection publication and queued-prompt promotion paths, the shared `BehaviorAvailability` wire type, Pager's projection cache, and regression tests. No generic event framework or second admission state machine is introduced.
