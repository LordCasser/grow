## Why

The Tasks pane and the opened subagent title show a child's model but omit its effective reasoning effort. The durable spawn fact already records that effort, so the UI currently hides information needed to distinguish otherwise identical child routes.

## What Changes

- Project the effective spawn effort with the child lifecycle notification, including reconnect replay.
- Show `model (effort)` in the existing right-aligned Tasks row and after the description in the opened subagent title. Omit the suffix when no effort is known.
- Keep the parent row synchronized with authoritative child model and effort changes.
