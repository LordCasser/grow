## ADDED Requirements

### Requirement: Debug latest-link updates preserve unowned temporaries
Unix debug latest-link updates SHALL NOT remove an existing temporary entry before symlink creation. A failed creation SHALL preserve that entry and the current latest link. Cleanup after failed publication SHALL only be attempted after this update successfully created the temporary link.

#### Scenario: Temporary path collision
- **WHEN** a regular file or symlink already occupies the latest-link temporary path
- **THEN** updating latest leaves both the pre-existing entry and current latest target unchanged.

#### Scenario: Publication failure
- **WHEN** temporary creation succeeds but renaming over latest fails
- **THEN** the newly created temporary is removed and the blocking destination remains.
