## Context

`LocalDraftStore::load` opens the source once and validates bytes from that handle. Its oversized and invalid-record paths currently call `quarantine(path)`, which renames the current directory entry without checking that it is still the opened source. Ordinary reads follow regular-file symlinks, while quarantine renames the symlink entry itself; that behavior must remain intact.

## Goals / Non-Goals

**Goals:** Carry source identity from the opened handle to the quarantine decision and leave a changed active entry untouched.

**Non-Goals:** Change draft parsing, retention limits, write/rekey semantics, or synchronous filesystem scheduling.

## Decisions

- Pass the opened file through bounded reading to quarantine. Immediately before rename, compare the active path's followed file identity to the opened handle's identity. This distinguishes a replaced regular file or changed symlink target while preserving regular-file symlink reads.
- If identity no longer matches, treat the validated source as superseded and return without renaming or removing the active entry. The same logic applies to metadata-size rejection and post-read oversize/JSON validation failures. Repeat the check after quarantine-directory setup to avoid moving a replacement introduced during that setup.
- Use platform file identity metadata rather than byte equality: replacement drafts may contain identical bytes but are separate filesystem entities. Do not add a dependency or alter the quarantine naming/retention policy.

## Risks / Trade-offs

- [Risk] A process can replace the path in the narrow interval between the final identity check and rename; portable standard filesystem APIs do not provide an atomic rename-if-source-identity-matches operation. The deterministic replacement window between opening/validation and the final check is covered, while the syscall gap remains backlog work.
- [Trade-off] If the opened source has already been detached, it is no longer reachable by the active key and is not copied into quarantine; the current replacement remains available for a later recovery attempt.
