## Why

The leader buffers a retractable candidate for clients that cannot discard provisional output. Its vector also retains unrelated live notifications until the candidate ends. Those notifications can arrive independently of the provider response cap, so a long attempt can grow the leader's resident buffer without a limit.

## What Changes

- Keep only candidate notifications in the leader's retractable buffer.
- Deliver independent notifications live to all observers, including observers that cannot retract a candidate.

This changes live presentation order when independent notifications interleave with a later accepted candidate. It does not change model execution, canonical admission, or durable replay. The separate question of a strict serialized-candidate ceiling remains a distinct resource boundary.
