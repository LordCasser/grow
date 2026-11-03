## ADDED Requirements

### Requirement: Session close retires runtime before its local view

Confirmed close of a bound session SHALL use the existing terminal session-close control and retire its local view only after successful acknowledgement. Closing SHALL retain the saved history and checkpoint the latest local draft through the existing draft owner. Failure SHALL retain the target identity and an actionable retry without claiming success. Async results SHALL apply only to the requesting AgentId, session identity and binding epoch. The final local session SHALL be closable with Dashboard or Welcome as the remaining surface. A busy detail SHALL stop its foreground or background activity before closing; an in-flight startup SHALL retain its original owner until its identity is known.

#### Scenario: Close the last idle session
- **WHEN** the user confirms closing the only local bound session
- **THEN** the existing runtime close is requested, the local view remains until confirmation, and success leaves Dashboard or Welcome without deleting history.

#### Scenario: Closure is not confirmed
- **WHEN** the runtime close fails or its transport response is unavailable
- **THEN** the page retains its original identity and closing state for an explicit retry; no successful close is shown.

#### Scenario: Late response belongs to a replaced binding
- **WHEN** a close response arrives after the view has rebound to another session or binding epoch
- **THEN** it cannot remove or change the replacement view.

#### Scenario: Background work is still active
- **WHEN** an idle foreground still owns running children, Goal or Workflow work
- **THEN** the confirmed detail stop uses the existing activity controls and does not drop its view as if work had settled.
