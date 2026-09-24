# Design: Isolate Pager leader-cluster test environment

## Decision

Keep the in-process leader and `AppView` scenario implementation in the library test module, but make each ignored test a launcher for its own exact-test process. The launcher creates a temporary home, sets `GROW_HOME` in the child environment, and asserts that libtest reports the requested test as passed. The child marker prevents recursive spawning. Since only the selected test runs in that process, it may safely set the mock inference endpoint, API key, and leader socket for the scenario lifetime.

The child process is the isolation boundary because `config::grow_home()` caches its first result process-wide and environment variables are process-wide. `serial_test` only orders participating tests and cannot isolate these values from the rest of the library test binary. The harness also creates an explicit mock model configuration from the mock server URL; an isolated home must not depend on a developer's `config.toml`.

## Consequences

- The existing private harness seams and scenario assertions stay in place.
- The scenarios remain ignored because they exercise a real local agent/leader integration path and take substantially longer than unit tests.
- Selected scenarios can run in parallel from libtest: every scenario gets a distinct process, temporary home, mock server, and socket path.
- This change does not remove the separate ignored PTY coverage listed in the backlog.
