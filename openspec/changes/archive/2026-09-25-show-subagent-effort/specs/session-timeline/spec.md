## ADDED Requirements

### Requirement: Subagent spawn projections retain effective reasoning effort

The subagent spawn Grow projection SHALL carry the effective reasoning effort selected for the child when one exists. Live publication and reconnect projection SHALL derive it from the same durable spawn fact, without substituting the model catalog's default effort.

#### Scenario: Child overrides its model's default effort

- **WHEN** a child is launched with an effective effort different from the catalog default
- **THEN** both its live spawn notification and a later projection from the durable spawn fact identify that effective effort.

#### Scenario: Model has no effort

- **WHEN** a child has no effective reasoning effort
- **THEN** the spawn projection omits the optional effort field.
