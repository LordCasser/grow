## MODIFIED Requirements

### Requirement: Visual auxiliary and local OCR fallback

For explicitly unsupported image input, Grow SHALL send an available configured visual auxiliary model only the images in the image group and the fixed English default prompt: `Transcribe the attached image into a neutral, standalone textual description for a model that cannot see images. Preserve all visible text exactly where practical and in its original language, along with structure, spatial relationships, UI state, diagrams, tables, code, errors, and other concrete details. Do not translate, tailor the description to a presumed task, or infer from conversation context.` The request SHALL NOT include conversation history, tool or Skill context, or source message text. Grow SHALL retain the source Surface reference as local Sideband provenance and use the fixed prompt identity for cache and durable-result matching. Image recovery progress and outcome SHALL use one stable operation row as specified by `client-surfaces`; the row SHALL report vision, OCR, completion, partial completion and failure in English rather than emitting a recovery NOTICE. If the auxiliary model is absent or fails, Grow SHALL attempt local OCR. Successful OCR and visual descriptions SHALL preserve recognized/source language without translation and be retained and reused for marked models. Every still-unresolved image group SHALL receive a typed unsupported-model projection with the existing canonical replacement `当前模型不支持多模态，图片已经被删除`; projection failure SHALL preserve the original image evidence and settle the operation row with an English failure state. Description, OCR and removal shadows for one Surface revision SHALL commit atomically before the primary request is rebuilt. Mixed and removal-only outcomes SHALL report their counts in the same English operation row.

#### Scenario: Auxiliary description succeeds

- **WHEN** a configured visual auxiliary model produces a nonempty description
- **THEN** its provider request contains only the group's images and the fixed English prompt, with no conversation history, tools, Skills or source message text; Grow retains the description in its original language and retries the primary request after the projection ACK.

#### Scenario: Auxiliary unavailable

- **WHEN** the visual auxiliary route is unconfigured or fails
- **THEN** the same operation row changes to English OCR progress and Grow attempts local OCR; a successful result is retained in its original language without translation.

#### Scenario: Both fallbacks fail

- **WHEN** neither visual description nor local OCR can provide text for an image group after the primary model explicitly rejects images
- **THEN** Grow durably replaces that group in the current Surface with one existing canonical `当前模型不支持多模态，图片已经被删除`, removes its raw image parts and retries the primary request without those images, while the same operation row reports the outcome in English.

#### Scenario: Mixed fallback outcomes

- **WHEN** one rejected request contains groups that are described successfully and groups that remain unresolved
- **THEN** one exact-revision projection atomically attaches descriptions to successful groups and removes only unresolved groups, without repeated per-image notices, and the same operation row reports the partial outcome in English.

#### Scenario: Image fallback fails without damaging the session

- **WHEN** unsupported-image removal is acknowledged and the user later sends a text-only follow-up
- **THEN** the new request uses the repaired Surface, contains none of the removed historical images and does not repeat the same capability 400 merely because the raw Timeline evidence still exists.

#### Scenario: Image projection cannot be committed

- **WHEN** projection validation, durable write or acknowledgement fails
- **THEN** Grow does not resubmit an in-memory-only lossy request, settles the same operation row with an English failure state, and preserves the original Timeline evidence for exact retry or explicit recovery.

#### Scenario: Reuse completed description after unrelated context changes

- **WHEN** the same immutable image source is selected after unrelated messages, tool results or Skill context have changed, or after recovery
- **THEN** Grow matches the completed result using the fixed prompt identity and exact source/image provenance, reuses it without sending that contextual text to the auxiliary provider, and binds a new projection to the current exact source revision.
