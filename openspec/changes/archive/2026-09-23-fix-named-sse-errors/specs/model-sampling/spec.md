## ADDED Requirements

### Requirement: Named provider stream errors retain their error facts

Chat Completions, Responses, and Messages SHALL recognize a complete `event:error` SSE frame with nonempty `code` and `message` as a provider error, even when its JSON body lacks a `type` or nested `error` field. Grow SHALL retain the provider code, message, and supplied request ID for diagnosis. Error classification SHALL distinguish confirmed content rejection, throttling, and overload, while unknown codes and malformed ordinary events SHALL NOT gain automatic retry eligibility. Existing attempt settlement, output retraction, admission, tool execution, and shared retry budgets SHALL remain authoritative.

#### Scenario: Content rejection after provisional tool output
- **WHEN** a provider emits a partial tool candidate followed by `event:error` with `InvalidParameter`, a content rejection message, and a request ID
- **THEN** Grow reports the provider facts, settles usage as known or unknown according to actual evidence, rejects the entire candidate, executes no tool, and does not retry that rejection.

#### Scenario: Known transient stream error
- **WHEN** an `event:error` frame carries a confirmed throttling or service overload code
- **THEN** Grow uses the existing 429 or overload classification, and any retry remains bounded by attempt safety and the shared budget.

#### Scenario: Missing fields or unknown code
- **WHEN** a non-error SSE frame lacks a required event type, a named error lacks required fields, or a named error has an unknown provider code
- **THEN** missing-field frames remain protocol failures and unknown provider errors preserve their code/message without being assumed transient.
