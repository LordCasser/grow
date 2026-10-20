## ADDED Requirements

### Requirement: Native vector dispatch rejects inconsistent state
The vendored vector backend SHALL reject missing metadata-IN dispatch entries and unsupported vector or distance dispatch values with a SQLite error before accessing absent entries, computing an undefined distance, or writing with undefined byte counts. Valid vector search behavior SHALL remain unchanged.

#### Scenario: Metadata IN has no matching argument
- **WHEN** text metadata filtering receives an empty IN dispatch array or no entry for its argument index
- **THEN** it returns SQLITE_ERROR without indexing beyond the dispatch array and releases its temporary rowid/statement resources

#### Scenario: Unsupported native dispatch value
- **WHEN** vector distance dispatch or vector blob writing receives an unsupported element or metric value
- **THEN** it returns SQLITE_ERROR through its existing cleanup boundary rather than using an uninitialized result, byte length or offset

#### Scenario: Supported vector queries
- **WHEN** the backend receives a supported MATCH/k search or a normal text metadata IN filter
- **THEN** valid nearest-neighbor and metadata-filter results retain their existing semantics
