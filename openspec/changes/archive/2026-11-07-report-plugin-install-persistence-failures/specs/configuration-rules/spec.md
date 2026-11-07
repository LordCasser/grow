## ADDED Requirements

### Requirement: Plugin installation reports incomplete registry publication

Ordinary Git or local plugin installation through the shared CLI and ACP application path SHALL load the existing registry with failure propagation before changing installation artifacts, and SHALL report success only after the resulting install entry is saved. A registry read or parse failure SHALL preserve existing evidence without treating it as an empty writable registry. A save failure after artifact publication SHALL return an error describing the incomplete registration and possible remaining files, and SHALL NOT continue post-install auto-enablement or report a complete installation. This contract SHALL NOT imply rollback of already published artifacts.

#### Scenario: Saving the new installation fails

- **WHEN** plugin files have been published but the install registry cannot be saved
- **THEN** CLI public dispatch returns an error and ACP reports InternalError without requesting reload; both describe the incomplete installation, preserve the actual artifact state, and do not run post-install enablement.

#### Scenario: Existing registry cannot be parsed or read

- **WHEN** an installation encounters an existing registry read or parse error
- **THEN** it fails before publishing a new artifact or enabling a plugin and preserves the existing registry bytes and installed entries.

#### Scenario: Installation saves successfully

- **WHEN** artifact publication and registry save succeed
- **THEN** the existing post-install warnings policy and successful CLI/ACP result apply, with the recorded plugin names and paths corresponding to the installed files.
