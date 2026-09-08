## ADDED Requirements

### Requirement: Local draft invalidation retries without restoring stale content
Within a running pager, failed local draft invalidations SHALL retain per-key retry intent with backoff, including after agent closure or session binding. Pending invalidation SHALL prevent stale disk content from being restored. A newly captured valid draft with payload SHALL supersede the pending invalidation for its key.

#### Scenario: Prompt ownership transfers while deletion fails
- **WHEN** an ACP prompt RPC transfers ownership and removal fails for its current or session draft key
- **THEN** retain and retry each failed deletion without treating old content as recoverable or blocking the RPC on successful deletion

#### Scenario: Repeated unsupported input
- **WHEN** capture continues to reject the same current input while deletion is pending
- **THEN** preserve a future retry deadline without per-tick deletion attempts or postponing it on every sync

#### Scenario: New draft supersedes removal
- **WHEN** valid new content is captured for a key with pending invalidation
- **THEN** cancel that key's old deletion intent so its retry cannot remove the new draft

#### Scenario: Binding or reopening with pending deletion
- **WHEN** an invalidated key loses its agent owner or binds from cwd to session before removal succeeds
- **THEN** keep deletion obligations for the original keys and do not restore or migrate their stale content into the new composer
