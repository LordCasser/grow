## ADDED Requirements

### Requirement: Provider recovery preserves Goal pause and spending

An exhausted provider failure in a Goal-internal Turn SHALL use the durable Goal pause path and stop automatic continuation. Ordinary user input in the resident session SHALL remain possible without implicitly restarting the paused Goal. Explicit restart SHALL use the existing Goal activation path and preserve Goal identity, recorded usage and budget admission rules.

#### Scenario: Provider recovers while the Goal is paused
- **WHEN** an internal Goal Turn exhausts malformed-generation retries and the provider becomes healthy
- **THEN** the Goal remains paused with no automatic provider request until an authorized activation is admitted.

#### Scenario: User manually continues a paused Goal session
- **WHEN** the user sends `继续` in the same resident session after the Goal failure
- **THEN** the user Turn can complete while the Goal remains paused, and an explicit Goal restart can subsequently reactivate the same Goal with its accumulated spending intact.

#### Scenario: Exact budget prevents restart
- **WHEN** the Goal budget is exhausted or exact-budget usage remains unresolved
- **THEN** ordinary user interaction remains available outside that stopped Goal without charging or replenishing its budget or clearing the uncertainty, and an explicit restart does not dispatch Goal-owned provider work until the existing resolution path permits activation.
