## ADDED Requirements

### Requirement: Stdio creation replay state follows the JSON-RPC request identity

The CLI bridge SHALL associate each pending session/new snapshot with that request's exact JSON-RPC ID and SHALL settle it only upon a matching response. Numeric and string IDs SHALL remain distinct. A matching error or unusable success SHALL discard only that pending creation. A matching successful creation SHALL cache its assigned session ID with that request's cwd and MCP declaration; unrelated responses SHALL NOT consume another creation. Duplicate settled responses and unconfirmed creations SHALL NOT add reconnect loads. Existing deterministic confirmed-session replay and close removal SHALL remain intact.

#### Scenario: Overlapping creations respond in reverse order

- **WHEN** two session/new requests with different IDs and cwd/MCP snapshots receive successful responses in reverse order
- **THEN** both confirmed sessions retain their own request snapshots and each is loaded exactly once on reconnect.

#### Scenario: Error and unrelated response interleave

- **WHEN** one pending creation errors while a load response and another creation response arrive
- **THEN** only each matching creation is settled, the load response does not take either snapshot, and only successfully confirmed open sessions are replayed.

#### Scenario: Numeric and string IDs differ

- **WHEN** creations use numeric 1 and string "1"
- **THEN** their responses settle separate entries and cannot exchange cwd or MCP snapshots.
