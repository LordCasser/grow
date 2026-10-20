## ADDED Requirements

### Requirement: Official release binaries identify the validated source
Official release builds SHALL embed the version and seven-character source prefix from the validated immutable release commit across native and container build boundaries. Explicit malformed source metadata SHALL fail the build; release smoke SHALL verify the complete expected version identity.

#### Scenario: Container Git metadata is unavailable
- **WHEN** an official builder receives the validated full RELEASE_COMMIT but Git inside the build environment is missing or cannot read the checkout
- **THEN** the binary reports that commit's seven-character prefix with the validated version rather than unknown

#### Scenario: Explicit build identity changes or is malformed
- **WHEN** RELEASE_COMMIT changes on an existing Cargo target directory or is present but not a full hexadecimal commit
- **THEN** Cargo rebuilds the identity for a valid new value, and malformed input fails compilation without a silent Git/unknown fallback

#### Scenario: Release smoke sees a wrong source identity
- **WHEN** a smoke-tested release binary has the right product version but a missing or wrong source prefix
- **THEN** the smoke gate fails and publication remains blocked

#### Scenario: Ordinary local build without explicit metadata
- **WHEN** RELEASE_COMMIT is absent in a local development build
- **THEN** existing Git-based identity discovery and the Git-unavailable unknown fallback remain available
