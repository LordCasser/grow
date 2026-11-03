## ADDED Requirements

### Requirement: Loading history preserves session navigation

While history replay is incomplete, Pager SHALL keep registered Dashboard navigation and previous/next session actions available, including attached frame navigation clicks. Navigation SHALL retain the loading owner without admitting editing, prompts, or session control commands. Completion SHALL update its own page; failure of a temporary load page SHALL retire that owner through draft checkpointing and apply its original return surface only if still foreground.

#### Scenario: Navigate while a session loads
- **WHEN** a loading session receives registered Dashboard/previous/next navigation or an attached frame navigation click
- **THEN** navigation proceeds while replay and its draft remain owned by the original page.

#### Scenario: Background load fails
- **WHEN** the user has navigated elsewhere before a temporary load page fails
- **THEN** the failed page retires without moving the current focus or discarding its retained draft.

#### Scenario: Ordinary input during replay
- **WHEN** text, paste, prompt submission, or Ctrl+X session commands arrive during replay
- **THEN** no input is admitted and the draft is unchanged.
