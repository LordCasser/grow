## ADDED Requirements

### Requirement: Image fallback respects cancellation and cold recovery

Image fallback SHALL stop new auxiliary/primary provider requests and new OCR processes when its owning foreground turn is cancelled. Completed neutral image descriptions with valid durable source provenance SHALL be reused by a fresh actor after recovery, including a crash after the completed description but before its image projection. Cancellation SHALL NOT be treated as ordinary auxiliary failure that authorizes later fallback work or a lossy projection. Existing OCR output validation, original media retention and projection acknowledgement rules SHALL remain in force.

#### Scenario: Auxiliary preparation is cancelled

- **WHEN** the owning turn is cancelled before its auxiliary provider request crosses admission
- **THEN** no auxiliary HTTP request, OCR process, image projection or primary resubmission is started for that cancelled recovery

#### Scenario: First image group is cancelled during auxiliary work

- **WHEN** a turn with multiple unresolved image groups is cancelled while its first auxiliary request is active
- **THEN** that work is cancelled and no later group's auxiliary request, OCR fallback or primary resubmission begins

#### Scenario: OCR produces valid text

- **WHEN** the configured auxiliary fails and local OCR produces nonempty valid text
- **THEN** a durable OCR projection retains the original image and its text, and the marked primary retries with text only after acknowledgement

#### Scenario: OCR cannot produce text

- **WHEN** the configured auxiliary fails and local OCR is empty or fails
- **THEN** the existing durable unsupported-image removal is acknowledged before a text-only primary retry, while immutable original evidence remains

#### Scenario: Recover after description completion before projection

- **WHEN** a fresh actor restores a validated Timeline and completed image-description Sideband after a crash before ImageProjection
- **THEN** it reuses the matching neutral description, installs a valid projection and sends a text-only primary request without another auxiliary request

#### Scenario: Recover a completed description without a current auxiliary route

- **WHEN** a valid completed neutral description exists but the auxiliary route is absent or cannot be used after recovery
- **THEN** its reuse is evaluated independently of that route and does not fall through to OCR or removal merely because new auxiliary work is unavailable

#### Scenario: Recover an already described image

- **WHEN** a fresh actor restores an image with an acknowledged description or OCR projection and its marked primary continues
- **THEN** the existing description is reused without auxiliary work and the primary request retains the textual representation

## MODIFIED Requirements

### Requirement: Image description recovery batches durable lookups

A recovery pass SHALL reuse one validated durable-history load for its uncached image-description queries. It SHALL preserve the validated historical source revision, exact source Surface identity, prompt and completed-result provenance checks, including integrity validation of other Sidebands required by the parent Timeline. A matching neutral description from an earlier revision MAY be reused when its same immutable image source remains in the current branch; unrelated appends SHALL NOT invalidate it. A queried revision beyond the durable parent or a description from a later revision SHALL NOT be accepted. The new ImageProjection SHALL still bind the exact current source revision and image fingerprint. The pass SHALL NOT retain the full loaded history across auxiliary provider requests.

#### Scenario: Several groups need durable reuse

- **WHEN** a recovery pass has multiple uncached image groups
- **THEN** their durable lookups share one history load and each query receives only its own matching completed result

#### Scenario: Historical Sideband integrity fails

- **WHEN** required Sideband validation fails during batch lookup
- **THEN** no result is accepted from that failed snapshot and existing description failure or provider fallback handling applies

#### Scenario: Every group is cached

- **WHEN** all group descriptions are available in the session cache
- **THEN** the pass performs no durable-description history load

#### Scenario: A new input follows a completed transcription

- **WHEN** a new input advances the Surface revision while the same immutable image source and neutral prompt remain selected
- **THEN** recovery reuses the older completed description and creates its projection against the current exact revision without another auxiliary call

#### Scenario: Future revision or different provenance

- **WHEN** a lookup uses a revision newer than the durable parent, a candidate describes a later revision, or source identity, selected branch, prompt or completed result does not match
- **THEN** that candidate is not accepted as a reusable description
