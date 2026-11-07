## ADDED Requirements

### Requirement: MCP calls revalidate the originating client after preparation

A session-bound MCP tool invocation SHALL revalidate its captured originating client and current tool eligibility after asynchronous initialization or reconnect preparation and before dispatching `tools/call`. A removed, replaced or disabled client/tool SHALL fail without dispatching to that client or a same-name replacement. An inherited client SHALL additionally satisfy the existing live upstream client binding and tool ceiling. The invocation SHALL retain its admitted deadline and SHALL NOT gain replay or transport-reset authority from this rejection.

#### Scenario: Eligibility changes during model initialization

- **WHEN** an admitted model tool waits for initialization and the session removes or replaces the server or disables its tool before preparation completes
- **THEN** releasing preparation causes an explicit pre-dispatch failure and both the originating and replacement services observe zero tool calls from that invocation.

#### Scenario: A direct call waits through configuration change

- **WHEN** a direct session MCP invocation captures a client and its initialization completes after removal, replacement or disablement
- **THEN** it revalidates through the same current session state and fails without sending to either client or silently choosing a new endpoint.

#### Scenario: An upstream revocation precedes child dispatch

- **WHEN** a child still holds an inherited client while an ancestor has revoked its tool or replaced the authorized binding during preparation
- **THEN** the pending invocation fails even if the child map has not reconciled, and no tool call is dispatched to the retired or replacement service.

#### Scenario: The captured client remains eligible

- **WHEN** preparation completes and the captured client, tool and applicable inherited ceiling remain current
- **THEN** the invocation sends exactly once to that client within the original admitted deadline; unrelated catalog changes do not extend that deadline or cause replay.

#### Scenario: Revocation follows an already dispatched request

- **WHEN** the request was already dispatched before a later revocation or uncertain transport failure
- **THEN** the existing originating-peer cancellation and uncertain-outcome semantics remain authoritative, without claiming rollback or automatically sending the operation again.
