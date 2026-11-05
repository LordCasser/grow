## MODIFIED Requirements

### Requirement: Canonical tool metadata versions its closed vocabulary

Canonical `grow/tool` metadata SHALL identify its closed kind and namespace vocabulary with one shared version. The current vocabulary SHALL use version 7 and include the native TODO read kind `plan_read`, distinct from the planning write kind. Subagent enumeration, status and wait SHALL retain their respective kinds in that same version 7 envelope. The checked-in schema and published interface notes SHALL describe the emitted vocabulary. Metadata SHALL remain descriptive and SHALL NOT grant tool authority.

#### Scenario: A subagent observation tool is presented
- **WHEN** canonical metadata is constructed for list_subagent, get_subagent_status or wait_subagent
- **THEN** it emits version 7 and the corresponding subagent_list, subagent_status or subagent_wait kind through the existing shared envelope.

#### Scenario: A native TODO read is presented
- **WHEN** canonical metadata is constructed for todo_read, including a renamed client-facing tool
- **THEN** it emits version 7 with kind plan_read and the actual registered name; the read completes its tool row without emitting an ACP Plan update.

#### Scenario: A closed vocabulary changes
- **WHEN** a new kind is added to the public metadata vocabulary
- **THEN** the emitted shared version and checked-in schema advance together; a stale schema fails the existing regression instead of silently accepting drift.
