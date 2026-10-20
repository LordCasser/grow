## ADDED Requirements

### Requirement: Exact Cargo selections stay pinned to one compiled test executable

Exact Cargo test selection SHALL compile the selected package/target once, identify the unique test executable from Cargo JSON compiler-artifact messages where `profile.test` is true and `executable` is nonempty, and invoke that same executable directly for test enumeration and execution. Cargo diagnostics and progress SHALL remain readable; artifact JSON SHALL NOT obscure the bounded diagnostic tail. Missing or ambiguous executable selection SHALL fail closed. Existing exact-name, ignored-test opt-in, successful execution, phase deadline and process-tree cleanup checks SHALL remain in force.

#### Scenario: One compiled artifact serves all exact-test phases
- **WHEN** a Cargo no-run build produces exactly one test executable
- **THEN** the runner enumerates and executes the selected test through that same executable without invoking Cargo again, and still proves one exact match and one successful test.

#### Scenario: Test executable is missing or ambiguous
- **WHEN** Cargo produces no eligible test executable or produces more than one unique eligible executable
- **THEN** the runner fails before enumeration and execution.

#### Scenario: Artifact JSON is streamed without hiding diagnostics
- **WHEN** the compile stream contains compiler artifacts and compiler messages
- **THEN** artifacts are captured independently of diagnostic-tail size, rendered diagnostics and Cargo progress remain visible, and compiler messages without rendered text retain their raw content.

#### Scenario: Test executable keeps its package working directory
- **WHEN** the selected package is a member below the Cargo workspace root
- **THEN** enumeration and execution run with the package manifest's parent directory as their working directory.
