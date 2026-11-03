# Design

Replace the `PathBuf` success value of `write_daily_log` with a small result enum that carries the path only when bytes were written and identifies the existing ephemeral skip. Keep I/O failures as `io::Error`.

The flush caller reindexes, updates its in-memory duplicate marker, includes a path in the completion notification, and reports `written` only for the written variant. An ephemeral skip becomes an explicit skipped outcome, is excluded from the error counter, and does not count as a successful checkpoint. The session-end hook likewise reports skipped instead of a written path. No storage policy or other flush admission/transport behavior changes.
