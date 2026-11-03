# Align the Minimal quit PTY confirmation fixture

## Why

The Minimal PTY test waited for the first Ctrl+D confirmation hint before sending the second key. `wait_for_text` pumps PTY output until the hint appears, so under a loaded runner that observation can consume the product's one-second confirmation window. A later Ctrl+D correctly arms a new confirmation instead of quitting.

## What Changes

- Keep asserting that the first Ctrl+D renders the confirmation hint.
- Send Ctrl+C to clear that arm, wait for its hint to disappear, then write the two Ctrl+D bytes together to the PTY.
- Preserve the existing process-exit assertion and five-second exit deadline.

This is PTY fixture maintenance only. It does not change the one-second quit contract or product behavior, so this change uses `skip_specs: true` and has no delta spec.
