## Why

Session `01a1c301-0971-7f20-af72-29e37f07151b` showed two old background commands as Running after resume. Each command had a distinct task ID and a durable `CommandsSettled` record from an earlier suspension. The Tasks history projector restored their starts, but suspension did not write the ordinary presentation completions it relies on.

## What changes

Restore a terminal Tasks row from its durable command settlement in the selected session history. Cover Ctrl+D suspension and cold resume in a real terminal test.

## Capabilities

### Modified Capabilities

- `client-surfaces`: Restore background command terminal state after suspension.
