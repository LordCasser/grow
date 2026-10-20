# Design

The compile phase adds `--message-format=json` to the existing exact package/target `cargo test --no-run` command. The existing streaming line parser recognizes compiler-artifact messages, records only nonempty executables whose profile has `test: true`, and consumes artifact JSON without adding it to the diagnostic tail. `compiler-message.message.rendered` remains readable; when rendered text is absent, retain the raw diagnostic. Plain Cargo progress such as Compiling/Finished remains visible.

Compilation succeeds only when exactly one unique test executable is recorded. The list phase invokes that executable with the selected test name and `--exact --list`. The execute phase invokes the same path with the selected test name and existing exact/format/color/thread/ignored arguments. Both phases retain their independent timeout and process-tree cleanup behavior. Existing result parsing still requires exactly one listed name and exactly one successful, non-ignored test.

The real fixture writes each build-script invocation to a log and changes a watched input after each run so a second Cargo command would rerun the script. Its regression requires one invocation across all three phases.

## Validation

Run the exact-runner unit and real-fixture suites, then the complete `scripts/tests` suite under the supported Python versions. Validate OpenSpec strictly. No large-workspace Cargo build is part of this change.
