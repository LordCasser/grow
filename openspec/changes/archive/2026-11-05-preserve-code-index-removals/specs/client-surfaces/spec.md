## ADDED Requirements

### Requirement: Incremental code navigation retains final removals

When the codebase index processes an ordered file-event batch, a path whose final effective event is removal SHALL be absent from the resulting definitions, references and file metadata. Earlier creation or modification in that batch SHALL NOT erase the removal. Rename source and destination paths SHALL retain their respective final effects. A later creation or modification after removal SHALL preserve the existing reindexing behavior. Unknown-path removal SHALL be harmless, unrelated paths SHALL remain available, and an already published index snapshot SHALL remain unchanged.

#### Scenario: Indexed file is modified then removed
- **WHEN** an indexed file is deleted and one accepted batch contains its modification followed by removal
- **THEN** the next index snapshot contains no definitions, references or metadata for that file.

#### Scenario: Creation does not prove an empty initial index
- **WHEN** a path ends a batch with removal after creation, including create-modify-remove or remove-create-remove sequences for a previously indexed path
- **THEN** its previous and intermediate index entries are removed rather than retained because the events were cancelled.

#### Scenario: Renamed destination is deleted
- **WHEN** a batch renames a file onto a previously indexed destination and then removes the destination
- **THEN** the resulting snapshot has no definitions, references or metadata for either path, while unrelated paths remain indexed.

#### Scenario: Recreated file and immutable earlier snapshot
- **WHEN** a removed path is later recreated with new content before the batch ends
- **THEN** the resulting snapshot reflects the new indexed content, and an earlier published snapshot retains its original content.

#### Scenario: Newly created file is removed before indexing
- **WHEN** a batch creates and removes a previously unknown path
- **THEN** processing succeeds with that path absent and does not remove unrelated entries.
