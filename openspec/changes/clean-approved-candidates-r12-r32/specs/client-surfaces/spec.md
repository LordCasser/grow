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

### Requirement: Image numbering does not emit unused dedicated metadata
ACP image construction SHALL omit the unused grow.dev/imageDisplayNumber metadata key. Visible image numbers and textual image anchors, image bytes, URIs, durable identities and generic ACP metadata preservation SHALL remain unchanged.

#### Scenario: Recovered image retains its visible identity
- **WHEN** a numbered image placeholder is recovered into an attachment
- **THEN** the textual anchor preserves its number and the attachment preserves its content and URI without emitting dedicated display-number metadata.

#### Scenario: Other metadata survives normalization
- **WHEN** an ACP image with unrelated metadata is normalized or admitted
- **THEN** generic metadata remains preserved by the existing normalization and admission paths.

### Requirement: Scroll diagnostics use the debug command entry
Pager SHALL expose the existing scroll HUD toggle through /debug scroll and SHALL NOT register the redundant /scroll-debug alias. The HUD, environment enablement, FPS and scroll logging SHALL remain available.

#### Scenario: Toggle scroll diagnostics
- **WHEN** the user executes /debug scroll
- **THEN** Pager dispatches the existing ToggleScrollDebugHud action and HUD hints reference /debug scroll.

### Requirement: Announcement metadata omits inactive persistence flag
Announcements SHALL NOT expose the unused persistent field. Dismissibility, expiry and hidden-announcement persistence SHALL retain their existing behavior.

#### Scenario: Default announcement payload
- **WHEN** default announcements are serialized for clients
- **THEN** no persistent field is emitted and supported announcement display controls remain present.
