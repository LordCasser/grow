## ADDED Requirements

### Requirement: Canonical tool metadata versions its closed vocabulary

Canonical `grow/tool` metadata SHALL identify its closed kind and namespace vocabulary with one shared version. The subagent enumeration, status and wait kinds SHALL be emitted as version 6, distinct from the version 5 vocabulary shipped in v2.5.5. The checked-in schema and published interface notes SHALL describe the emitted vocabulary. Metadata SHALL remain descriptive and SHALL NOT grant tool authority.

#### Scenario: A subagent observation tool is presented
- **WHEN** canonical metadata is constructed for list_subagent, get_subagent_status or wait_subagent
- **THEN** it emits version 6 and the corresponding subagent_list, subagent_status or subagent_wait kind through the existing shared envelope.

#### Scenario: A closed vocabulary changes
- **WHEN** a new kind is added to the public metadata vocabulary
- **THEN** the emitted shared version and checked-in schema advance together; a stale schema fails the existing regression instead of silently accepting drift.
