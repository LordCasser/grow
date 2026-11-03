## ADDED Requirements

### Requirement: Exit suspension uses runtime ownership rather than visible pages

Pager SHALL ask the runtime to confirm suspension before exit even when Welcome or only viewer pages remain. A startup without confirmed session identity SHALL retain its owner and show an actionable unconfirmed exit. A leader SHALL derive and overwrite the requesting connection's scope from its existing session drivers; one client's exit SHALL NOT suspend another client's roots or reuse another client's pending exit. Standalone process exit SHALL cover all its roots. Confirmation SHALL cover a concurrently closing root's writer through the existing lifecycle transaction and suspension deadline. Global Quit input SHALL cancel any interrupted command prefix. Pending exit SHALL block new user admission while retaining resize and the existing retry/force actions; startup replay SHALL NOT swallow those actions. Leader startup requests SHALL settle registration before that connection can confirm exit.

#### Scenario: Visible pages do not establish runtime emptiness
- **WHEN** Quit is confirmed from Welcome or a viewer-only surface
- **THEN** runtime confirmation is requested before Quit; local view flags cannot skip it.

#### Scenario: Startup identity is not known yet
- **WHEN** Quit is confirmed while session creation or fork/load startup has no confirmed identity
- **THEN** the startup owner remains and exit is unconfirmed with an explicit retry; the process does not exit as if no runtime existed.

#### Scenario: Two clients request exit
- **WHEN** clients with distinct driver roots request exit, including overlapping in-flight requests
- **THEN** each request covers only its authoritative scope and has its own pending owner; forged client scope is overwritten by the leader.

#### Scenario: Runtime close is draining
- **WHEN** a root has left the resident map but its close still owns a writer
- **THEN** exit cannot succeed until the existing lifecycle transaction proves that writer has settled, and failure retains the existing retry/force flow.

#### Scenario: Quit interrupts a command prefix
- **WHEN** the user opens Ctrl+X guidance and then presses global Quit
- **THEN** the old prefix cannot make a later Ctrl+X confirm closing the session.
