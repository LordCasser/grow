## Context

`restore_descendant_state` obtains validated child ownership and a typed lifecycle sequence from each source session. It currently sends every lifecycle entry through the forced display handler, then consults canonical child ownership only to decide whether a Spawned row extends the BFS. Typed presentation facts can be structurally valid while naming a different parent, subagent or child. The replay handler's Started-row deduplication also depends on immutable eventId, which older valid lifecycle records may not carry.

## Decision

Carry the exact typed canonical Spawn and any linked terminal Result facts in each validated ownership projection, then validate a child session using the existing Timeline Seed/Result validators in one child-session read before projecting its contents. This reuses the captured root load frontier and avoids reopening an already-visited parent during breadth-first traversal. For each lifecycle update, require its parent envelope and `(subagent_id, child_session_id)` to match the source session's validated direct-child ownership before dispatch. Keep BFS traversal limited to those same canonical child IDs.

For ID-less replayed Spawned updates, search the existing scrollback for a Started row with the same child session ID and reuse that row's entry ID. Retain eventId deduplication for identified updates and finished-row behavior.

## Consequences

Unowned or cross-linked lifecycle presentations cannot add descendant display rows or traversal edges. Repeated valid ID-less restore preserves one Started row. Presentation recovery remains observational and does not authorize execution, resume, or result acceptance.
