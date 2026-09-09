## Why
Goal-owned children fail before TurnStarted with InvalidTurnIdentity: shared admission supplies goal_id but the empty child Goal tracker cannot supply definition_revision. This is adjacent to, but distinct from, descendant admission-window synchronization.

## What Changes
Use the matching immutable inherited Goal context to complete child turn ownership. Preserve strict Timeline pair validation and root-local Goal lifecycle ownership.
