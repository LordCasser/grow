## Why

Two Pager input boundaries remain broken. After a completed bracketed paste, `coalesce_rapid_keys` can turn later ordinary `a`, Enter, `b` keys into a second Paste, although the current client contract requires each following key to retain its own event. In file completion, a directory drill clears visible results while a new query runs; Escape is only dispatched while the dropdown is visible, leaving the active `@` context and drill anchor behind.

## What Changes

- Keep every event following a completed bracketed Paste in the same input batch on its ordinary routing path.
- Allow Escape to dismiss an active file-search context while results are empty or pending.
- Replace the obsolete paste assertion and verify both boundaries with focused regressions.

The existing paste and current-query requirements remain authoritative; the delta adds the exact cases that exposed the gaps.
