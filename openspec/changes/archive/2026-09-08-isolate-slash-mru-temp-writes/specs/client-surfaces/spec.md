## ADDED Requirements

### Requirement: Slash MRU writes own unique temporary files
Each slash MRU snapshot write SHALL own a unique temporary file in the destination directory and publish a complete snapshot by atomic replacement. Failure cleanup SHALL not remove another writer's temporary path.

#### Scenario: Concurrent snapshot writers
- **WHEN** independent writers publish snapshots to the same MRU destination
- **THEN** each write uses its own temporary file and the resulting file contains one complete snapshot, with existing last-writer-wins semantics.

#### Scenario: Legacy temporary path exists or publication fails
- **WHEN** the old fixed temporary path already exists or destination replacement fails
- **THEN** do not overwrite/remove the unrelated fixed path, and clean only the failing writer's owned temporary file.
