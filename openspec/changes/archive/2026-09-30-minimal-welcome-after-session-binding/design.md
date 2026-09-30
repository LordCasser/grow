# Design

The session ID is part of `TranscriptOwner`, which drives Minimal's visible-epoch clear and print frontier. `dispatch_new_session_inner_with_id` queues the welcome card while the new Agent still has no session ID. The renderer may draw that placeholder before `handle_session_created` binds the ID. A later owner transition clears the visible card without resetting `welcome_pending`.

The Minimal API seam will expose whether the visible root Agent has a bound session. `maybe_commit_welcome` will leave the pending flag untouched until that condition holds. The subsequent frame first establishes the bound owner's epoch, then commits the card before conversation blocks. Existing width and terminal-write retry behavior remains in place.
