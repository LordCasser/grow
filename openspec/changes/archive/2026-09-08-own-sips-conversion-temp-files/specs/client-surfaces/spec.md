## ADDED Requirements

### Requirement: Sips conversion owns private temporary files
Sips conversion SHALL keep source and output files within a unique owned temporary directory with Unix mode 0700, and release that directory on ordinary success and failure exits.

#### Scenario: Conversion fails before or after process creation
- **WHEN** source creation/write, command launch, conversion status or output retrieval fails
- **THEN** release the owned directory and its files rather than relying on reaching per-file cleanup statements.

#### Scenario: Independent successful conversions
- **WHEN** separate conversions create workspaces and produce output
- **THEN** use distinct private directories, return the output bytes and release each workspace independently.
