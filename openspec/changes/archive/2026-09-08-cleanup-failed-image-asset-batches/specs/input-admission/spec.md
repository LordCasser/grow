## ADDED Requirements

### Requirement: Failed image asset batches reclaim completed files
When user-image batch persistence fails during decoding or writing, it SHALL attempt handle-relative durable removal of assets successfully created earlier in that call, preserve pre-existing files and return the original failure. Cleanup failures SHALL be reported without replacing the primary error.

#### Scenario: Later image cannot be saved
- **WHEN** a later asset write fails after an earlier asset was published
- **THEN** the earlier asset is removed when cleanup succeeds, unrelated existing assets remain and the original write error is returned.

#### Scenario: Invalid later encoding
- **WHEN** later Base64 decoding fails after an earlier asset was published
- **THEN** the same cleanup applies to the completed assets.
