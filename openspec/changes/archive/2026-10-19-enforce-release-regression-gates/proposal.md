# Bind release regression gates to the validated source commit

### Why

The release workflow validates an annotated tag and records its commit, but publish currently depends only on build, tag validation, and dispatch checks. Platform regression workflows are separate and can run against `main` or a later commit, so a green result does not prove the release source passed those regressions. Several Cargo exact-name loops also treat a successful zero-test run as a pass.

### What Changes

- Make the core regression workflow reusable for a caller-supplied commit and complete its relevant `main` push coverage.
- Make strict OpenSpec validation reusable for the same caller-supplied commit and require it before publication.
- Run session suspend/resume and plan approval resume regressions in the core workflow, plus production transcript export regressions across the native macOS/Linux/Windows workflow matrix, all on the same immutable commit selected by release validation.
- Run the real built-binary sampling failure/continuation and Goal restart regressions in that core gate, with explicit exact selection and its just-built binary.
- Require each exact Cargo selection to be uniquely listed and to execute exactly one test; missing, ambiguous, failed, skipped, or zero-test selections fail closed.
- Make release publication depend on successful completion of commit-bound regression and specification workflows, including explicit result checks that prevent skipped or cancelled gates from authorizing publication.
- Replace existing Windows exact-name shell loops with the shared precise runner.

## Capabilities

### Modified Capabilities

- `development-workflow`: release publication requires relevant regression gates to pass on the exact validated release commit.

## Scope

This change owns CI workflow definitions, the exact Cargo test runner, its focused Python tests, and the OpenSpec record. It does not change Rust product behavior, add broad PTY coverage, or claim that hosted Windows CI has already run.
