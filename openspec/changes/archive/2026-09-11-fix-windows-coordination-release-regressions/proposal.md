# Why

Windows coordination CI exposes three concrete problems: actor fixtures use `/tmp` where Windows requires an absolute drive-qualified path; replacing a peer manifest fails while a reader holds the old manifest; an independent storage adapter cannot load a live session because it uses a writer-style directory capability that conflicts with the publishing handle's retained DELETE access. The latter two affect runtime coordination/observation and block release verification.

# What changes

Use platform-native temporary paths in the existing actor fixture and apply the existing Unix condition to the PTY harness import. Preserve dependency caches after failed coordination CI runs. Publish Windows peer manifests with handle-based extended rename and POSIX replacement semantics, preserving existing readers of the old file. Use the existing shared-read contained-directory operations for independent session load operations, without inserting those observer capabilities into the writer cache. Reuse the opened entity for Workflow reads. Preserve writer leases, path/identity checks, no-reparse rules and sideband recovery ownership.

# Capabilities

The disk-usage test fixture also selects the existing platform-correct symlink helpers; all regression assertions remain intact.

Extend local-coordination and session-timeline with the verified Windows reader/writer coexistence boundaries. No new subsystem, storage format or cross-platform architecture rewrite.

# Impact

Preserve bounded stderr and exit diagnostics on failed process regressions. An explicit workflow-dispatch option reruns only CLI/process validation after unchanged library tests have passed; the default and pull-request matrix retain every existing library test step.

Give only the Windows coordination debug executable a larger main-thread stack after a confirmed stack overflow. Run the unchanged process scenarios against native distribution executables as a release smoke gate, preserving production stack settings.

Coordination manifest publication, JSONL session observation, the portable actor test fixture and existing regression cases. Windows execution is required before archiving. Linux/macOS regression and release builds remain gates in the parent release record.

The native Windows process regression also exposes an immutable input artifact rename using an ordinary DOS path beyond MAX_PATH. Reuse the existing verbatim/UNC path encoder for the pinned-handle no-replace publication. Normalize a confirmed non-directory child to the existing invalid-entity scan exclusion, without suppressing operational I/O errors.
