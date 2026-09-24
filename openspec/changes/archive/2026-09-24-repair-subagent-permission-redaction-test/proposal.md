## Why

The ACP handler regression test still expects the raw Bash command and classifier details in parent scrollback, although the archived `redact-bound-subagent-permission-details` change requires those audit fields to be redacted.

## What Changes

Update the stale test assertions to require the redacted command summary and reject raw request details and classifier text. This is test maintenance only; runtime behavior and the archived contract do not change.
