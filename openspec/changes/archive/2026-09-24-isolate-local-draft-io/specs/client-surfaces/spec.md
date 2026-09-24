## ADDED Requirements

### Requirement: Local draft filesystem latency does not block Pager interaction

Pager SHALL perform local-draft cwd resolution, load, rekey, write, remove, and quarantine I/O outside its input and paint event loop. A stalled draft filesystem operation SHALL NOT delay key handling or drawing. Ordered worker commands SHALL preserve prompt-RPC invalidation before a later local draft write. A recovered draft SHALL apply only to its still-current agent/session/cwd binding and only when live unsent input has not superseded the load. Normal quit SHALL wait for a checkpoint of the latest eligible draft and pending invalidations; a filesystem failure SHALL retain retry intent without a busy loop.

#### Scenario: Slow draft store during editing

- **WHEN** a draft read or write is held by a controlled slow filesystem operation while the user types
- **THEN** Pager continues to handle and draw the new input, and the eventual disk completion cannot replace that newer input.

#### Scenario: Prompt ownership changes while disk work is pending

- **WHEN** a prompt RPC transfers draft ownership while an older write or load is in flight
- **THEN** invalidation is ordered after old work, a stale recovery cannot restore submitted text, and a subsequent new draft can persist without being deleted by the old invalidation.

#### Scenario: Quit with pending draft work

- **WHEN** Pager quits while the latest eligible draft or invalidation is still pending
- **THEN** its checkpoint waits for the ordered worker outcome and does not report a successful normal exit before the attempt completes.
