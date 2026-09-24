## Context

`LocalDraftStore::quarantine` currently creates `root/quarantine`, renames an invalid regular draft to a UUID-suffixed `.bad` file, and syncs the root. The directory has no retention policy. Source reads and active draft records already have individual bounds, but those do not cap accumulated quarantined records.

This change only owns retained quarantine quantity. The concurrent path replacement and synchronous slow-filesystem boundaries remain separate backlog work.

## Decisions

- Keep at most 64 regular files and 16 MiB total. This makes the count consistent with the existing 64-record store ceiling and limits ordinary 256 KiB corrupt records to the same 64-record scale while still capping unexpectedly large files.
- After each successful quarantine rename, inventory regular files and symlinks using non-following metadata. Sort oldest modification time first, then filename; treat an unavailable modification time as the oldest time so the tie-breaker produces a stable result. Remove in that order until both bounds hold.
- Count symlinks using their own metadata length and never follow them. Ignore other non-regular entries. The quarantine directory remains private and is populated by this store; handling path substitution or arbitrary concurrent mutation is outside this change.
- Sync the quarantine directory after rename and reclamation, and keep the existing root-directory sync for the namespace change.
- If inventory or deletion fails, propagate the I/O error through the existing quarantine caller. Do not silently claim invalid content was isolated when retention maintenance could not finish.

## Risks

- mtime ordering is a retention preference, not a proof of insertion time. The filename tie-breaker makes ties and unavailable timestamps deterministic.
- This policy does not bound how long synchronous directory metadata or deletion calls can take; slow filesystem deadlines remain open.
