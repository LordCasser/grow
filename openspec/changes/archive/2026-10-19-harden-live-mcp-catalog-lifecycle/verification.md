# Verification

Date: 2026-10-19. Source-only task. No installed executable, user MCP configuration, or real session was changed. Tests use isolated GROW_HOME, temporary repositories, loopback mock MCP servers, and an ACP reverse-invoker fixture. Existing negative core tests additionally use documentation-range unreachable endpoints; no configured user MCP server or model provider was invoked.

## Build boundary

The current shared worktree has unrelated in-progress planning changes. The combined workspace command failed before these tests could run with missing `tool_types::planning` / planning exports, missing TaskToolInput fields, and SubagentCompletedOutput field errors. These were not changed to make this task compile. Failure log: `/tmp/grow-mcp-lifecycle-workspace.log`.

To validate this task independently, an isolated source archive at `8cc20454da9e62874170fb3ed1cc532591954947` was overlaid with the accepted prior in-process MCP refresh fix and this audit's source/test changes. For heavily modified ACP/session files only the MCP sections were overlaid; unrelated workspace work was excluded. This is evidence for the MCP changes on that baseline, not a claim that the whole current dirty worktree builds.

All commands used the shared target directory, `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`, `RUST_MIN_STACK=16777216`, and `--locked --offline`. Only this task's temporary sources/test outputs are cleaned after verification.

## Regression commands

Pre-fix reproduction (pre-audit runtime, first three lifecycle tests): 2 passed, 1 failed. The resident-load empty-catalog test reproduced retention of the old client-only server. CRUD and basic cold recovery passed. No claim is made that later-added tests were run red.

```sh
cargo test -p mcp -p shell -p pager --lib --locked --offline -- \
  servers::tests acp::spawn::tests app::acp_handler::tests::mcp \
  mcp_catalog_ accepted_client_mcp session::mcp_dispatcher \
  session::mcp_restart config::watcher config::reloader --test-threads=1
```

Final result: **268 passed, 0 failed** (MCP core 158, Pager 30, Shell 80). Log: `/tmp/grow-mcp-lifecycle-regression.log`.

New adversarial cases cover six Pager lifecycle tests (global/project CRUD; repository ancestors; resident empty reconnect; cold recovery/current inputs/watcher reattachment; SSE tools-list notifications with auto-restart disabled, schema replacement and empty catalog; blocked old initializes), three actor catalog tests (full schema/search agreement and actual model bridge invocation after recovery; old discovery completion; retirement/bridge restoration), client-source admission during paused acknowledgement, shared ancestor unregistration, dispatcher-discovery cancellation, and retired-call late failures under removal/replacement. Existing core transport generation/timeout/cancellation/retry tests and session dispatcher/recovery tests are included in the final command.

## Static and contract checks

Targeted rustfmt checks and changed-file whitespace checks passed. Shared files with unrelated formatting were checked only on the new MCP blocks, without reformatting unrelated work.

`openspec validate --all --strict --no-interactive` passed before archive (22 items). `openspec archive harden-live-mcp-catalog-lifecycle --yes` merged the lifecycle requirement and archived this change. Post-archive all and archived validation passed; **21 active/spec items + 585 archived changes passed**.

See audit.md for explicit boundaries and separate policy/identity debt. Test fixture corrections during development included the constructor's actual Pending state, intentionally dropped oneshot receivers, unavailable Debug output, and omitted empty tools vectors. None removes an adversarial scenario or changes a production failure into success.

Cleanup: removed the isolated source snapshot and pre-audit overlay copies, and 390 task-specific test executable/object/dependency files (2358848574 bytes; about 2.20 GiB). Shared libraries, unrelated builds, and diagnostic logs were preserved.
