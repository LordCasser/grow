## ADDED Requirements

### Requirement: MCP cancellation dispatch has a bounded Grow owner

Grow's best-effort notification task for a dropped or timed-out MCP tool request SHALL wait at most three seconds for notification queueing and send confirmation. That separate dispatch budget SHALL NOT extend the original tool-call deadline. The task SHALL target only the originating service and request identity, send no duplicate retries, and distinguish unconfirmed or failed delivery from confirmed notification send. Stopping the Grow wait SHALL NOT claim that SDK transport work was withdrawn or remote side effects were reversed.

#### Scenario: Peer stops reading cancellation

- **WHEN** a tool request ends and its cancellation notification cannot complete because the transport is blocked
- **THEN** the original call retains its deadline and the Grow dispatch task ends its own wait within three seconds, reporting unconfirmed delivery.

#### Scenario: Service replacement after request admission

- **WHEN** a request's client has been replaced before its old call is dropped
- **THEN** cancellation uses the old service and exact old request id without sending that id to the replacement.

#### Scenario: Completed request or duplicate cancellation

- **WHEN** a request completes normally or an abandoned request has already caused its one cancellation attempt
- **THEN** normal completion sends no cancellation and the abandoned request does not retry its notification.
