# Defer historical Markdown presentation (P1)

## Why
P0 release attribution measured193.8/774.5ms of synchronous final Markdown parsing for512/2048-turn text history, about81% of client batch service. Offscreen blocks pay that cost even though existing layout renders only nearby entries. Backend RPC and visible output are different bottlenecks.

## What changes
Keep source/normalizer completion, message lifecycle, ordering, image-reference discovery and all load barriers synchronous. Materialize final Markdown presentation only when display/copy/link/source-map access needs it. Reuse existing renderer/wrap cache; use source-based offscreen height estimates so estimating does not force pretty parsing. Keep Mermaid finalization eager where a source may contain Mermaid to preserve existing diagram geometry with minimal machinery.

No persistence/protocol changes, new actors, background history, pagination, early input, SQLite or persistent caches. P0 root/descendant Timeline reread remains independent debt.
