## ADDED Requirements

### Requirement: Exact Cargo regression runners expose bounded phases
The exact Cargo regression runner SHALL distinguish compilation, test enumeration, and test execution; stream command output while a phase is running; and enforce configurable phase deadlines.

#### Scenario: A selected test is compiled and executed
- **WHEN** the runner starts an exact test selection
- **THEN** it SHALL report compilation, enumeration, and execution as separate phases, stream each phase's output as it arrives, and retain exact-name and one-successful-test checks.

#### Scenario: A Cargo phase exceeds its deadline
- **WHEN** compilation, enumeration, or test execution exceeds that phase's configured deadline
- **THEN** the runner SHALL fail with the phase identified, attempt to terminate the command's process tree, and return only after cleanup is confirmed or explicitly reported as unconfirmed.

#### Scenario: The runner is cancelled during a Cargo phase
- **WHEN** the runner receives an interrupt while a Cargo phase is active
- **THEN** it SHALL attempt to terminate the command's process tree, reap the command process, report cancellation and whether cleanup was confirmed, and exit unsuccessfully.

#### Scenario: A descendant keeps an inherited output pipe open after its parent exits
- **WHEN** a Cargo phase's direct child exits but a descendant keeps the merged output pipe open past the phase deadline
- **THEN** the runner SHALL finish cleanup waiting within a bounded interval and fail with an explicit cleanup-unconfirmed result if it cannot confirm the tree has stopped.

#### Scenario: A configured deadline is not finite and positive
- **WHEN** a phase deadline is zero, negative, NaN, or infinite
- **THEN** the CLI SHALL reject the value before starting Cargo.

#### Scenario: The output reader fails while the command exits successfully
- **WHEN** the output pipe reader encounters an I/O error during a Cargo phase
- **THEN** the runner SHALL report the read failure and SHALL NOT treat it as clean end-of-file, even if the command exits successfully.

#### Scenario: Exact selection is absent, ignored without opt-in, or executes no passing test
- **WHEN** enumeration is non-unique, an ignored test was not explicitly enabled, or execution does not prove exactly one successful non-ignored test
- **THEN** the runner SHALL fail even when Cargo exits successfully.
