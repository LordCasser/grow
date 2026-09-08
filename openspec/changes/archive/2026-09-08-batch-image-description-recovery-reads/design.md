# Design
Collect uncached image group lookup identities for the current frozen transcript. One blocking storage operation reads and validates the parent Timeline and all required Sidebands, selects the newest completed matching result for each query, and returns only descriptions with provenance. Do not keep full historical ledgers alive across provider awaits, introduce a persistent cache/index, or weaken validate_sideband_ledgers.

An all-cached pass needs no storage lookup. Newly generated results remain in the existing in-memory cache. A read error remains a failed durable reuse attempt and follows existing provider fallback; it must never fabricate a recovered result. A single snapshot does not claim a transaction across simultaneously appended files; current storage validation remains authoritative.

Tests must exercise multiple distinct queries, wrong prompt/revision/source, newest completed selection, unrelated-ledger corruption preservation and one load per pass. Scope excludes full-history byte quotas, OS filesystem hard deadlines and cross-process snapshot redesign.

# Implementation details
The storage API takes ordered (SurfaceId, prompt) queries plus the shared frozen source revision, returning ordered optional results. Existing selection is factored into a private pure lookup over validated history. The actor submits uncached queries once before the provider loop and inserts recovered outputs into its existing cache. No new index or persistent entity. History load/validation occurs outside the query iterator; matching itself still scans candidates per query. The one-load property is directly visible at this boundary, not established by an IO-count instrumentation test.
