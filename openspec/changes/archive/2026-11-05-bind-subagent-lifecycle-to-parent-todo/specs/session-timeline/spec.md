## ADDED Requirements

### Requirement: Runtime planning context remains a request-only projection

The ChatState request builder SHALL accept host-owned runtime context separately from persistent retrieved memory. It SHALL include runtime context in the effective provider request, source projection and request-pressure accounting without appending a Messages event, Surface item or synthetic user-history row. Each request SHALL use only its current supplied planning context. Existing memory-reminder persistence, causal tool pairing and native continuation boundaries SHALL remain valid.

#### Scenario: Build with runtime planning context
- **WHEN** a request is assembled with runtime planning context
- **THEN** the provider request contains it and its input budget accounts for it, while Timeline messages, Surface identities and user-facing history remain unchanged by that context.

#### Scenario: Refresh or omit runtime context
- **WHEN** the next request supplies different runtime context or omits it
- **THEN** only that request's context is projected and no previous runtime-context item leaks from persistent memory.

#### Scenario: Actual retrieved memory accompanies runtime planning
- **WHEN** a request includes both a new retrieved-memory reminder and runtime planning context
- **THEN** memory follows its existing durable exactly-once behavior and planning stays request-only, with both included in complete input accounting.
