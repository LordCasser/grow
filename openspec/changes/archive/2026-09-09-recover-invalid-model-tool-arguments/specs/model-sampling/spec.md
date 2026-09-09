## ADDED Requirements

### Requirement: Malformed completed tool arguments recover without execution
When a structurally valid completed Responses function call contains invalid JSON arguments, sampling SHALL reject the entire response and may resample the unchanged last valid request. Invalid calls and valid siblings from that rejected response SHALL NOT become executable tools or accepted native continuation. This narrow typed recovery SHALL NOT make generic serialization, incomplete status or conflicting identity errors retryable.

#### Scenario: Bad arguments followed by valid output
- **WHEN** the first completed response has invalid tool JSON and the next attempt is valid
- **THEN** recovery emits Retrying diagnostics, the TUI retains normal running activity, and only the valid response is admitted without surfacing a terminal failure to pause Goal.

#### Scenario: Repeated invalid generation
- **WHEN** invalid tool JSON persists
- **THEN** recovery stops by three total attempts or the lower configured cap and surfaces the normal terminal failure; explicit zero retries disables recovery.

#### Scenario: Accounting and evidence before retry
- **WHEN** a rejected attempt has billed usage or unknown usage
- **THEN** its existing usage settlement and evidence acknowledgment complete before another attempt; failure or Goal admission closure prevents retry.

#### Scenario: Cancellation during recovery
- **WHEN** the request is cancelled before the next attempt
- **THEN** no further provider request is issued.
