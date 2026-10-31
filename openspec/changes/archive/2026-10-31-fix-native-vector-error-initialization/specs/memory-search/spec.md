## ADDED Requirements

### Requirement: Native float vector errors have initialized ownership

The native float vector parser SHALL return an initialized, SQLite-owned error string or NULL on failure. Allocation failure before diagnostic text exists SHALL NOT expose an uninitialized error pointer to scalar error reporting or cleanup. Valid conversions and ordinary invalid-input diagnostics SHALL retain their existing semantics.

#### Scenario: JSON vector allocation fails
- **WHEN** the initial float vector allocation fails before a diagnostic string is created
- **THEN** the parser returns SQLITE_NOMEM with a NULL error output, without transferring a vector or undefined cleanup ownership.

#### Scenario: Scalar conversion receives a parser failure
- **WHEN** vec_f32 or vec_quantize_int8 receives that failed conversion
- **THEN** it reports a SQLite error without reading or freeing an undefined pointer, and the same connection remains usable after the fault ends.

#### Scenario: Valid or ordinary invalid input
- **WHEN** float scalar conversion receives supported vector input or malformed input without allocation failure
- **THEN** the existing converted value or concrete input diagnostic is preserved.
