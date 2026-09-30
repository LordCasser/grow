## ADDED Requirements

### Requirement: Overlapping Workflow discovery roots have one scope

Workflow discovery SHALL scan a directory only once when the project and user workflow paths coincide or their successfully opened directory capabilities identify the same filesystem entity. The shared directory SHALL be classified as User. Distinct project and user directories SHALL retain their separate scopes, including when both define the same name.

#### Scenario: Project root is the user's home directory

- **WHEN** the project `.grow/workflows` path and user `workflows` path name the same directory
- **THEN** each file in that directory is discovered once as a User Definition and resolves with its User identity.

#### Scenario: Distinct scoped directories share a name

- **WHEN** the project and user workflow directories differ and both contain a valid Definition with the same name
- **THEN** discovery retains both Definitions with their respective scopes.
