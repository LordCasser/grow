## ADDED Requirements

### Requirement: Worktree deletion confines linked registration cleanup to its owner

Deleting a checkout SHALL NOT authorize recursive deletion of an arbitrary gitfile target. Fast worktree cleanup SHALL remove an external Git registration only after verifying its linked-registration location under the matching common Git directory and its backlink to that exact checkout. Unverified separate metadata and unrelated targets SHALL remain unchanged. Symbolic checkout pointers SHALL NOT grant deletion authority over their targets. Failure to complete a verified registration cleanup SHALL remain observable and SHALL NOT be reported as complete successful removal.

#### Scenario: Remove a linked worktree
- **WHEN** the public removal API receives a checkout with a valid linked registration and matching common-dir/backlink
- **THEN** it removes the checkout and its registration while retaining the common repository and sibling registrations.

#### Scenario: Gitfile names separate or unrelated metadata
- **WHEN** a checkout's editable gitfile points at separate Git metadata or an unrelated directory lacking matching linked-registration ownership
- **THEN** removal does not recursively delete that target; only the explicitly requested checkout can be removed.

#### Scenario: Relative registration references
- **WHEN** gitdir, commondir or the registration backlink uses a legal relative path
- **THEN** identity validation resolves each reference relative to its owning directory and applies the same deletion boundary.

#### Scenario: Registration cleanup cannot complete
- **WHEN** a verified registration cannot be removed after the checkout has been removed
- **THEN** the API reports incomplete removal and retains recoverable metadata rather than recording full success.

#### Scenario: Follow-up stale cleanup preserves rejected registration
- **WHEN** public checkout removal or linked rehydration is followed by path- or prefix-scoped stale cleanup and the candidate registration has a foreign commondir or otherwise unverified linked ownership
- **THEN** every cleanup entry applies the same registration ownership proof and preserves that candidate; a best-effort result cannot grant deletion authority withheld by the removal verifier.

#### Scenario: Retry a tracked linked cleanup after checkout removal
- **WHEN** a prior partial removal has left a tracked linked-worktree record while the checkout is missing
- **THEN** retry revalidates only that checkout's registration from the record's source repository and completes or confirms its absence before unregistering; unreadable or unverifiable recovery evidence remains an observable incomplete result.
