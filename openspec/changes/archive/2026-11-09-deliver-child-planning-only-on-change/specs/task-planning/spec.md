## MODIFIED Requirements

### Requirement: Planning context is constructed by Grow without user-chat insertion

Grow SHALL restrict automatic request-only planning summaries to the primary Agent's own committed plan. The summary SHALL be bounded and counted in effective request pressure without becoming an authored prompt or persisted chat row. Subagent request assembly SHALL NOT automatically query or project the parent or local plan. Effective parent TODO changes SHALL be appended to the receiving subagent's context at its next safe sampling boundary through the existing typed delivery path. Explicit authorized plan reads remain available.

#### Scenario: First child request locates its parent step
- **WHEN** a newly admitted or resumed child makes its first provider request without a parent change event
- **THEN** it uses its authored task with the canonical delegation binding retained by the runtime and no automatic planning snapshot or parent query.

#### Scenario: Repeated requests refresh plan context
- **WHEN** a primary Agent makes repeated requests after its local plan changes
- **THEN** its bounded local summary reflects the committed local plan without accumulating synthetic history or altering retrieved-memory persistence.

#### Scenario: Parent is unavailable
- **WHEN** a bound child explicitly reads its parent plan and the owner is unavailable
- **THEN** the existing read tool reports the unavailable owner. Ordinary sampling has no dependency on that read.

#### Scenario: Repeated snapshots during unchanged work
- **WHEN** a child makes multiple requests without a new parent TODO change
- **THEN** no automatic planning snapshot or new planning notification is added.

#### Scenario: Linked parent TODO changes
- **WHEN** an effective objective change or stop for a directly linked child is durably received
- **THEN** the existing typed notification is appended at the child's safe sampling boundary with the exact change identity and its guidance. Later requests may retain that item as history without repeating delivery.
