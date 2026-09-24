## ADDED Requirements

### Requirement: Agent teardown retires its session writers
When an `MvpAgent` is destroyed while its process runtime continues, it SHALL request shutdown of every primary and active child session actor it owns. A replacement agent SHALL not load the same session until the old writer has released its lease.

#### Scenario: Agent owner ends with resident sessions
- **WHEN** the Agent owner is dropped while primary or child session actors remain resident
- **THEN** each actor receives the existing shutdown command and the owner releases its handles without blocking the local event loop.

#### Scenario: In-process leader replacement
- **WHEN** a leader generation is stopped and a replacement generation will load the same session
- **THEN** the replacement waits until the old session writer lease is released before loading; it does not overlap writers or treat an active lease as a successful load.
