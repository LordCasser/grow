## ADDED Requirements

### Requirement: Prompt image viewer loading is deferred
Opening an image viewer from a prompt image SHALL return a loading viewer without performing image-file reads or image conversion on the input thread, and use the existing background completion pipeline for both encoded-memory and durable-file sources.

#### Scenario: Real prompt image interaction
- **WHEN** the user opens an image chip that has memory bytes or a durable source path
- **THEN** preserve its display number and enqueue owned background loading using the requesting terminal protocol.

#### Scenario: Viewer closes or reopens before completion
- **WHEN** a background result belongs to an older opening, even of the same attachment
- **THEN** discard it through owner/target validation instead of replacing the current viewer.

#### Scenario: Background source fails
- **WHEN** source reading or decoding fails
- **THEN** settle the current loading viewer through existing failure handling without blocking input on that work.
