# Change: Isolate Pager leader-cluster test environment

## Why

Pager's in-process leader-cluster scenarios mutate process environment and rely on `grow_home()` resolution. They are ignored and require serial execution because they share those process globals with the full library test binary.

## What Changes

- Run each ignored leader-cluster scenario through an exact-test child process with a temporary `GROW_HOME` provided before the child test harness starts.
- Keep the scenarios on-demand while allowing each selected scenario to run independently of unrelated process-global state.
- Remove the resolved leader-cluster environment isolation debt from the backlog.

## Impact

Test harness only. This does not change product behavior or archived behavior contracts; `skip_specs: true` records that no contract changes.
