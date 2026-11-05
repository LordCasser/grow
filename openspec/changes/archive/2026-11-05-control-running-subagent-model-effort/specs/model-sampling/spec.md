## MODIFIED Requirements

### Requirement: Native model control uses the session step boundary

Grow SHALL expose a native `set_subagent_model` tool accepting a required subagent_id and optional model/effort for an active directly owned child. It SHALL replace the caller-self `set_session_model` tool without an alias. It SHALL require at least one selection and reject unknown fields, malformed effort, unavailable/hidden/unselectable models and unsupported effort. Model-only SHALL use user-selection effort defaults. Effort-only SHALL compose with the target actor's newest desired model. It SHALL return queued only after target actor admission, without claiming application or waiting for that child's current Step. Accepted changes SHALL use the same closed-Step application, latest-wins ordering, durable route/native-continuation/failure boundaries as user controls. Rejection SHALL not mutate parent or child selection. Parent selection SHALL remain unchanged on success.

#### Scenario: Tool switches the next request
- **WHEN** a parent calls the native tool for its running direct child
- **THEN** admission returns queued without interrupting/restarting the child's current provider attempt, and the child's next admitted request uses the applied selection while the parent retains its route.

#### Scenario: Effort patch follows a pending user model
- **WHEN** a user model change is pending on the child and the parent tool supplies only effort
- **THEN** the target actor composes effort with that newest desired model without restoring its older route.

#### Scenario: Invalid selection has no mutation
- **WHEN** selections are absent, malformed/unsupported or target model is unknown, hidden or unselectable
- **THEN** the tool fails explicitly and neither parent nor child receives a substituted selection or provider request.

#### Scenario: Application fails or a later control supersedes it
- **WHEN** a child's admitted selection fails application or a newer target control supersedes it
- **THEN** existing authoritative target control semantics record the outcome and queued does not become evidence of application.

#### Scenario: Resume follows a runtime-controlled child
- **WHEN** the child completes after an applied parent runtime selection and a later ordinary resume omits selections
- **THEN** it inherits that child's latest durable model/effort, including unset effort, without rewriting source birth facts.
