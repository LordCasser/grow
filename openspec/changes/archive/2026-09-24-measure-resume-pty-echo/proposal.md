## Why

Shell's existing session-load benchmark measures the server path, but the backlog's p95 input-echo condition also includes Pager replay, layout, terminal writes, and the moment a resumed PTY becomes interactive. The current `--continue` PTY test checks correctness without recording those times.

## What Changes

- Add an ignored, isolated PTY measurement using the existing synthetic session generator and mock provider.
- Record elapsed resume-to-visible-history time and repeated prompt-key echo latency for 128- and 512-turn fixtures.
- Use the first measurements to identify the stage that needs a production fix before claiming the backlog condition.

## Capabilities

No behavior contract changes. This change only adds a measurement harness and validation notes; `.openspec.yaml` sets `skip_specs: true` because it does not change runtime behavior or existing requirements.

## Impact

Pager PTY integration tests and development instructions. The benchmark is ignored during normal test runs and uses an isolated Grow home and a local mock provider.
