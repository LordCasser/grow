# Design

## Commit identity

The release `validate` job remains the source of truth for the tag's checked-out commit. It passes that full commit hash to the reusable core, native coordination, Windows storage, and OpenSpec validation workflows. Each reusable workflow checks out the supplied immutable hash and verifies `HEAD` equals it before running. Normal PR, `main`, and manual runs use their event commit. Gates do not depend on the latest state of `main`.

The release publish job depends on `validate`, `select`, `build`, `dispatch-guard`, and every reusable regression/specification gate. Its condition requires that the workflow has not been cancelled and that every dependency has result `success`; a failed, skipped, cancelled gate, or cancelled workflow cannot publish.

## Exact test execution

The shared Python runner accepts a Cargo package, either `--lib` or one integration-test target, an exact fully-qualified test name, and optional `--ignored`. It first runs Cargo's test listing and requires one exact matching entry. It then runs the test with `--exact`, checks the command status, and requires Cargo to report exactly one executed test. This catches stale names even though Cargo exits successfully when filtering selects nothing.

The runner's unit tests exercise the listing and execution protocol with controlled Cargo outputs, including missing/ambiguous names, a zero-test success, and a failing test. Existing Rust tests remain the behavior-level evidence.

## Workflow coverage

The reusable core regression workflow includes the library checks already used by CI plus selected essential suspended-session and plan-resume PTY scenarios. Local coordination retains its process-level regression and runs exact `export_cmd` tree and `trajectory_cmd` production HTTP archive tests on its native macOS/Linux/Windows matrix. The dedicated Windows storage job uses the same exact runner for each selected storage scenario. Hosted release invocation runs the reusable workflow over the release commit; local verification can validate the workflow contract and runner behavior, but cannot claim a hosted Windows run.

The core workflow also invokes the built-binary `test_session_protocol_recovery` cases through the exact runner with `GROW_BINARY` pinned to its just-built application. These cover three-attempt malformed generation, deterministic malformed wire JSON, live-session manual continuation and Goal pause/restart budget gates. The behavior and focused child-budget/terminal-ACK regression belong to `recover-session-after-sampling-failure`; their library and binary cases run on this same release commit.
