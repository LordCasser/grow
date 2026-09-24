## Why

The remaining sampling-attempt memory question needs an end-to-end measurement of the built binary, including one small streamed response as a baseline and one multi-megabyte streamed response.

## What Changes

Add an ignored macOS built-binary benchmark that runs `grow` under `/usr/bin/time -l` against the existing mock Chat Completions SSE server. It reports maximum resident set size for one small response and one approximately 12 MiB response, asserting each run succeeds and makes exactly one inference attempt.

After the remaining attempt-period Grow queue paths are closed by their own archived changes, record the measured peak and remove the resolved preview-memory item from `openspec/backlog.md`.

This is measurement-only: it does not change runtime behavior or a user-facing contract, so `skip_specs: true` applies.
