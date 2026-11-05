## Why

`spawn_subagent` exposes `model` but not `effort`, although the existing subagent runtime already accepts a reasoning-effort override. The model cannot select both values for one delegated task.

## What Changes

- Add optional `effort` to the shared Task tool input and forward it to `SubagentRuntimeOverrides.reasoning_effort`.
- Reuse runtime parsing, selected-model capability validation, precedence, sampling configuration and durable/UI projection.
- Keep omission behavior unchanged and preserve the source model/effort when `resume_from` is used.

## Impact

Affected capability: model-sampling. Update shared tool metadata, Task dispatch tests and developer guidance. No new runtime, dependency or persisted format.
