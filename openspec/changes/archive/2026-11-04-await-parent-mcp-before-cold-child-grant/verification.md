# Verification

## Reproduced failure

Linux candidate `f1ce5f35661ffe620855bb218656072b6e669ce4` failed the real cold birth-authority lifecycle in [Core run 41679387890](https://github.com/LordCasser/grow/actions/runs/41679387890): child pool `{}` instead of `{birth}`. The parent snapshot happened before its background MCP transports were published; subsequent parent readiness could not change the frozen empty child binding. The controlled narrow restore also failed before the production fix, 0 passed / 1 failed, `/tmp/grow-260-parent-mcp-gate-before.log` (54.12s).

## Local regression evidence

- Original five birth cases after the initialization wait: 1 passed / 0 failed, `/tmp/grow-260-parent-mcp-gate-after.log` (128.46s), covering the real independent-process fault phases and original permission/identity assertions.
- Six birth cases after the publication-owner barrier, including an explicitly rejected initialize RPC: 1 passed / 0 failed, `/tmp/grow-260-parent-mcp-final-exact.log` (151.26s). Failed client objects can remain registered under the existing recovery policy; the rejected tool must not succeed or dispatch, and the extra server remains forbidden.
- Final caller cancellation wiring and probe: real narrow `suspend` 1/1 (4.46s), then `restore` 1/1 (6.24s), `/tmp/grow-260-parent-mcp-final-cancel-suspend.log` and `...-restore.log`. During held initialize, the immediate pool request responds, a separate initialized snapshot remains pending and then finishes as None after its caller token is cancelled; the original restore later retains birth and completes all real call/descendant assertions.
- Shell library: 4572 passed / 0 failed / 9 ignored, `/tmp/grow-260-parent-mcp-final-shell.log` (322.63s). This preceded the final publication-lock/caller-token refinements; the final targeted lifecycle and required exact-source CI cover those refinements.
- MCP library: 179 passed / 0 failed, `/tmp/grow-260-parent-mcp-final-mcp.log`. Unix parser selectors 2/2 and background selector 1/1 passed in the corresponding `...-unix-*.log` files.
- Final workspace all-target check and formatting: PASS, `/tmp/grow-260-parent-mcp-final-cancel-workspace.log` and `...-format.log`.

## Scenario review and boundaries

Slow initial discovery is exercised with the existing HTTP MCP server held on a watch gate; no production deadline was widened. Current narrower and missing/invalid birth facts retain their existing negative assertions. Rejected discovery settles and keeps the tool unavailable. Cancellation is exercised through the real initialized-snapshot RPC. A separate read-only review confirmed matching publication-lock ordering, the immediate ordinary Spawn path, and RestoreBlocked retaining the original checkpoint.

Parent actor closure is checked by source: sideband cancellation or a closed responder drops the reply, the handle returns None, and cold preparation returns RestoreBlocked rather than an empty grant. There is no separate fault injection closing a parent with that waiter pending; this is a stated verification boundary, not a claimed dynamic pass. Native CI and official publication remain tracked by the release closure.
