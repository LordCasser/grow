## ADDED Requirements

### Requirement: Standard Messages stream errors use known service classifications

A qualified provider stream error whose normalized exact type is `api_error` SHALL use the existing internal-server-error classification. A qualified stream error whose normalized exact type is `timeout_error` SHALL use the existing gateway-timeout classification. Grow SHALL retain the original provider type, message and supplied request identity, and SHALL NOT attach the unknown-code retry veto to either known type. These logical stream classifications SHALL NOT overwrite a real non-success HTTP status. Retry SHALL remain conditional on existing caller vetoes, attempt settlement, replay safety, shared attempt limits and absolute deadlines.

#### Scenario: Standard internal error after stream admission

- **WHEN** a valid Messages SSE error carries `api_error` after HTTP 200 stream admission
- **THEN** Grow exposes the original facts with logical status 500 and permits only retry eligibility under the existing bounded recovery policy.

#### Scenario: Standard processing timeout in a stream

- **WHEN** a valid Messages SSE error carries `timeout_error` without a non-success HTTP response status
- **THEN** Grow exposes the original facts with logical status 504 and applies the existing conditional service-error recovery policy.

#### Scenario: Real HTTP status is available

- **WHEN** a non-success HTTP response carries `timeout_error` with status 500
- **THEN** Grow preserves actual status 500 rather than replacing it with the stream-only logical status.

#### Scenario: Unknown or mixed ordinary error remains ineligible

- **WHEN** a stream carries an unknown lookalike code or an ordinary-event identity combined with an error envelope
- **THEN** unknown-code veto and ordinary-frame Serialization rules remain authoritative, even if the text resembles a standard error.

#### Scenario: Recovery is unsafe or exhausted

- **WHEN** a known service stream error occurs after replay is prohibited or after the shared attempt or time budget is exhausted
- **THEN** no further provider attempt is admitted, no failed tool candidate executes, and applicable usage remains settled according to available evidence.
