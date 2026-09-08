## ADDED Requirements

### Requirement: Image normalization workers have process-wide admission
The shell normalization blocking adapter SHALL run at most one normalize/transcode closure at a time per process. Its permit SHALL remain owned by the blocking closure until that closure returns or unwinds, even when its async caller is canceled.

#### Scenario: Caller canceled during decode
- **WHEN** a blocking image normalization task continues after its async waiter is canceled and another normalization is requested
- **THEN** the new task waits until the existing blocking task finishes before entering its closure.
