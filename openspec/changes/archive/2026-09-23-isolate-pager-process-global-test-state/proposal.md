# Change: Isolate Pager process-global test state

## Why

Pager unit tests that set `GROW_HOME` share the library test process with parallel environment readers. `config::grow_home()` caches its first resolution in a process-wide `OnceLock`, so serializing only the tests that set the variable does not isolate the first read and may direct trust or session writes to the wrong home. Theme and mouse-capture tests also reset mutable globals only on their success path.

## What Changes

- Run tests that require a private `GROW_HOME` as exact-test child processes with the variable set before the child starts.
- Make the shared session fixture consume that explicit child environment rather than mutating it.
- Reset theme and mouse-capture test state during unwinding as well as successful completion.
- Narrow the backlog entry to the remaining ignored leader-cluster fixture, which explicitly requires single-threaded execution.

## Impact

Test-only isolation changes. No user-facing behavior or archived contract changes; `skip_specs: true` records that no behavior contract changes.
