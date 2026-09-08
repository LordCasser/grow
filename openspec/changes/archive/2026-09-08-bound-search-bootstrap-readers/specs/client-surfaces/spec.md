## ADDED Requirements

### Requirement: Search bootstrap bounds live Timeline readers
Search bootstrap SHALL acquire its configured concurrency capacity before opening each Timeline reader. Capacity SHALL remain held until both the admitted indexing task and any detached blocking Timeline read finish. Identity-checked snapshot readers SHALL continue to be used.

#### Scenario: Session count exceeds descriptor budget
- **WHEN** many sessions are bootstrapped with sufficient descriptors for the configured concurrent work but fewer descriptors than sessions
- **THEN** bootstrap processes their ledgers without retaining one open reader per session.

#### Scenario: Blocking read outlives timeout
- **WHEN** an admitted blocking Timeline read continues after its async timeout
- **THEN** its concurrency capacity remains occupied until the blocking read finishes.

#### Scenario: Reader admission fails
- **WHEN** the storage authority rejects opening a reader
- **THEN** bootstrap reports failure without publishing a completed-bootstrap marker.

#### Scenario: Read-only bootstrap visits many session directories
- **WHEN** Timeline readers are created for search projection
- **THEN** their transient directory capabilities are not retained in the adapter writer cache after reader construction.

#### Scenario: Bootstrap enumerates summaries
- **WHEN** bootstrap lists session summaries
- **THEN** enumeration retains summary values rather than a directory capability per result, without populating the writer cache or weakening identity validation.
