## ADDED Requirements

### Requirement: Cold child grants wait for initial parent MCP discovery

Before freezing a restored child’s current MCP bindings, cold restoration SHALL wait for the direct parent’s initial MCP handshakes to settle under the existing startup policy. It SHALL then intersect surviving current bindings with the durable birth membership, current profile and direct-parent ceiling. This wait SHALL NOT block the session command executor. An abandoned request or closing parent SHALL NOT be treated as a successful empty pool. Ordinary new-child admission retains its existing immediate-grant behavior.

#### Scenario: Parent discovery is slower than child restoration
- **WHEN** a restored child has a surviving birth member and the parent’s initial transport discovery is still pending
- **THEN** cold restoration waits before freezing bindings, unrelated session commands remain responsive, and releasing discovery lets the child retain only surviving birth members with new process identities.

#### Scenario: Discovery fails or the parent closes
- **WHEN** initial discovery finishes without a birth member or the parent closes before replying
- **THEN** a failed member is unavailable under the existing current-authority intersection, while a closed or abandoned parent request does not admit a replacement empty grant.
