## ADDED Requirements

### Requirement: Agent configuration modal discovery does not block the event path

Pager SHALL display the agent configuration modal immediately while its filesystem-backed catalog loads in a bounded background worker. A timed-out or failed scan SHALL report failure in the modal without making the UI wait for the underlying filesystem call to finish. A result SHALL apply only to the modal instance that requested it.

#### Scenario: Discovery stalls
- **WHEN** opening the modal starts a filesystem scan that remains blocked
- **THEN** the modal opens and remains dismissible, other UI events remain responsive, and the request eventually reports a load error while the worker keeps its execution permit until exit.

#### Scenario: Modal closes and reopens before an older scan completes
- **WHEN** a first modal instance is closed, another instance opens, and the first scan later returns
- **THEN** the old result does not overwrite the newer modal's state.

#### Scenario: Configuration editor returns
- **WHEN** an external editor completes while the modal is still open
- **THEN** catalog refresh runs through the same bounded worker and retains modal identity.
