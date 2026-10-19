## MODIFIED Requirements

### Requirement: Session image descriptions retain original media

Grow SHALL retain original image information as immutable Timeline evidence. Sampling SHALL send original images for an unmarked canonical provider/model pair. Only a confirmed unsupported-image failure SHALL mark that pair in the current session and trigger a durable `ImageProjection`; unrelated request failures SHALL NOT do so. A successful description or OCR projection SHALL retain original images alongside reusable text in the current Surface. When no textual fallback can be produced after a confirmed rejection, an acknowledged unsupported-image projection SHALL remove the unresolved images from the current Surface and leave the canonical replacement text without altering the original Timeline message.

#### Scenario: First request for a model

- **WHEN** a session sends an image to an unmarked provider/model pair
- **THEN** the request includes original image content rather than a preemptive description or deletion.

#### Scenario: Model rejects image input

- **WHEN** the primary model explicitly rejects a request that contains image input
- **THEN** the session records that provider/model pair, commits one exact-revision image projection, and retries only after the projection is durably acknowledged.

#### Scenario: GLM-style multimodal rejection

- **WHEN** an image-bearing request receives HTTP 400 with `InvalidParameter: glm-5.2 is not a multimodal model`
- **THEN** the shared classifier treats it as an unconditional image-input rejection rather than an ordinary terminal request failure.

#### Scenario: Text-only content type rejection

- **WHEN** an image-bearing request receives HTTP 400 with the explicit claim `messages.content.type 参数非法，取值范围 ['text']` (or the same list using double quotes, optionally followed by whitespace or terminal punctuation)
- **THEN** the shared classifier treats it as an unconditional image-input rejection and the session follows the existing visual auxiliary/OCR and durable image-projection flow before rebuilding the request.

#### Scenario: Switch to a different model

- **WHEN** sampling switches to an unmarked provider/model pair after another pair used a successful description or OCR projection
- **THEN** the first request to the new pair can use the retained original images, while returning to a marked pair selects the reusable text.

#### Scenario: Switch after an unsupported removal projection

- **WHEN** unresolved images were durably removed from the current Surface and sampling later switches models
- **THEN** request assembly does not resurrect images from immutable Timeline evidence; retrying the original media requires an explicit rewind or new attachment.

#### Scenario: Unrelated image validation failure

- **WHEN** a 400 reports malformed bytes, size, dimensions, format, transparency or policy rather than unconditional model capability
- **THEN** Grow does not mark the pair text-only and does not delete images through the unsupported-model projection.
