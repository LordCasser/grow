# Route child transcript file feedback to its source view

## Why

`/export` already snapshots the visible child Agent's scrollback and cwd, but explicit-file job admission and completion look only in the top-level `app.agents` map. A child export writes the right bytes yet shows no saving, success or failure notice in the child. Switching views can hide that omission.

## What changes

- Resolve file-job notices by the frozen Agent ID and session ID across root and child views, regardless of which view is active when the job finishes.
- Keep a removed or rebound child from receiving stale feedback, while preserving the serial file queue and actual output destination.
- Verify Minimal child export, view switching, and removal/rebinding with source-specific content and feedback assertions.

No file I/O or generic session switching behavior changes.
