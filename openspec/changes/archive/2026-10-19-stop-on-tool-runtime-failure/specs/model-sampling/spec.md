## ADDED Requirements

### Requirement: Tool runtime failures stop dependent sampling

A failure returned by the tool execution pipeline SHALL stop the admitted model turn and completion recovery before any subsequent provider request. Ordinary tool error results and permission outcomes SHALL retain their existing behavior. The existing turn terminal owner SHALL settle unfinished Tool and Step scopes in causal order, without claiming that an unconfirmed execution succeeded. Fatal diagnostics SHALL retain the original pipeline error, including when terminal persistence also fails.

#### Scenario: Tool admission fails
- **WHEN** a model response is admitted but its tool start or runtime preflight cannot be confirmed
- **THEN** no dependent model request is sent, and the original admission failure is retained rather than replaced by a later Step boundary error.

#### Scenario: Tool result or terminal acknowledgement fails
- **WHEN** tool execution has returned but result publication or Tool terminal acknowledgement fails
- **THEN** dependent sampling and completion recovery stop; unresolved scopes are settled only through confirmed existing terminal rules, or remain explicitly unconfirmed if persistence is unavailable.

#### Scenario: Cold recovery after a failed tool boundary
- **WHEN** a session is reopened from the durable prefix of a failed tool boundary
- **THEN** the prefix remains valid, no successful tool completion is fabricated, and any confirmed fatal Turn terminal retains its original failure evidence.

#### Scenario: Turn terminal fails after a tool runtime error
- **WHEN** terminal persistence fails while settling a tool pipeline failure
- **THEN** the returned failure reports the unconfirmed terminal and also preserves the original tool error.
