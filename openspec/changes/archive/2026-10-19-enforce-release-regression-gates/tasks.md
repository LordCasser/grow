## Implementation

- [x] Add exact Cargo test runner with list uniqueness and executed-count checks.
- [x] Add meaningful Python tests for missing/ambiguous selections, zero execution, and failing execution.
- [x] Make core regression reusable on an explicit verified commit and cover relevant main pushes.
- [x] Make OpenSpec validation reusable on an explicit verified commit.
- [x] Add the selected suspend/resume, plan approval, and cross-platform production export tests.
- [x] Include the three exact built-binary sampling failure/manual continuation and Goal recovery tests, using the core job's just-built binary.
- [x] Replace Windows exact test loops with the shared runner and complete main coverage for relevant workflows.
- [x] Gate release publication on successful same-commit regression completion, including skipped/cancelled result handling.

## Verification

- [x] Run the exact runner and release DAG Python unit tests.
- [x] Validate changed workflow YAML and inspect the release dependency/result conditions.
- [x] Run `openspec validate --all --strict --no-interactive`.
- [x] Record which hosted platform checks remain pending; do not claim local Windows execution.
