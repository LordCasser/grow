## ADDED Requirements

### Requirement: Sparse Responses failure envelopes preserve provider errors

When SDK decoding fails, the Responses client SHALL recognize the exact sparse failure envelope containing only top-level type and response, response error/object/status and error code/message, with type response.failed, object response, status failed and nonempty string code/message. It SHALL propagate the provider error through existing stream-error classification without inventing a Response snapshot. Original evidence and the observed failed provider terminal SHALL be retained; existing recovery and response-admission owners SHALL remain authoritative.

#### Scenario: Provider fails midstream with a sparse error envelope

- **WHEN** a Responses stream sends partial output and ends with the exact sparse failed envelope
- **THEN** the request fails with the provider code/message and existing error classification, records the observed failed terminal, preserves raw evidence, and does not complete or admit a tool call from the partial output.

#### Scenario: Sparse error classification follows existing policy

- **WHEN** the valid sparse failure carries a server, authentication, rate or request error code
- **THEN** the existing shared stream-error mapping determines its classification and existing bounded recovery policy applies without adding a new retry loop or budget.

#### Scenario: Complete typed failure remains authoritative

- **WHEN** a full ResponseFailed event can be decoded normally
- **THEN** existing typed status, usage and terminal processing remains in effect and is not replaced with a fabricated sparse snapshot.

#### Scenario: Other malformed events remain rejected

- **WHEN** the sparse envelope has missing, null, wrong-type, empty, duplicate or additional fields, has inconsistent object/status values or another known event type, or a completed/incomplete response lacks required fields
- **THEN** the sparse-failure exception does not repair it and existing known-event validation continues to reject it.
