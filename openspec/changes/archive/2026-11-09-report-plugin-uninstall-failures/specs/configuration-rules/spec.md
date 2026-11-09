## ADDED Requirements

### Requirement: Plugin uninstall reports incomplete removal and deregistration

CLI and ACP plugin uninstall SHALL use the shared application owner and propagate existing registry load failures before destructive work. Removal failure or a still-existing unexpected repository path SHALL prevent deregistration and SHALL return an error preserving the installed record. Success SHALL require both confirmed repository path removal and saved deregistration. Save failure after removal SHALL describe the actual partial completion without claiming rollback or complete uninstall. ACP failures SHALL not request reload or restart. Existing multi-plugin confirmation and caller data-retention policies SHALL remain in effect.

#### Scenario: Repository removal fails
- **WHEN** repository removal fails or the target remains present as an unexpected file type
- **THEN** uninstall returns an error, retains its registry record, and does not report full removal.

#### Scenario: Deregistration save fails
- **WHEN** repository files were removed but the resulting registry cannot be saved
- **THEN** CLI and ACP report incomplete uninstall with the known removed path and persistence error, preserving actual files/records rather than fabricating rollback.

#### Scenario: Registry is unreadable or malformed
- **WHEN** uninstall cannot load the existing registry
- **THEN** it returns an error before removing files or changing registration.

#### Scenario: Uninstall succeeds
- **WHEN** confirmation, removal and registry save succeed
- **THEN** callers report the removed plugins with the existing keep-data policy and successful reload behavior.
