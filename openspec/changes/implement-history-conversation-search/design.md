# Scope and constraints
Extend actual history search beyond prompt recall to searchable past conversation content/results. Inspect existing storage/search and selection APIs before deciding the minimal implementation. Preserve navigation and existing prompt recall. No new parallel storage authority without evidence.

Detailed implementation decisions and delta scenarios must be completed before corresponding code changes.

## Existing foundations
`views/history_search.rs` is prompt recall with a background nucleo matcher and text selection. `session/storage/search.rs` and `search_fts.rs` already provide conversation indexing, bounded bootstrap and query orchestration. Inspect existing Dashboard/RPC search consumers and reuse this index; do not create a second conversation database. The selected() stub alone cannot deliver conversation-result search.

Current `/history` ignores arguments and opens prompt recall; `/resume` opens the existing session picker. The picker already queries `grow/session/search` with includeContent=true and displays results. Prefer a discoverable history conversation-search entry that reuses this picker and its debounced async search, preserving keyboard prompt recall. Verify actual result snippets, selection/resume semantics and disabled-index feedback before finalizing the command surface.

## Implementation decision
`/history [query]` reuses ShowSessionPicker with an initial query; `/history --prompts` preserves explicit prompt recall and Up-arrow recall remains unchanged. No new state owner or database. Seed all deep searches from the existing AppView sequence so reopening a modal cannot reuse request identities. Existing content snippet rendering and session selection/resume are retained. Propagate a bounded, sanitized search error in DeepSearchResults and apply it only after checking current picker liveness/sequence. Update the user command guide. Remove the uncalled selected() stub only; selected_text stays authoritative.
