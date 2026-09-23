## MODIFIED Requirements

### Requirement: Image normalization uses the bounded compute path

Image normalization SHALL run through its existing cancellation-safe worker admission without an inactive process-wide normalization cache or its unused remote activation flag. Image format conversion, integrity checks, size and pixel limits, original-content fallback and attachment metadata SHALL retain their existing behavior. Drop outcomes SHALL remain attributable to individual input indexes but SHALL be grouped by identical reason before rendering; one normalization batch SHALL produce at most one model reminder and one `ImageDropped` update, with each distinct reason rendered once and all affected indexes listed in stable input order.

#### Scenario: Normalize repeated attachments

- **WHEN** attachments with identical content are admitted
- **THEN** each uses the supported normalization path and produces the same valid content and per-attachment metadata without consulting a disabled cache.

#### Scenario: Cancel a normalization waiter

- **WHEN** a caller is cancelled after blocking normalization has started
- **THEN** the running worker retains admission capacity until its actual work completes.

#### Scenario: Re-encoding cannot meet the bound

- **WHEN** normalization cannot produce an encoding under its byte limit
- **THEN** the original attachment and its indexed fallback notice remain available.

#### Scenario: Several images fail for the same reason

- **WHEN** one normalization batch drops multiple images for an identical integrity, dimension or pixel-count reason
- **THEN** the model reminder and `ImageDropped` notes contain one summary line for that reason with every affected image index, and the client receives one NOTICE block rather than one repeated sentence per image.

#### Scenario: Images fail for different reasons

- **WHEN** one normalization batch drops images for more than one reason
- **THEN** each distinct reason appears on one line in first-occurrence order, indexes within each line preserve input order, and the user-attachment and tool-result paths use the same summaries.

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

#### Scenario: Switch to a different model

- **WHEN** sampling switches to an unmarked provider/model pair after another pair used a successful description or OCR projection
- **THEN** the first request to the new pair can use the retained original images, while returning to a marked pair selects the reusable text.

#### Scenario: Switch after an unsupported removal projection

- **WHEN** unresolved images were durably removed from the current Surface and sampling later switches models
- **THEN** request assembly does not resurrect images from immutable Timeline evidence; retrying the original media requires an explicit rewind or new attachment.

#### Scenario: Unrelated image validation failure

- **WHEN** a 400 reports malformed bytes, size, dimensions, format, transparency or policy rather than unconditional model capability
- **THEN** Grow does not mark the pair text-only and does not delete images through the unsupported-model projection.

### Requirement: Visual auxiliary and local OCR fallback

For explicitly unsupported image input, Grow SHALL use an available configured visual auxiliary model to describe images and show `当前模型不支持多模态，调用视觉辅助LLM处理中...`. If the auxiliary model is absent or fails, Grow SHALL show `视觉辅助模型未配置或者调用失败，使用OCR处理中...` and attempt local OCR. A successful description or OCR result SHALL be retained and reused for marked models. Every still-unresolved image group SHALL instead receive a typed unsupported-model projection whose exact replacement is `当前模型不支持多模态，图片已经被删除`. Description, OCR and removal shadows for one Surface revision SHALL commit atomically before the primary request is rebuilt.

#### Scenario: Auxiliary description succeeds

- **WHEN** a configured visual auxiliary model produces a nonempty description
- **THEN** Grow retains that description and retries the primary request with text after the projection ACK.

#### Scenario: Auxiliary unavailable

- **WHEN** the visual auxiliary route is unconfigured or fails
- **THEN** Grow displays the OCR status and attempts local OCR, retaining a successful nonempty result in the description field.

#### Scenario: Both fallbacks fail

- **WHEN** neither visual description nor local OCR can provide text for an image group after the primary model explicitly rejects images
- **THEN** Grow durably replaces that group in the current Surface with one `当前模型不支持多模态，图片已经被删除`, removes its raw image parts and retries the primary request without those images.

#### Scenario: Mixed fallback outcomes

- **WHEN** one rejected request contains groups that are described successfully and groups that remain unresolved
- **THEN** one exact-revision projection atomically attaches descriptions to the successful groups and removes only the unresolved groups, without emitting repeated per-image projection notices.

#### Scenario: Image fallback fails without damaging the session

- **WHEN** unsupported-image removal is acknowledged and the user later sends a text-only follow-up
- **THEN** the new request uses the repaired Surface, contains none of the removed historical images and does not repeat the same capability 400 merely because the raw Timeline evidence still exists.

#### Scenario: Image projection cannot be committed

- **WHEN** projection validation, durable write or acknowledgement fails
- **THEN** Grow does not resubmit an in-memory-only lossy request, reports a typed projection failure, and preserves the original Timeline evidence for exact retry or explicit recovery.
