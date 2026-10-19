## ADDED Requirements

### Requirement: Suspension rejects new auxiliary command work while draining admitted owners

Acceptance of a valid physical session suspension SHALL close top-level auxiliary activity admission before asynchronous suspension persistence. A mailbox command that cannot obtain its existing activity permit SHALL terminate through the normal closed-session response path without panicking, admitting provider work or performing that command's asynchronous mutations. Best-effort command/prompt suggestions SHALL return no suggestion. The mailbox SHALL continue servicing existing settlement, runtime observation and suspension/restore control messages. Already admitted owners SHALL retain their existing cancellation and durable settlement obligations; deadline expiry SHALL NOT reopen admission or authorize a checkpoint before their drain.

#### Scenario: Late autocomplete after suspension admission
- **WHEN** AISuggest or SuggestPrompt is delivered after a valid suspension request has closed auxiliary admission
- **THEN** it returns no suggestion, starts no provider attempt, and does not panic or prevent lifecycle and observation commands from being processed.

#### Scenario: Suspension request persistence has not acknowledged
- **WHEN** the valid suspension owner is waiting for its initial durable request acknowledgement and a finite auxiliary command arrives
- **THEN** that new work is rejected before provider or command effects while the original suspension owner retains its identity and outstanding persistence.

#### Scenario: Existing owner outlives the caller deadline
- **WHEN** an already admitted auxiliary owner has not settled by the shared caller deadline
- **THEN** the caller receives an unconfirmed result, new finite work remains blocked, no checkpoint is acknowledged early, and the existing owner can later settle through the still-live mailbox. If its drain finishes after the deadline, the Session returns unconfirmed before dispatching new checkpoint writes with that expired deadline; a new request can retry the same operation with a fresh deadline without resetting the healthy writer.

#### Scenario: Finite command arrived before the gate closed
- **WHEN** a finite command obtained its permit before suspension closed admission
- **THEN** its existing cancellation and settlement are drained before the checkpoint; suspension neither discards its accounting nor reclassifies it as a newly admitted request.

#### Scenario: Settlements remain available
- **WHEN** a suspended or suspending actor receives usage facts, checkpoint queries or physical retirement for the same suspension operation
- **THEN** the existing lifecycle path remains usable without reopening auxiliary admission or recreating interrupted work.

#### Scenario: Request is already expired at admission
- **WHEN** a queued request would start a new node suspension worker but has already exhausted its caller deadline before admission
- **THEN** it returns unconfirmed without spawning a new persistence owner or poisoning the healthy Timeline writer; a later request may retry with a fresh deadline.
