## ADDED Requirements

### Requirement: Inline-media loading has bounded worker and payload admission
Pager SHALL perform inline-media filesystem reads and image preparation off the UI thread. It SHALL admit at most two such workers process-wide, at most two pending paths per AgentView, read at most 16 MiB of source bytes per path, and retain at most 16 MiB of prepared bytes per completion. Preparation may use its existing transient 100 MB conversion-output allowance per worker; output over 16 MiB SHALL be discarded before mailbox admission. A request rejected only because of worker or pending saturation SHALL remain eligible for a later render retry. Genuine read or preparation failures SHALL retain the existing bounded rename-race retry and failed-path behavior. A result from a mailbox detached at a session boundary SHALL NOT enter the replacement session's cache.

#### Scenario: Worker capacity is saturated
- **WHEN** an inline-media path is requested while both worker permits are occupied
- **THEN** Pager does not block the UI, does not mark the path pending or failed, and may request it again on a later render

#### Scenario: Per-view pending capacity is saturated
- **WHEN** an AgentView already has two inline-media paths pending
- **THEN** another path is not admitted or marked failed and remains eligible for a later render retry

#### Scenario: Source or prepared image exceeds the byte limit
- **WHEN** source bytes or prepared image bytes exceed 16 MiB
- **THEN** Pager does not retain those bytes in a completion mailbox or CPU cache

#### Scenario: Session changes during inline-media loading
- **WHEN** a worker completes after its AgentView has reset the inline-media loader
- **THEN** the completion remains in the detached old mailbox and cannot populate the replacement session cache
