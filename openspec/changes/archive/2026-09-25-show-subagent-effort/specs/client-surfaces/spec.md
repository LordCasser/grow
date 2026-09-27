## ADDED Requirements

### Requirement: Subagent model labels show effective reasoning effort

Pager SHALL display a known effective child effort immediately after its model as `model (effort)` in both the Tasks pane row and the opened subagent title. It SHALL omit the suffix when no effort is known and SHALL update both surfaces when the child's authoritative model or effort changes.

#### Scenario: Open a running child

- **WHEN** a child with model `bigmodel/glm-5.3` and effort `max` appears in Tasks and its detail view is opened
- **THEN** both model labels include `bigmodel/glm-5.3 (max)` without adding a separate column.

#### Scenario: Child changes effort

- **WHEN** an authoritative child model change selects a new effort
- **THEN** the parent Tasks row and opened title display the new model and effort.

#### Scenario: Child has no known effort

- **WHEN** a child spawn has no effective reasoning effort
- **THEN** both labels show its model without empty parentheses.
