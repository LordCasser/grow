## ADDED Requirements

### Requirement: Plugin config mutations share the raw settings transaction

Plugin path, enable/disable and CTA dismissal config.toml edits SHALL acquire the existing process-wide settings write lock before reading and retain it through atomic publication. They SHALL mutate raw TOML, preserving unrelated unknown fields and authored environment references. Only missing files MAY be treated as empty under the existing operation policy; hard read errors and invalid TOML SHALL fail without replacing the original file. Successful replacement SHALL retain existing platform permission handling. Async consumers SHALL await this transaction; synchronous UI handlers SHALL route persistence through the existing effect executor.

#### Scenario: Unreadable plugin config is not an empty config

- **WHEN** a plugin/CTA edit encounters a hard read error or invalid TOML
- **THEN** it returns an observable failure and preserves the prior target rather than publishing a default or partial config.

#### Scenario: Plugin and settings edits overlap

- **WHEN** plugin/CTA mutations and a settings update are admitted concurrently in one process
- **THEN** each reads the prior committed raw config under the same write lock, and their unrelated changes and unknown/environment-valued fields survive the final publication.

#### Scenario: CTA dismissal uses async persistence

- **WHEN** the user dismisses a plugin CTA from the synchronous mouse handler
- **THEN** local UI behavior remains responsive and the existing effect executor awaits the same serialized raw transaction, retaining the current persistence error reporting.
