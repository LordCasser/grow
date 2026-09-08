## ADDED Requirements

### Requirement: Relative credential helper cwd resolves once
A configured relative credential-helper cwd SHALL resolve against the Grow process working directory before program resolution and child startup. The resolved directory SHALL be used consistently for relative executable paths and the child working directory.

#### Scenario: Relative executable under relative cwd
- **WHEN** a direct-exec helper specifies a relative cwd and a command such as ./token.sh
- **THEN** it starts the program inside that directory without applying the cwd prefix twice, and relative file reads use that directory.
