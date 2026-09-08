# Design
Move the existing CREATE TABLE IF NOT EXISTS meta statement to immediately after the journal-aware connection opens. Query the version row using optional()? so missing rows yield None and actual SQL/FromSql errors propagate before document-schema initialization or version stamping. Remove the duplicate meta creation from the later schema batch. This uses the existing table and does not add a schema, marker or recovery abstraction.

Test a real temporary index whose version value is replaced with a BLOB: String decoding should fail, leaving that exact value and existing documents/marker unchanged. The old implementation instead treats the read error as None and restamps the current version. Test fresh/no-version initialization and rerun readable older/newer/malformed-text version cases. A readable nonnumeric string retains its existing policy; this change is specifically about unsuccessful reads. Keep outer confirmed-corruption recovery and SQLite Busy/Locked classification intact.

Do not widen into concurrent cross-version migration coordination, quarantine file retention, schema compatibility policy or primary session storage changes.
