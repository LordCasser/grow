# Verification

## Contract and implementation

- Descendant ownership v2 carries the validated typed `SubagentSpawnEvent`, its parent sequence, and any linked terminal event. A child session is read once and checked with `Timeline::validate_subagent_seed_link` and, when linked, `Timeline::validate_subagent_result_link`; the captured root ownership and already-read parent are reused during BFS.
- Before dispatch, Pager checks each typed lifecycle fact's source envelope and canonical parent/subagent/child tuple. Traversal adds only a canonical owned Spawn.
- For replayed ID-less Spawned updates, Pager reuses the existing Started row's entry ID for that child. Event-ID deduplication and finished-row behavior remain in place.
- All projections remain display state. They do not issue child execution or resume authority.
- `audit.md` records the separate `StreamingMarkdownRenderer::clone` finding and caller evidence: there is no production hot path requiring a clone fix.

## Test results

- Pager suite: **7,620 passed, 0 failed, 16 ignored** (`/tmp/grow-2.5.2-pager-final3.log`). It includes `descendant_lifecycle_with_mismatched_identity_is_not_projected`, `forced_restore_reuses_idless_started_row_for_canonical_descendant`, `descendant_restore_uses_owned_disk_layout_and_dedups_live_overlap`, and `precise_entry_navigation_matches_sticky_display_setting`.
- Shell suite: **4,413 passed, 0 failed, 9 ignored** (`/tmp/grow-2.5.2-shell-last.log`).
- Settings suite: **235 passed, 0 failed, 0 ignored** (`/tmp/grow-2.5.2-settings.log`).
- Native MCP reload probe: **1 passed, 0 failed, 0 ignored** (`/tmp/grow-2.5.2-mcp-probe.log`).
- Strict OpenSpec validation: **33 passed, 0 failed** (`/tmp/grow-2.5.2-specs-prearchive.log`).

The cross-entity cases use canonical parent/child Timeline facts and keep Seed/Result validation active. The standalone MCP probe had no ignored tests.
