## ADDED Requirements

### Requirement: Negative image capability follows the resolved transport

Grow SHALL scope a confirmed negative image-input capability to the resolved transport identity consisting of the actual provider-facing model, API backend, endpoint and query route, as represented by `ModelImageInputKey`. Catalog aliases SHALL NOT determine whether a transport has previously rejected images. A different alias resolving to the same transport SHALL share its negative capability; any change to model, backend, endpoint or query route SHALL produce an independent capability state. Primary sampling, configured visual auxiliary sampling and compaction SHALL use the same identity rule. A configured auxiliary resolving to the same transport already known to reject the primary image request SHALL NOT be invoked recursively. User-visible model labels SHALL continue to use their catalog identity.

This capability state SHALL only affect whether the exact resolved transport is treated as previously known to reject images. An unmarked transport SHALL receive original image content on its first request. After an explicit rejection, the existing visual auxiliary, local OCR, durable `ImageProjection` acknowledgement and request reconstruction flow SHALL remain in force. Descriptions keyed to the same source and image group MAY be reused after the auxiliary transport changes.

#### Scenario: Same alias resolves to a changed transport

- **WHEN** a catalog alias previously received an explicit image rejection but now resolves to a different model, backend, endpoint or query route
- **THEN** the new transport has no inherited negative image capability and its first request may include the retained original image

#### Scenario: Different aliases resolve to the same rejected transport

- **WHEN** one alias receives an explicit image rejection and another alias resolves to the same model, backend, endpoint and query route
- **THEN** the second alias observes the same negative capability and uses the existing marked-model image representation without repeating the rejected image request

#### Scenario: Auxiliary alias resolves to the rejected primary transport

- **WHEN** the configured auxiliary alias resolves to the exact transport that rejected the primary image request
- **THEN** Grow skips the auxiliary call to that runtime and continues through the existing OCR/removal projection flow without recursive retry

#### Scenario: Auxiliary alias resolves to a different transport

- **WHEN** a configured auxiliary alias retains its name but resolves to a transport whose model, backend, endpoint or query route differs from the rejected primary transport
- **THEN** Grow treats it as an independent runtime and may invoke it through the newly resolved route

#### Scenario: Primary or auxiliary runtime rejects image input

- **WHEN** a primary or configured auxiliary transport explicitly rejects an image-bearing request
- **THEN** Grow records the rejection under that request's resolved transport identity, and later lookup by any alias uses the same identity rule

#### Scenario: Compaction checks a model's image capability

- **WHEN** compaction evaluates whether an image-bearing request can use its resolved primary transport
- **THEN** it consults the same transport-bound negative capability state used by normal sampling

#### Scenario: A pending image compaction outlives its transport route

- **WHEN** the model, backend, endpoint or query route changes while an image-bearing compaction request is pending
- **THEN** its old rejection does not mark or recover the new transport; an alias-only change to the same transport does not invalidate its identity
