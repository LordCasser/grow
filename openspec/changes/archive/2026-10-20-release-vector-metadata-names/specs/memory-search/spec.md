## ADDED Requirements

### Requirement: Native vector metadata names follow virtual table ownership
The vendored vector backend SHALL release every owned metadata shadow-table name when its virtual-table owner is destroyed or initialization fails after allocation. Statement-only cleanup SHALL retain names needed by the live table, and rename SHALL transfer ownership to replacement names without leaking or double-freeing either generation.

#### Scenario: Successful metadata table lifecycle
- **WHEN** a virtual table with multiple metadata columns is used, optionally renamed or reconnected, and then dropped or disconnected
- **THEN** valid queries retain their results and all names owned by each retired virtual-table instance are released exactly once

#### Scenario: Initialization or partial allocation cleanup
- **WHEN** construction fails after metadata-name allocation or cleanup sees partially allocated name slots
- **THEN** allocated metadata names are released, NULL slots are safe, and repeated cleanup does not double-free names

#### Scenario: Statement cleanup while the owner remains live
- **WHEN** transaction sync or rename finalizes cached statements while the table remains usable
- **THEN** current metadata-name ownership remains valid until replacement or destruction, and subsequent metadata queries succeed
