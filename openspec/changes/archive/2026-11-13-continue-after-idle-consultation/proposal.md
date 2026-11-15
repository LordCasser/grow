## Why

An answering session can receive a completed ask as attributed context while idle and leave the new information unread until someone prompts it again.

## What Changes

- Allow one internal continuation when a successful consultation is durably delivered to an eligible idle answering session.
- Route admission through the existing idle arbiter and consume the receipt with its exact attributed context before sampling.

## Capabilities

### Modified Capabilities

- `local-coordination`: successful ask completion and idle continuation.
- `session-timeline`: canonical receipt ownership and continuation eligibility.
