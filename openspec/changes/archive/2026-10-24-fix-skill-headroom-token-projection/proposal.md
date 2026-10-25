## Why

Explicit skill loading can fail with `RejectedHeadroom` while the TUI shows about 531K / 1M context. The reported session's provider log records 530111 input tokens immediately before the failed skill turn, and 531346 in a subsequent ordinary request. Skill publication currently recomputes the whole canonical Surface estimate instead of using ChatState's provider-anchored request projection. This includes history omitted by the provider projection and also ignores provider envelope overhead when it exceeds the raw estimate.

## What Changes

- Use the existing projected context pressure at both user-group and model-skill publication boundaries.
- Continue estimating the newly added complete content and checking current Surface revision and headroom atomically.
- Preserve full-body delivery, atomic input consumption, and real budget rejection behavior.

## Capabilities

- `model-sampling`: consistent context accounting for skill publication.
