## MODIFIED Requirements

### Requirement: Side questions reuse durable image fallback

/btw SHALL use one prepared route and one frozen acknowledged context, preferring existing descriptions. It SHALL wait for primary image projection acknowledgement and observe Sideband cancellation. If the selected request still contains images on a known rejecting route, it SHALL reuse neutral image description/OCR recovery before provider admission. An unknown route SHALL receive its first optimistic request. Only an explicit image capability rejection SHALL record negative capability for that actual route and permit one recovered text attempt. Recovery SHALL preserve the frozen source range, revision and coordinates, SHALL NOT rebase to new context or mutate the primary Surface, and SHALL fail when a selected image cannot be described or the recovered request exceeds its admitted budget. Ordinary errors SHALL retain existing handling. The question and answer remain Sideband-only.

#### Scenario: Known text-only route
- **WHEN** /btw selects images on a known text-only route
- **THEN** acknowledged descriptions or neutral recovery supply text before its answer request; missing descriptions fail without sending images.

#### Scenario: Primary conversion has not finished
- **WHEN** primary conversion or its durable acknowledgement is pending
- **THEN** capture waits cancellably and uses the acknowledged result.

#### Scenario: Historical images rejected by side question
- **WHEN** an unknown prepared route explicitly rejects selected images
- **THEN** only that route is marked unsupported and at most one text recovery attempt follows using the same frozen source, while an ordinary 400 causes no recovery.

#### Scenario: Foreground context advances during the failed request
- **WHEN** main history advances while the frozen request is rejected
- **THEN** recovery and the answer use only the original frozen images and text, preserving their provenance and leaving the primary Surface unchanged.

#### Scenario: Unrelated failure or failed projection
- **WHEN** a provider returns an ordinary error or primary projection failed
- **THEN** ordinary errors cause no capability learning; a failed durable primary write retains the writer fail-stop and cannot be bypassed by independent recovery or a cached description.

#### Scenario: No main conversion owns unresolved images
- **WHEN** selected images have no description and the prepared route is known text-only
- **THEN** neutral frozen recovery runs before answer admission, without changing primary history.

### Requirement: Recap reuses acknowledged image descriptions

Manual and automatic recap SHALL select acknowledged image descriptions after cancellably waiting for primary image projection. They SHALL preserve existing backend reasoning selection, complete tool evidence, bounded window, display-only result and recap cancellation identity. Only images retained by the final budget SHALL be recovered. Known rejecting routes SHALL prepare text before provider admission; explicit rejection by an unknown route SHALL record actual route capability and permit one frozen text recovery attempt. Recovery SHALL NOT mutate the primary Surface or rebase its source. Missing descriptions or exceeded budget SHALL use existing manual unavailable or automatic silent failure behavior.

#### Scenario: Existing descriptions after a model switch
- **WHEN** recap selects described images after switching to text
- **THEN** it sends text without raw media or another description call.

#### Scenario: Unresolved selected images
- **WHEN** known or newly confirmed text-only recap selects unresolved images
- **THEN** neutral recovery supplies text for that frozen source or recap terminates through its existing failure path.

#### Scenario: Unresolved images are outside the retained budget
- **WHEN** budgeting removes unresolved old images
- **THEN** they do not block or start image recovery.

#### Scenario: Conversion awaits acknowledgement
- **WHEN** recap arrives during primary image conversion or durable acknowledgement
- **THEN** it waits cancellably before capture and only selected unresolved images may subsequently need neutral recovery.
