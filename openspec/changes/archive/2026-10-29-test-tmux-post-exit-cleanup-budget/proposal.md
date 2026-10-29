## Why

The tmux probe regression uses a 1.2-second scripted leader under a 1.5-second process deadline. Its 300 ms nominal remainder is larger than the 100 ms TERM wait, so the test does not distinguish the fresh post-exit drain budget from reusing the process deadline.

## What Changes

- Replace the elapsed-time assertion fixture with a controlled leader/descendant pipe fixture.
- Exercise the shared post-exit cleanup path with an already-expired process deadline; verify captured output and observe the descendant's TERM handler before it exits under the existing cleanup allowance.
- Leave production deadline durations and timeout behavior unchanged.

## Scope

This is a test-only clarification of existing process cleanup behavior. `.openspec.yaml` sets `skip_specs: true`; no contract delta is added.
