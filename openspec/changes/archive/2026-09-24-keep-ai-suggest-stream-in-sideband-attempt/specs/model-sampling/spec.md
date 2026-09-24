## ADDED Requirements

### Requirement: Streamed Sideband attempts retain evidence through body completion

A Sideband provider attempt SHALL remain within its admitted evidence and usage lifetime until its streamed response has been read, decoded, and classified. Raw provider bytes, observed native terminal, and any observed stream end SHALL be attached to that attempt before its Sideband result or failure is committed. A backend that completes at a native terminal without polling to transport EOF SHALL NOT fabricate a stream-end marker. A partial or malformed stream SHALL NOT be reported as a completed provider attempt merely because HTTP stream opening succeeded.

#### Scenario: AI shell suggestion receives a complete streamed response

- **WHEN** an AI shell-command suggestion receives a complete stream from Chat Completions, Responses, or Messages
- **THEN** the Sideband attempt evidence retains the raw response bytes and native terminal, and provider work is marked returned only after the response is collected.

#### Scenario: AI shell suggestion stream fails after opening

- **WHEN** an AI shell-command suggestion stream ends without completion evidence or contains malformed SSE after HTTP stream opening succeeds
- **THEN** the owning Timeline retains the observed response bytes and failure classification, plus any actually observed stream-end marker, before the Sideband fails; no suggestion is accepted.
