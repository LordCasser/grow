## ADDED Requirements

### Requirement: Initial Responses empty text parts may omit empty annotations

For a known `response.content_part.added` event whose part is `output_text`, whose explicit text is empty and whose annotations key is absent, the sampler SHALL supply an empty annotations array only to the typed-decode copy. Original evidence and existing completed-response validation SHALL remain authoritative. This exception SHALL NOT repair done or terminal snapshots or hide other known-event decode errors.

#### Scenario: Initial empty text part omits annotations

- **WHEN** a Responses stream opens an empty output_text part without annotations and later supplies a valid completed response
- **THEN** the initial event decodes with an empty annotations list, later text and completion are processed normally, and the original raw event remains unchanged.

#### Scenario: Existing annotations are present

- **WHEN** the initial part already includes an empty or nonempty valid annotations array
- **THEN** the sampler preserves the provided annotations exactly and does not replace them with an inferred list.

#### Scenario: Malformed or unsupported missing-field shape

- **WHEN** annotations is null or a wrong type, the initial text is nonempty with annotations absent, a done or terminal snapshot omits annotations, or another known field is invalid or duplicated
- **THEN** the known event remains rejected by its existing typed or protocol validation and is not repaired by the initial-empty-text exception.

#### Scenario: No valid completion follows

- **WHEN** an accepted initial empty text part is followed by EOF or an existing terminal or identity validation failure
- **THEN** the sampler does not report a completed response merely because the initial annotations list was inferred.
