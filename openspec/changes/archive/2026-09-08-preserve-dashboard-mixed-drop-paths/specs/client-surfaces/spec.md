## ADDED Requirements

### Requirement: Dashboard preserves mixed drop paths
Dashboard dispatch and peek SHALL insert recognized image and ordinary-file drop entries in source order for bracketed paste, paste-key text, and deferred file URL completion.

#### Scenario: Image and ordinary file share a paste
- **WHEN** a recognized batch contains an image and an ordinary file path
- **THEN** attach the image and insert the ordinary path as text in the same target, without dropping that path through image-only filtering.

#### Scenario: Image cap does not suppress file text
- **WHEN** the target rejects an image because its attachment cap is reached and the batch also contains an ordinary path
- **THEN** keep the existing image rejection feedback and still insert the ordinary path.

#### Scenario: Explicit file URL cannot be loaded as an image
- **WHEN** the shared classifier recognizes an explicit file URL as an ordinary path, including a missing image file
- **THEN** insert that path as text and report successful handling without adding an image attachment.

#### Scenario: Question and stale target guards
- **WHEN** a peek is in question mode or a deferred target has become stale
- **THEN** preserve existing text-only admission and deferred discard rules rather than routing mixed entries into the hidden reply.
