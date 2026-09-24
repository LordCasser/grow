## MODIFIED Requirements

### Requirement: Image normalization workers have process-wide admission
The shell image normalization pipeline for each `normalize_one` call SHALL run in one process-wide single-worker blocking closure. That closure SHALL include input base64 decoding, optional endpoint transcoding and its PNG base64 encoding, normalization validation/decoding/re-encoding, and output base64 encoding. Its permit SHALL remain owned by the closure until every stage returns or unwinds, even when its async caller is canceled.

#### Scenario: Normalization stages share admission
- **WHEN** an image requires optional endpoint transcoding and/or compression
- **THEN** input decode, conversion, compute, and output encoding all execute while the same worker permit is held

#### Scenario: Caller canceled during decode
- **WHEN** a normalization closure continues after its async waiter is canceled and another image normalization is requested
- **THEN** the new image waits until every stage in the existing closure finishes before entering its own normalization pipeline.
