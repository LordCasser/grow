## ADDED Requirements

### Requirement: Permission read errors do not import shared grants
Permission state loading SHALL allow shared-file fallback only when the client-specific read reports NotFound. Other read errors, including invalid UTF-8, SHALL use default remembered state and SHALL NOT rewrite the failed source. This does not override independent configured policy or permission modes.

#### Scenario: Invalid UTF-8 client cache
- **WHEN** a shared cache contains grants and a client-specific cache contains invalid UTF-8
- **THEN** loading that client returns default remembered state and preserves the invalid source bytes.

#### Scenario: Client cache is a directory
- **WHEN** a client cache path is a directory and its read fails
- **THEN** loading returns default remembered state without importing shared grants or altering the directory.
