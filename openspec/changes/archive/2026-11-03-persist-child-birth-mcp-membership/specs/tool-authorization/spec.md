## ADDED Requirements

### Requirement: Cold child restore retains durable birth MCP membership
A child Spawn SHALL durably retain the known MCP server membership of the filtered live grant frozen for canonical Spawn admission before child execution. Cold restoration SHALL require that fact and intersect it with current eligible inherited services and the direct parent ceiling. Retained servers SHALL bind to the current process incarnation and obtain new permits; current configuration SHALL NOT add members to the original child or its descendants. Missing birth membership SHALL fail before tree execution is released, while a known empty set SHALL remain empty.

#### Scenario: Profile and parent pool become wider offline
- **WHEN** a child born with a narrow MCP pool is cold restored after its profile or parent pool admits more services
- **THEN** only surviving birth members may be rebound, used or delegated, with fresh incarnation/generation authorization.

#### Scenario: Current authority becomes narrower
- **WHEN** a birth member is unavailable or no longer permitted by the current parent/profile
- **THEN** that member is unavailable to the restored child, and other retained members are not expanded.

#### Scenario: Birth membership is missing or empty
- **WHEN** a cold child lacks a durable membership fact or has a recorded empty birth set
- **THEN** missing facts block execution before preparation, while the known empty set permits restoration without granting any MCP server.

#### Scenario: Parent transports change during initialization
- **WHEN** the parent removes a birth member, replaces its client identity or adds another server after the child admission grant is frozen
- **THEN** the grant is not resampled during actor setup; calls intersect the frozen IDs with live authority, and the old child cannot acquire the replacement or added transport.
