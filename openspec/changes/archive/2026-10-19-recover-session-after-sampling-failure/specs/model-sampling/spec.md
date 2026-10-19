## ADDED Requirements

### Requirement: Manual continuation survives exhausted provider generation

After a provider generation failure exhausts automatic recovery, a live session SHALL settle the failed Step and Turn and release foreground ownership through the ordinary completion path. A later admitted user prompt SHALL start a new Turn in that same resident session with a fresh logical sampling budget, without restarting Grow or replacing the actor. Valid prior history and all durable spending SHALL remain authoritative; rejected candidate calls and native continuation SHALL NOT enter the next request.

#### Scenario: Three malformed completed responses followed by manual continuation
- **WHEN** retractable delivery and the existing budgets permit three attempts, Chat Completions, Responses or Messages returns three complete identity-valid candidates containing invalid tool-argument JSON, automatic recovery stops, and the provider later returns valid output after the user submits `继续`
- **THEN** the same resident actor completes the new Turn, no rejected tool call executes, and all four actual attempts are accounted once with distinct request-local recovery ownership.

#### Scenario: User input arrives while the failed Turn settles
- **WHEN** a user prompt is admitted while the failed Turn is committing its terminal or releasing foreground ownership
- **THEN** normal input ordering promotes it only after the predecessor's durable terminal, without a lost input, duplicate terminal or manual Idle reset.

#### Scenario: Protocol JSON is malformed
- **WHEN** a provider response fails deterministic protocol JSON parsing and the user sends a later prompt after the provider recovers
- **THEN** the same session can admit the new Turn, while the malformed protocol response retains its existing automatic-retry classification.

### Requirement: Sampling failure preserves settled child output grants

A child output grant SHALL be charged by confirmed attempt usage. A sampling failure alone SHALL NOT mark accurately settled spending incomplete or exhaust an otherwise available grant. Unknown consumption, failed usage settlement and actual exhaustion SHALL continue to close provider admission. A new user Turn SHALL NOT replenish the grant or erase prior spending.

#### Scenario: Known malformed generation leaves output capacity
- **WHEN** a budgeted child exhausts malformed-generation retries, every attempt has confirmed output usage, and grant capacity remains
- **THEN** its usage remains complete and exact, and any subsequently admitted work is limited to the remaining grant.

#### Scenario: Known output exhausts the grant
- **WHEN** rejected attempts consume the child's remaining output grant
- **THEN** no further provider attempt is admitted, including after new user input, and consumption is not falsely classified as unknown merely because sampling failed.

#### Scenario: Failed attempt has unknown usage
- **WHEN** a child's failed attempt has no trustworthy output usage or its usage cannot be confirmed
- **THEN** its grant remains incomplete and closed, and manual continuation cannot bypass the gate.

#### Scenario: Sampler terminal cannot be confirmed
- **WHEN** the ordered sampler terminal does not reach the session's stream-drained acknowledgement barrier
- **THEN** the child grant closes conservatively as incomplete, even if earlier attempt usage was known, and new user input does not reopen it or overwrite existing usage ledger facts.
