## RENAMED Requirements

- FROM: `### Requirement: Initial Responses empty text parts may omit empty annotations`
- TO: `### Requirement: Responses text snapshots may omit empty annotations`

## MODIFIED Requirements

### Requirement: Responses text snapshots may omit empty annotations

The Responses SSE decoder SHALL supply an empty annotations array to the typed-decode copy when an `output_text` part has a string `text` and omits the `annotations` key in `response.content_part.done`, an assistant message in `response.output_item.done`, or an assistant message in a terminal response snapshot. In `response.content_part.added` the default SHALL apply only when the explicit text is empty. Original evidence and the SDK and protocol checks for all other members SHALL remain authoritative. The annotation default SHALL compose with the existing terminal usage-detail completion.

#### Scenario: Initial empty text part omits annotations

- **WHEN** a Responses stream opens an empty output_text part without annotations and later supplies a valid completed response
- **THEN** the decoder supplies an empty annotations list for the part and retains the original raw event.

#### Scenario: Closing snapshots omit annotations

- **WHEN** part-done, item-done and terminal snapshots contain text with absent annotations, including a terminal that also omits usage details
- **THEN** a valid stream completes with its original text and usage semantics, and the raw SSE evidence retains all omissions.

#### Scenario: Existing annotations are present

- **WHEN** an applicable text part contains an annotations key with an array, null, a wrong type or a duplicate key
- **THEN** the decoder preserves the supplied members and the SDK validates them as before.

#### Scenario: Malformed or unsupported missing-field shape

- **WHEN** an initial part is nonempty with annotations absent, or an otherwise eligible event has conflicting, missing or invalid required fields
- **THEN** the known event remains rejected by typed or protocol validation.

#### Scenario: No valid completion follows

- **WHEN** an accepted text part is followed by EOF or an existing terminal or identity validation failure
- **THEN** the annotation default does not admit a completed response.
