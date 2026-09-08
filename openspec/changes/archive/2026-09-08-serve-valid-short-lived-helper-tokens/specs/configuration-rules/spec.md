## ADDED Requirements

### Requirement: Valid short-lived helper tokens remain sendable
The credential helper refresh margin SHALL trigger proactive refresh without itself making an otherwise valid token unavailable to the request-time bearer resolver. Actual expiry, token-shaping configuration changes, and a failed attempted refresh SHALL make the cached token unavailable for sending.

#### Scenario: Newly minted token inside refresh margin
- **WHEN** a helper returns a token with 30 seconds remaining
- **THEN** the request-time resolver returns that token until actual expiry or invalidation.

#### Scenario: Proactive refresh fails
- **WHEN** refreshing a near-expiry cached token fails
- **THEN** the prior token is unavailable for sending even if its original expiry is still in the future.
