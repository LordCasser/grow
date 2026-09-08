## ADDED Requirements

### Requirement: Image descriptions retain their original image evidence
An acknowledged ImageProjection SHALL retain original image payloads alongside their nonempty descriptions in materialized image parts, while advancing existing replacement Surface identities and validating source fingerprints, counts and provenance. Local OCR descriptions SHALL identify their local engine rather than claim a provider Sideband.

#### Scenario: Resume a described image
- **WHEN** a session with a completed image description is replayed
- **THEN** the image and description are both available with consistent causal Surface identities.

#### Scenario: Select request representation
- **WHEN** a known unsupported canonical provider/model pair prepares a request
- **THEN** the request uses available descriptions without mutating the retained image, and unresolved descriptions prevent a lossy retry.

#### Scenario: Switch to an unknown pair
- **WHEN** another canonical provider/model pair prepares its first request
- **THEN** original image parameters remain available even if another pair previously used descriptions.
