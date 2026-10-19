# Verification

## Current verification — 2026-10-19

The interrupted overall Shell run `/tmp/grow-v2-4-0-shell22.log` individually reports both required cases as passed:

- `agent::mvp_agent::tests::mcp_child_policy_reload_tests::internal_reload_fans_out_policy_only_command_to_matching_child ... ok` exercises the real reload handler with cwd scope, confirms the matching active child receives `UpdateMcpServers` with an empty catalog, and confirms an out-of-scope child receives nothing.
- `session::actor::tests::mcp_catalog_lifecycle_tests::subagent_auxiliary_refresh_updates_local_gate_without_mutating_shared_client ... ok` covers application of the policy-only refresh without mutating the shared client.

The parent later terminated the Shell test process because a separate compaction test hung; this log has no Shell suite total and is not a full-suite pass. The two named child-policy tests completed before termination. Strict OpenSpec validation `/tmp/grow-v2-4-0-openspec-all29.log` reports **31 passed, 0 failed**, including this change. The scoped fanout, actor path, and recorded verification tasks are complete.

The separate MCP library run `/tmp/grow-v2-4-0-mcp-lib-tests3.log` reports **178 passed, 0 failed** as a library regression baseline; it does not substitute for the two named child-policy tests above.
