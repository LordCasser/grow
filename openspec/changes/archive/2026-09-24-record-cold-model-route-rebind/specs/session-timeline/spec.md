## ADDED Requirements

### Requirement: Cold model route changes continue durable selection
Fresh sessions SHALL durably record a secret-free baseline for the selected catalog model, reasoning effort, provider-facing model, and backend/endpoint transport identity before admitting prompts. Cold load SHALL compare the last durable model route with the currently resolved catalog route and, when they differ, durably append an exact from/to transition before publishing the actor or admitting sampling. A failed append SHALL prevent publication. A historical Timeline without a route observation SHALL receive an explicit current-route baseline without inventing an earlier route. Resident reconnect SHALL retain its live route. Stored discontinuous transitions SHALL still be rejected.

#### Scenario: Catalog route changes across restart
- **WHEN** the same catalog model ID resolves to a different backend, endpoint, query route, or wire model after a cold restart
- **THEN** the replacement writer records a transition from the last durable route to the selected current route before the resumed actor can sample, and a later reload validates the continuous chain.

#### Scenario: Catalog route is unchanged
- **WHEN** a cold load resolves the same model, effort, provider model, and transport identity as the latest durable observation
- **THEN** it does not append a redundant transition.

#### Scenario: Stored history is already discontinuous
- **WHEN** two existing model observations disagree on the exact preceding route
- **THEN** load rejects the invalid history rather than bridging or rewriting it.
