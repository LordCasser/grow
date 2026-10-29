## Why

The five shared-session PTY fixtures launch `--no-exit-on-disconnect` leaders through their clients but do not own the detached leader process. The leaders remain alive after the test sandbox is removed.

## What Changes

- Start the dedicated leader through the existing `test_support::TestProcess` owner in the PTY harness.
- Point PTY clients and the owner at the same isolated leader socket and sandbox.
- Let the owner shut down its concrete process on normal scope exit and unwinding; never signal a PID discovered only from a lock file.

## Scope

This changes test resource ownership only. Product leader persistence semantics and user processes are unchanged; `.openspec.yaml` sets `skip_specs: true` and adds no behavior delta.
