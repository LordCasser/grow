## ADDED Requirements

### Requirement: Restore feedback does not require an unused degree cache
Pager SHALL present code-restoration summaries and failures without storing restore_degree in AgentSession or forwarding it through internal completion actions. Shared RestoreDegree wire types, parsing and validation SHALL remain intact.

#### Scenario: Loaded or forked restoration result
- **WHEN** session loading or worktree forking completes with a code-restoration summary
- **THEN** Pager displays the existing success or failure feedback and performs the existing follow-up actions without caching restore degree.

### Requirement: Terminal client excludes the entertainment game
Pager SHALL NOT register or run the GBOOM game, its renderer, game simulation clock or exclusive keyboard enhancement layer. Shared image viewing, terminal keyboard restoration, animation clocks, focus handling and debug diagnostics SHALL remain available.

#### Scenario: Command registry without game
- **WHEN** Pager builds its built-in command registry
- **THEN** no gboom command is registered and supported debug commands remain registered.

#### Scenario: Input and terminal operation after game removal
- **WHEN** a user views an image, changes focus or closes the terminal UI
- **THEN** existing image controls and common terminal/input lifecycle behavior remain intact without game-owned state or clocks.
