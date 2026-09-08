## Why
Cold session load initializes the actor from model defaults, then routes persisted reasoning effort through an ordinary model switch. If the durable selection ends at high but the current default is max, that switch records max → high, violating the next replay's continuity check. This is a separate failure from the earlier initial-context shadow-set bug.

## What Changes
Initialize a newly loaded actor with the persisted effort before any model-control admission. Loading unchanged selection must not manufacture another model transition. Preserve strict rejection of discontinuous histories. Do not rewrite existing user sessions.
