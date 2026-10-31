## ADDED Requirements

### Requirement: Subagent rejection preserves its actual policy cause
Subagent validation and dispatch SHALL distinguish unknown definition IDs, explicit global toggle disablement, parent Agent delegation restrictions and unavailable validation. Only an explicit false toggle SHALL be described as disabled by `[subagents.toggle]`. Parent restriction diagnostics SHALL identify the parent Agent and current allowed full IDs. Definition existence SHALL be checked before parent filtering; path-derived IDs SHALL NOT gain implicit basename aliases.

#### Scenario: Enabled type excluded by the parent
- **WHEN** an existing type has toggle true but the parent allow/deny policy forbids it
- **THEN** preflight and dispatch reject it as a parent policy restriction without claiming global disablement, and no child is spawned.

#### Scenario: Short name does not identify the nested definition
- **WHEN** only a nested path ID exists and the request uses its bare frontmatter name
- **THEN** validation reports an unknown ID even under a restricted parent and provides only allowed available full IDs.

#### Scenario: Explicit global disablement
- **WHEN** an existing type has toggle false
- **THEN** validation rejects it with the global toggle cause; automatic permission approval does not override it.
