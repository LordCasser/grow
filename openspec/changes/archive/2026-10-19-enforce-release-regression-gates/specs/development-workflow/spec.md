## ADDED Requirements

### Requirement: Release regression gates are bound to the published source
The release workflow SHALL require its relevant regression suite to succeed on the exact commit validated from the release tag before publishing assets.

#### Scenario: Publish a validated release
- **WHEN** the release workflow validates a tag and publication is requested
- **THEN** it SHALL run reusable regression gates and strict OpenSpec validation against the validated commit, and publish only if validation, build, dispatch guard, and every required gate completed successfully.

#### Scenario: A regression gate does not complete successfully
- **WHEN** a required gate fails, is skipped, or is cancelled
- **THEN** the release workflow SHALL NOT publish assets.

#### Scenario: The release workflow is cancelled after its dependencies finish
- **WHEN** the release workflow is cancelled after its dependencies have completed
- **THEN** the release workflow SHALL NOT publish assets.

### Requirement: Exact Cargo regression selections prove execution
CI SHALL reject exact Cargo test selections that are missing, ambiguous, fail, or execute no test.

#### Scenario: A selected test exists exactly once
- **WHEN** CI selects an exact test name and its ignored status is explicitly requested when applicable
- **THEN** CI SHALL verify one matching test is listed and exactly one test executes successfully.

#### Scenario: A selected test name is stale or does not execute
- **WHEN** an exact selection has no unique listed test or Cargo reports zero executed tests
- **THEN** the regression gate SHALL fail.
