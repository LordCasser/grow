## ADDED Requirements

### Requirement: Textual image placeholders do not load files

Pager and Shell SHALL treat a numbered image placeholder in text as an anchor only. They SHALL NOT infer an image attachment by opening the path embedded in that text. Image bytes SHALL enter through an explicit attachment admission path.

#### Scenario: Placeholder without attachment
- **WHEN** submitted text contains `[Image #N: <path>]` without a corresponding image attachment
- **THEN** the model-visible text retains the numbered anchor without the path, and the path is not opened to synthesize an attachment.

#### Scenario: Placeholder with attachment
- **WHEN** submitted text contains a numbered image placeholder and the client submits image content as an attachment
- **THEN** the numbered anchor and attachment content remain available without re-reading the path from placeholder text.

## MODIFIED Requirements

### Requirement: Image numbering does not emit unused dedicated metadata
ACP image construction SHALL omit the unused grow.dev/imageDisplayNumber metadata key. Visible image numbers and textual image anchors, image bytes, URIs, durable identities and generic ACP metadata preservation SHALL remain unchanged.

#### Scenario: Recovered image retains its visible identity
- **WHEN** a numbered image anchor accompanies an explicit attachment submitted or restored by the client
- **THEN** the textual anchor preserves its number and the attachment preserves its content and URI without emitting dedicated display-number metadata.

#### Scenario: Other metadata survives normalization
- **WHEN** an ACP image with unrelated metadata is normalized or admitted
- **THEN** generic metadata remains preserved by the existing normalization and admission paths.

## REMOVED Requirements

### Requirement: Placeholder image caps bound actual reads

Removed because textual placeholder paths no longer trigger file reads. Explicit drag, paste and image attachment paths retain their own byte budgets.

### Requirement: Orphan image reads honor remaining recovery budget

Removed because orphan recovery is removed. No aggregate recovery read occurs from placeholder text.
