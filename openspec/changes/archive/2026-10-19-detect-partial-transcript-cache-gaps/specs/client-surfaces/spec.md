## ADDED Requirements

### Requirement: Offline transcript reads detect missing committed user echoes
The offline transcript reader MUST detect when the selected Timeline branch contains a committed user item with a persisted prompt index but the rewind-filtered display ledger contains no complete ACP user echo for that input's exact message identity. Completeness includes the ordered block count and digest committed with the consumed input. It MUST fail the read with an explicit incomplete-cache error and MUST NOT assign a nearby update by timestamp or synthesize a replacement row.

For each eligible durably ended Timeline turn, the offline transcript reader MUST find a cached `TurnCompleted` with the exact ACP prompt identity persisted on `TurnStarted`. If the mapping or row is absent, it MUST report an incomplete cache and MUST NOT invent a completion result or stop reason. Recovery-only terminal records that do not represent a user-facing completion are excluded.

#### Scenario: A committed user echo is missing
- **WHEN** an offline read contains a current-branch committed user item with prompt index `n`
- **AND** the captured display ledger has no ACP user chunk with prompt index `n`
- **THEN** the read fails with an explicit incomplete-cache error naming `n`

#### Scenario: A matching complete echo exists
- **WHEN** the captured display ledger contains all ACP user chunks for the exact committed message identity
- **AND** their ordered block count and digest match the consumed input's display fingerprint
- **THEN** the offline read proceeds without changing the source ledger

#### Scenario: A user echo is truncated or missing a block
- **WHEN** a matching prompt identity exists in the display ledger
- **AND** its ordered chunks are truncated or do not match the committed block count or digest
- **THEN** the read fails with an explicit incomplete-cache error

#### Scenario: A terminal row is missing
- **WHEN** an eligible Timeline turn has a durable `TurnStarted` prompt identity and a durable terminal event
- **AND** the display ledger has no `TurnCompleted` with that exact prompt identity
- **THEN** the read fails with an explicit incomplete-cache error without synthesizing a completion

#### Scenario: A prompt was removed by rewind
- **WHEN** a committed user item exists only on a discarded branch
- **THEN** its absent display echo does not fail the selected-branch read

### Requirement: Offline transcript reads detect missing durable Workflow and Control projections
Before publishing a durable Workflow, Goal, Control, or CurrentMode display row, the producer MUST durably record a bounded Timeline expectation that binds the exact UI event identity, source family and identity, source revision captured with the update, and serialized payload digest. Offline transcript reads MUST compare each expectation visible through the captured Timeline frontier with the captured display-ledger frontier and fail with an explicit incomplete-cache error if the row is absent or its digest differs. Expectations MUST NOT contain display bodies or grant execution authority. Ephemeral updates and updates coalesced before durable publication MUST NOT create expectations. Readers MUST NOT infer a missing row from timestamps, mutable manifests, neighboring rows, or current runtime state, and MUST NOT rewrite the cache.

#### Scenario: Durable Workflow row is missing
- **WHEN** the captured Timeline contains a Workflow projection expectation for event ID `e`
- **AND** the captured display ledger contains no row with event ID `e`
- **THEN** the offline read fails with an incomplete-cache error identifying the missing projection

#### Scenario: Durable Control row has changed
- **WHEN** the captured display ledger contains the expected event ID but its serialized payload digest differs from the Timeline expectation
- **THEN** the offline read fails with an incomplete-cache error and does not use a nearby revision as a replacement

#### Scenario: Ephemeral Workflow or Control snapshot
- **WHEN** a Workflow or Control snapshot is sent only as an ephemeral or coalesced update
- **THEN** no durable projection expectation is required and its absence from the captured display ledger does not fail the read

#### Scenario: Durable Goal or CurrentMode row is missing
- **WHEN** the captured Timeline contains a Goal or CurrentMode projection expectation
- **AND** the captured display ledger contains no row with that exact event ID
- **THEN** the offline read fails with an incomplete-cache error naming the missing projection

#### Scenario: Captured frontiers exclude a later projection
- **WHEN** a projection expectation or display row is appended after the read's captured Timeline or display-ledger frontier
- **THEN** the read does not mix that later fact into its fixed snapshot

#### Scenario: A cache-missing expectation still reserves its event identity
- **WHEN** a cold load has a validated Timeline expectation whose cache append was interrupted, or whose row belongs to a rewind-discarded branch
- **THEN** the process event counter is seeded past that expectation before the session actor can publish another row, so the missing identity is not reused

#### Scenario: Existing history has no projection expectation
- **WHEN** a current-format history contains Workflow or Control rows but predates projection expectation facts
- **THEN** the reader does not infer missing expectations or reconstruct absent rows; unsupported older Timeline formats follow schema-version rejection
