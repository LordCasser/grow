## ADDED Requirements

### Requirement: Hydrate reasoning effort before actor publication
Cold session load SHALL initialize the actor sampling effort from the persisted selection before publishing the actor or admitting model controls. Reopening an unchanged selection SHALL NOT append a model transition from a configuration default that differs from the durable selection.

#### Scenario: Saved effort differs from model default
- **WHEN** a valid session ends with effort high and the configured model default is max
- **THEN** cold load initializes the actor with high, does not record max → high for hydration, and a later resume preserves continuity.

#### Scenario: User switches after loading
- **WHEN** a loaded session accepts an actual effort or model change
- **THEN** the new model observation starts from the preceding durable selection and a subsequent resume succeeds.

#### Scenario: Existing discontinuity
- **WHEN** a stored model observation does not continue the preceding durable selection
- **THEN** load continues to reject the invalid history rather than silently rewriting or skipping it.

#### Scenario: Persisted unset effort
- **WHEN** the saved effort is unset and the model configuration now has a default effort
- **THEN** cold load preserves the unset value rather than substituting the default.

#### Scenario: Unsupported historical effort
- **WHEN** the configured model no longer admits a saved explicit effort
- **THEN** cold load reports that incompatibility before publishing an actor, without inventing a replacement selection.

#### Scenario: Resident reconnect
- **WHEN** a client reconnects to a resident session
- **THEN** load retains the live actor sampling selection, including unset effort, without submitting a model-control request for hydration.
