## ADDED Requirements

### Requirement: Fullscreen side-question answers use a large popup

In the fullscreen TUI, an active `/btw` request SHALL display its question, loading, answer, or error in a popup sized like the fullscreen Plan preview instead of reserving a short inline transcript strip. The popup SHALL keep answer Markdown, links, text selection, scrolling and the existing dismissal behavior. Covered transcript controls and links SHALL NOT receive popup clicks. The normal prompt SHALL remain usable.

#### Scenario: A long answer arrives

- **WHEN** `/btw` returns a long Markdown answer in the fullscreen TUI
- **THEN** the answer appears in a large popup with a scrollable body and the transcript does not lose rows to an inline panel.

#### Scenario: An answer is dismissed and revisited

- **WHEN** the user dismisses a completed `/btw` popup and double-clicks its saved transcript row
- **THEN** the full answer opens in a read-only popup of the same size, with the original question and Markdown content available.

#### Scenario: A popup covers a transcript link or control

- **WHEN** the user clicks within the `/btw` popup over an underlying transcript target
- **THEN** only the popup target handles the click.
