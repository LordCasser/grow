## Why
Hidden announcement state uses truncating writes, and IO errors are discarded before the pager emits an unconditional success result. Failed/partial writes can lose saved preferences without truthful diagnostics.

## What Changes
Publish a complete snapshot through a unique same-directory temporary file and atomic replacement, propagate IO failure to the existing pager result path, and clean up only the temporary file owned by that attempt. Preserve canonical hidden_ids serialization and in-memory UI behavior. Snapshot ordering/read budgets remain separate.
