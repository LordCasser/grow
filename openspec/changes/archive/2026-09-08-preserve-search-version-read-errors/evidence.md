# Source evidence (main, 2026-09-08)

session/storage/search_fts.rs::open_with_journal_mode:
- Opens the connection with JournalMode's busy-timeout policy.
- Reads meta.value as Option<String>, then optional().unwrap_or(None) suppresses every query/conversion error.
- None skips the old-version drop path and later triggers INSERT OR REPLACE of session_search_schema_version.
- Existing tests distinguish older/current/newer and readable malformed text, but the inspected cases do not test an unsuccessful SQL-value conversion.

search_recovery.rs::heal_unusable separately reprobes under its lock and declines non-corruption probe errors. This audit does not establish that lock contention causes quarantine; the concrete finding is swallowed metadata-read failure. No build, runtime fault injection or user data modification has yet been performed for this change.
