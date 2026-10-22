## ADDED Requirements

### Requirement: Side questions reuse durable image fallback
/btw SHALL use its prepared transport identity for negative image capability. An unknown transport SHALL first receive original images; an explicit unconditional image rejection SHALL settle the failed Sideband, persist the negative capability and use the existing visual auxiliary/OCR/durable ImageProjection flow before at most one rebuilt side-question request. A known text-only transport SHALL perform that preparation before sending. The rebuilt request SHALL freeze the acknowledged Surface again and select textual image descriptions before portable history projection. Question and answer SHALL remain Sideband-only; shared image projections SHALL retain existing original-media and unresolved-removal semantics. Ordinary request failures SHALL NOT trigger image conversion. All attempts SHALL retain the prepared client/model route.

#### Scenario: Historical images rejected by side question
- **WHEN** /btw with user or completed-tool images receives the explicit text-only content type rejection
- **THEN** the failed Sideband closes, shared image recovery is acknowledged and a new frozen Sideband sends textual context to the same prepared transport

#### Scenario: Known text-only route
- **WHEN** /btw uses a transport already marked text-only
- **THEN** it selects acknowledged descriptions without sending raw images or repeating auxiliary work for described groups

#### Scenario: Unrelated failure or failed projection
- **WHEN** /btw receives an ordinary 400 or its image projection cannot be acknowledged
- **THEN** it returns failure without an unacknowledged rebuilt provider request, and ordinary errors do not mark negative capability

#### Scenario: Foreground context advances during the failed request
- **WHEN** the frozen side-question request rejects images but the Surface revision has advanced before recovery
- **THEN** /btw returns failure without projecting images from the newer revision
