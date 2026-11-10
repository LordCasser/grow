## MODIFIED Requirements

### Requirement: Planning context is constructed by Grow without user-chat insertion

The primary Agent SHALL decide when to update its TODOs. Ordinary primary and subagent request assembly SHALL NOT automatically read a plan or append planning snapshots or confirmation prompts. Explicit authorized plan reads remain available. Only a committed effective parent TODO change SHALL trigger a planning notification, and only for affected directly linked subagents. Grow SHALL append each notification at the receiving subagent's next safe sampling boundary through the existing typed delivery path without requiring either Agent to poll for plan changes.

#### Scenario: First child request locates its parent step
- **WHEN** a newly admitted or resumed child makes its first provider request without a parent change event
- **THEN** it uses its authored task with the canonical delegation binding retained by the runtime and no automatic planning snapshot or parent query.

#### Scenario: Repeated requests refresh plan context
- **WHEN** a primary Agent makes repeated requests with a committed local plan, including after a TODO update
- **THEN** request assembly adds no plan summary or confirmation prompt; plan details remain available through explicit tool calls.

#### Scenario: Parent is unavailable
- **WHEN** a bound child explicitly reads its parent plan and the owner is unavailable
- **THEN** the existing read tool reports the unavailable owner. Ordinary sampling has no dependency on that read.

#### Scenario: Repeated snapshots during unchanged work
- **WHEN** a child makes multiple requests without a new parent TODO change affecting its link
- **THEN** no automatic planning snapshot or new planning notification is added.

#### Scenario: Linked parent TODO changes
- **WHEN** a parent commits an effective objective change or stop for a directly linked child
- **THEN** only affected children receive the existing typed notification at their safe sampling boundary with the exact change identity and its guidance. Later requests may retain that item as history without repeating delivery.
