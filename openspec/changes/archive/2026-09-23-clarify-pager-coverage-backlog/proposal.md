# Change: Clarify pager coverage backlog

## Why

The Pager backlog describes ignored coverage in broad categories, which can imply gaps are still uncovered even where active neighboring tests exist. Record the concrete ignored PTY cases and the commands that select their integration targets.

## What Changes

- Narrow the two Pager coverage bullets in `openspec/backlog.md` to current ignored test names and validation entry points.
- Keep the interactive pager suspend/restore limitation explicit because `PAGER=cat` does not exercise the child pager lifecycle.

## Impact

Documentation only. No behavior or archived contract changes; `skip_specs: true` is used because this change records existing test status and does not alter requirements.
