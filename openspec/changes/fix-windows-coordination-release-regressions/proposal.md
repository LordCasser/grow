# Why

Windows coordination CI exposes three concrete problems: actor fixtures use `/tmp` where Windows requires an absolute drive-qualified path; replacing a peer manifest fails while a reader holds the old manifest; an independent storage adapter cannot load a live session because it uses a writer-style directory capability that conflicts with the publishing handle's retained DELETE access. The latter two affect runtime coordination/observation and block release verification.

# What changes

Use platform-native temporary paths in the existing actor fixture. Publish Windows peer manifests with handle-based extended rename and POSIX replacement semantics, preserving existing readers of the old file. Use the existing shared-read contained-directory operations for independent session load operations, without inserting those observer capabilities into the writer cache. Reuse the opened entity for Workflow reads. Preserve writer leases, path/identity checks, no-reparse rules and sideband recovery ownership.

# Capabilities

Extend local-coordination and session-timeline with the verified Windows reader/writer coexistence boundaries. No new subsystem, storage format or cross-platform architecture rewrite.

# Impact

Coordination manifest publication, JSONL session observation, the portable actor test fixture and existing regression cases. Windows execution is required before archiving. Linux/macOS regression and release builds remain gates in the parent release record.
