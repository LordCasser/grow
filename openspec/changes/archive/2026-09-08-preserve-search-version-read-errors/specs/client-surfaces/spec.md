## ADDED Requirements

### Requirement: Search version reads distinguish absence from failure
Search index initialization SHALL treat an absent schema-version row as uninitialized, but SHALL propagate a failed schema-version query or decoding operation before continuing document-schema initialization or version stamping.

#### Scenario: Version cannot be decoded
- **WHEN** an existing schema-version row cannot be decoded as its required SQL text type
- **THEN** opening the index returns the error without restamping that row as the current version.

#### Scenario: Fresh metadata
- **WHEN** the index has no meta table or no version row
- **THEN** initialization establishes the existing meta table and can initialize the index normally.

#### Scenario: Readable version
- **WHEN** the version row is successfully read
- **THEN** the existing current/older/newer/readable-malformed version policy remains in effect.
