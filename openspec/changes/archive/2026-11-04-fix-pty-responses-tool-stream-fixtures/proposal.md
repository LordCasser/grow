## Why

The shared PTY tool-call fixtures emit Responses argument deltas before announcing the corresponding function-call items. The production Responses parser correctly rejects those streams, so tests that happen to use the Chat Completions twin do not exercise a valid Responses tool-call path.

## What Changes

- Repair the single-tool Responses fixture by reusing the complete test-support stream.
- Make the parallel Responses fixture announce each function-call item before its argument delta.
- Exercise the shared fixtures from PTY cases configured to use the Responses backend.
- Run the PTY cases from a project subdirectory under the fake HOME, keeping configuration in HOME without triggering the CLI's Home-directory onboarding prompt.

This is test-fixture maintenance only. It does not change product behavior or any archived contract, so this change uses `skip_specs: true` and has no delta spec.
