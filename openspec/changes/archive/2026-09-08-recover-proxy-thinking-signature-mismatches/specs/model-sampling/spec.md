## ADDED Requirements

### Requirement: Proxy thinking signatures recover without losing portable history
Messages thinking starts MAY omit an initial signature. A subsequent signature delta SHALL supply it normally. A response containing unsigned thinking SHALL retain visible facts but SHALL NOT retain provider-native continuation.

#### Scenario: Signature arrives later
- **WHEN** a thinking start omits signature and a later valid signature delta completes the block
- **THEN** decoding succeeds and the completed signed block remains eligible for native continuation.

#### Scenario: Thinking never receives a signature
- **WHEN** a complete response has an unsigned thinking block
- **THEN** visible reasoning, text and tool facts remain available and its native continuation is discarded.

### Requirement: Missing signature recovery is bounded by native request state
A missing-field signature serialization failure on a request with native continuation SHALL use the acknowledged continuation reset and retry with portable context. Other serialization errors and portable requests SHALL NOT qualify for this recovery.

#### Scenario: Native signature mismatch
- **WHEN** a native-bearing request fails with missing field signature
- **THEN** native state is cleared and the request is retried without opaque signatures while retaining portable conversation facts.

#### Scenario: Portable retry fails
- **WHEN** the portable request encounters the same missing signature error
- **THEN** it terminates instead of repeating continuation recovery.
