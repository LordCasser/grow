## ADDED Requirements

### Requirement: Advisory question responses use current user input admission

Pager SHALL present advisory questions without marking the session as waiting for the user or blocking its composer. A later user response SHALL be admitted as new user input through the ordinary durable input path, with the original question and recommendation attributed as context. It SHALL NOT complete an old tool again or revive a stopped Goal.

#### Scenario: Answer after the original task ended

- **WHEN** the user responds to an advisory question after its original tool and task have ended
- **THEN** the response is submitted to the current owning session as new user input, and the historical tool keeps its single original result.

#### Scenario: User explicitly opens the reply editor

- **WHEN** the user opens an available advisory reply from its history card
- **THEN** the source view's local question editor receives keyboard input immediately, preserves the ordinary composer for restoration, and does not mark the session as awaiting a blocking answer.

#### Scenario: Reply is drawn after an earlier card changes estimated height

- **WHEN** a new reply prompt is pinned at the viewport top and rebuilding the layout changes an earlier advisory card from its measured height to an estimate before the reply body arrives
- **THEN** the preserved position follows the same prompt in the current layout, and the body that fits below it is drawn without additional navigation; a stale absolute offset does not enable a sticky header that hides the body.

#### Scenario: Response admission fails or is retried

- **WHEN** the response fails durable admission or the client retries the same submission
- **THEN** failed text remains available and the stable input identity prevents duplicate accepted input.

#### Scenario: Leader forwards a provisional presentation

- **WHEN** an advisory question with effective blocking=false is presented through the leader
- **THEN** its bounded presentation request uses the current driver path and is not retained or replayed as a pending shared human interaction; other attached clients recover the suggestion through ordinary tool history, while blocking question sharing remains unchanged.

### Requirement: Blocking question preference is persistent

Settings SHALL expose whether the Agent may select nonblocking questions. This preference SHALL default to enabled, persist through the existing raw settings transaction, and govern new question admission after a successful live settings update and after restart. Questions already admitted SHALL retain their effective mode.

#### Scenario: Disable and reload

- **WHEN** the user disables nonblocking questions and later reconnects or restarts
- **THEN** newly admitted Agent questions remain blocking until the user changes the preference again.
