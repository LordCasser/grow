## Why

The persistence actor drops sampling candidate payloads when it drains its queue, but the producer first clones each full ACP notification into an unbounded persistence channel. A slow persistence actor can therefore retain provisional output proportional to stream volume even though it never writes that output.

## What Changes

- Send only an attempt key as the candidate's ordering marker through the persistence channel.
- Keep the full candidate notification on the live gateway path; keep canonical response projection as the only durable accepted output.
- Preserve untagged notification order around the first candidate marker.

## Capabilities

### Modified Capabilities
- `session-timeline`: candidate payloads are elided before entering the persistence queue.

## Impact

Session ACP dispatch, persistence actor and their focused tests. The persistence queue remains unbounded for other messages; this change removes candidate *bytes* from that queue without claiming a total session memory bound.
