# Design: Bind session picker list fetches to their view

## Decision

Store one `SessionPickerBinding` beside the existing current list sequence. It captures the root Agent ID (or Welcome), active child key, session ID, session binding epoch, and cwd. The effect receives the frozen cwd; the result continues to echo the sequence, which selects the one outstanding current binding without adding owner fields to every result variant. On delivery, compare the stored binding to the currently visible picker before touching entries, loading flags, notices, or detail generation. Dismissal clears the binding with the existing sequence invalidation.

The same cwd is used for the server request, selection anchor, and relaxed-scope notice latch. A change of Agent, child, session binding, or cwd makes the old result stale. The binding does not own a new data store or alter deep-search routing.
