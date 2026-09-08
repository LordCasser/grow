## Why
SessionSearchIndex::open_with_journal_mode uses optional().unwrap_or(None) for its version query. SQL/decoding failures become the fresh-database case, allowing schema setup and version restamping despite failure to establish the previous version. This also hides error classification from the new contention retry boundary.

## What Changes
Make absent version rows an explicit supported state while propagating failed version reads. Initialize the meta table needed by genuinely new databases before querying it, then use optional()? rather than swallowing all errors. Preserve existing readable-version upgrade policy and corruption-healing boundaries.
