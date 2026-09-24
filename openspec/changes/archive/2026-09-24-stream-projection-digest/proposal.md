# Stream the response projection digest

## Why

Response projection construction currently serializes the complete digest input into a temporary `Vec<u8>` before hashing. The serialized bytes are used only as SHA-256 input, so this allocation is avoidable.

## What changes

Serialize the same `DigestInput` directly into a minimal `Write` adapter backed by `Sha256`. Preserve the JSON serialization and resulting digest exactly, with a regression test comparing against the existing serialize-then-hash calculation.

## Capabilities

No externally observable behavior, interface, state or persistence contract changes. `skip_specs: true` because this is an internal allocation reduction and the existing session-timeline contract remains authoritative.

## Impact

Only response projection digest construction and the matching preview-memory backlog entry.
