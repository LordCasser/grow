## ADDED Requirements

### Requirement: Session image descriptions retain original media
Grow SHALL retain original image information alongside a reusable textual description. Sampling SHALL send original images for an unmarked canonical provider/model pair. Only a confirmed unsupported-image failure SHALL mark that pair in the current session and trigger text fallback; unrelated request failures SHALL NOT do so.

#### Scenario: First request for a model
- **WHEN** a session sends an image to an unmarked provider/model pair
- **THEN** the request includes original image content rather than a preemptive description.

#### Scenario: Model rejects image input
- **WHEN** the primary model explicitly rejects image input
- **THEN** the session records that provider/model pair and retries with a description while retaining the original image.

#### Scenario: Switch to a different model
- **WHEN** sampling switches to an unmarked provider/model pair after another pair used text fallback
- **THEN** the first request to the new pair again includes original images; returning to a marked pair selects descriptions.

### Requirement: Visual auxiliary and local OCR fallback
For unsupported image input, Grow SHALL use an available configured visual auxiliary model to describe images and show `当前模型不支持多模态，调用视觉辅助LLM处理中...`. If the auxiliary model is absent or fails, Grow SHALL show `视觉辅助模型未配置或者调用失败，使用OCR处理中...` and attempt local OCR. A successful description or OCR result SHALL be retained as the image text description and reused for marked models. If neither succeeds, Grow SHALL report the failure without discarding the original image or silently dropping its content.

#### Scenario: Auxiliary description succeeds
- **WHEN** a configured visual auxiliary model produces a nonempty description
- **THEN** Grow retains that description and retries the primary request with text.

#### Scenario: Auxiliary unavailable
- **WHEN** the visual auxiliary route is unconfigured or fails
- **THEN** Grow displays the OCR status and attempts local OCR, retaining a successful nonempty result in the description field.

#### Scenario: Both fallbacks fail
- **WHEN** neither visual description nor local OCR can provide text
- **THEN** the original image remains intact and the request fails with actionable feedback rather than a lossy retry.

#### Scenario: Image fallback fails without damaging the session
- **WHEN** both auxiliary description and OCR fail
- **THEN** only the current request fails recoverably, the user is told the current model does not support multimodal input and may switch models or rewind, no automatic rewind or incomplete image projection occurs, and Timeline replay and manual rewind remain usable.
