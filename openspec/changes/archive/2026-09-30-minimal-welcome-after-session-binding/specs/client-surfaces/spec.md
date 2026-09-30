## ADDED Requirements

### Requirement: Minimal welcome card belongs to a bound session epoch

Minimal mode SHALL commit a new-session welcome card after the root Agent has a bound session ID, so an intervening placeholder-to-bound visible-owner transition cannot erase the card. The card SHALL appear once for a cold new session and once for each subsequent `/new` session, before that session's committed conversation blocks.

#### Scenario: Cold session binding

- **WHEN** Minimal creates a placeholder Agent and draws before its session ID is bound
- **THEN** the welcome card remains pending; after binding, it commits into the bound owner's native terminal epoch and remains in scrollback as the first turn grows.

#### Scenario: New session after a completed turn

- **WHEN** the user enters `/new` after an earlier session has committed output
- **THEN** the new card commits after the new session binds, while the earlier session's terminal history remains available.
