## ADDED Requirements

### Requirement: Delivery policy is independent of execution preemption
Grow SHALL separate message admission and boundary eligibility from request preemption and Turn cancellation. A context-only delivery SHALL NOT itself cancel an active model request, interrupt a tool, or request an idle Turn. Existing user steer SHALL retain its explicit product composition of exact-Turn delivery and same-Turn request soft preemption; naming an operation steer SHALL NOT imply that every next-Step producer has preemption authority.

#### Scenario: Passive information arrives during sampling
- **WHEN** runtime admits context-only information while a provider request is running
- **THEN** the request continues unchanged and the information is eligible only at a subsequent authorized sampling boundary, without setting the user-interjection or parent-interrupt signal.

#### Scenario: Existing user steer remains explicit
- **WHEN** a user steer is admitted for the current exact Turn
- **THEN** Grow retains its existing request soft-preemption and same-Turn delivery behavior, without applying that execution policy to passive notifications.

#### Scenario: Exact target expires during admission
- **WHEN** an explicit user steer's named Turn has already closed before Allow
- **THEN** the existing stale-target rejection applies rather than silently converting it into a new Turn; an already-allowed residual steer continues to use its existing durable FIFO reroute.

### Requirement: Passive context never becomes human input authority
Context-only delivery SHALL preserve its runtime producer identity and SHALL NOT run HumanIntent admission, acquire PermissionEvidence, change Behavior, reorder user FIFO, bypass an edit hold, or create an implicit cancellation. Provider role representation SHALL NOT change these properties.

#### Scenario: Consultation contains approval wording
- **WHEN** a passive consultation answer says an action is allowed
- **THEN** the text is available as attributed consultation evidence without becoming human permission, a Behavior transition or an execution-control command.

#### Scenario: User queue is held
- **WHEN** passive context is pending while the head user FIFO item has a confirmed edit hold
- **THEN** the context remains available without opening a notification Turn or overtaking the held item through idle arbitration.
