## ADDED Requirements

### Requirement: Stable per-client permission cache keys
Remembered permission storage SHALL derive per-client filenames from the SHA-256 digest of the exact UTF-8 client identifier. An absent identifier SHALL use permission.toml; a missing per-client file SHALL retain the shared-file fallback. Legacy sanitized per-client names SHALL NOT be probed as a fallback.

#### Scenario: Previously colliding identifiers
- **WHEN** clients foo/bar, foo\\bar and foo_bar persist distinct grants
- **THEN** each client reloads its own grants without overwriting the others.

#### Scenario: Empty and long identifiers
- **WHEN** a client identifier is empty or contains a long Unicode string
- **THEN** it maps to a fixed-length filename distinct from the shared filename.
