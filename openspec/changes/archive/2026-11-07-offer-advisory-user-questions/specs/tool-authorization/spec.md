## ADDED Requirements

### Requirement: Questions can offer provisional recommendations without blocking

The Agent MAY explicitly select a nonblocking question when every question has an unambiguous recommended option and proceeding under that provisional choice is within existing authorization. The tool SHALL complete after bounded presentation admission and identify its result as the Agent's recommendation, not a user answer. Required user permission or decision gates SHALL remain blocking. Omitted per-call selection SHALL use the existing blocking form.

#### Scenario: Recommendation permits independent progress

- **WHEN** nonblocking mode is admitted and every recommended label matches an explicit option
- **THEN** the model may continue without a user response, and the returned recommendation grants no new user authority.

#### Scenario: Ambiguous recommendation

- **WHEN** effective nonblocking mode has a missing or unmatched recommendation
- **THEN** the call fails validation before publication rather than guessing an option.

#### Scenario: User requires blocking questions

- **WHEN** the effective persistent preference disables nonblocking questions
- **THEN** every newly admitted question uses the existing response-waiting form even if the Agent requested nonblocking mode.
