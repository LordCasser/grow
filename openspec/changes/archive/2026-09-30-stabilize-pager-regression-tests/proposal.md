## Why

Two Pager library tests give false failures in the current workspace. The Settings registry correctly includes the archived `show_model_provider` row, but its hard-coded list expectation omits it. The inline-media pending-limit test races other tests for the process-wide worker permits, so it sometimes observes one admitted request instead of two even though the per-view limit remains correct.

## What Changes

Update the Settings expectation to include the registered row. Make the per-view pending-limit test construct an already-saturated view directly, then assert another request is neither pending nor failed. This does not change runtime behavior or any contract, so the change skips delta specs.
