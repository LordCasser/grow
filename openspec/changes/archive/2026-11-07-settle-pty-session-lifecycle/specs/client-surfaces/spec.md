## ADDED Requirements

### Requirement: PTY session controls acknowledge the owned child lifecycle

PTY Session SHALL retain one owner for its directly spawned child, stop and timeout requests, actual exit status, and final output delivery. Reader EOF SHALL NOT establish child retirement. Stop success SHALL require confirmed child settlement and owned I/O completion; cancellation of an observer SHALL NOT cancel an admitted stop responsibility. Natural exit SHALL close the service unless linger is enabled. Explicit stop and the configured startup timeout SHALL close the session regardless of linger. Failed settlement SHALL remain observable rather than returning success.

#### Scenario: Child exits with final output
- **WHEN** the child writes its final line and exits with a nonzero status
- **THEN** complete terminal observation contains that output and the actual status, and EOF alone cannot supply a fabricated successful exit.

#### Scenario: Explicit stop or timeout
- **WHEN** stop is requested or the original timeout expires, including a linger session
- **THEN** the original child owner settles that request once, exposes its real terminal result or an explicit failure, and does not extend the deadline on repeated observations.

#### Scenario: Natural linger
- **WHEN** a child naturally completes under linger
- **THEN** the service retains final screen and status until a later explicit stop or timeout requests service shutdown.

### Requirement: PTY services consume the Session terminal owner

The PTY HTTP/WS service and CLI SHALL consume the Session's control and completion handles without holding the outer Session lock across shutdown waits. HTTP stop SHALL report success only after confirmed settlement. WS terminal status SHALL agree with the Session's actual result and bounded connection cleanup SHALL not indefinitely block service shutdown. Bind, registration and serve failures after child spawn SHALL clean up that exact child owner. Name cleanup SHALL remove only the registration still pointing to the current server's port; force replacement SHALL NOT authorize stopping the previous registered PID.

#### Scenario: Service startup fails after spawn
- **WHEN** bind or name registration fails after this CLI has spawned its child
- **THEN** this child is settled and the startup failure remains visible, including an explicit cleanup failure if settlement cannot be confirmed.

#### Scenario: Stop with an open WebSocket
- **WHEN** HTTP stop is requested while a WS is observing the same session
- **THEN** successful stop and the WS closed result describe the same actual child exit, and a slow WS cannot hold service shutdown indefinitely.

#### Scenario: Name has been replaced
- **WHEN** this server ends after another server has force-replaced its name registration
- **THEN** cleanup leaves the replacement's registration and process intact.
