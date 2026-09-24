# Seed the mock model for the interactive minimal pager restore test

## Why

The ignored `minimal_transcript_pager_restore_no_artifacts` PTY test exits its readiness gate on a clean isolated home because the Grow startup BYOK gate requires a configured model. The test times out on “No LLM is configured” before it can exercise `less` or terminal restoration.

## What changes

Seed the test's isolated home with the harness mock-model config before spawning Grow, then rerun that exact PTY test. This is test-fixture maintenance only; runtime behavior and accepted contracts do not change, so `skip_specs: true` is intentional.
