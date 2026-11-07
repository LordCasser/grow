# Cold restore authority fixture readiness

## Scope and status

2026-11-07：本项 Rust source READY，等待父 owner 独占重编 Shell 并复验最终源码。只修改 `crates/codegen/shell/src/agent/mvp_agent/tests/subagent_restore_authority_tests.rs` 的 gated cold-load 观察阶段；本记录独立于父维护的 `verification.md`。该 fixture 修正归入现有 `prepare-release-v2-6-2`（`skip_specs: true`）：不改变产品契约、生产代码、权限逻辑、共享 sleep helper、依赖、其他测试或五秒退出 budget。

## Evidence and readiness fact

已有全量证据 `/tmp/grow-262-core-final.log:14375`–`14383`：实际 narrow cold restore phase 0.53s 即失败，断言为 root command executor 必须在 MCP initialize gate 关闭期间回答 pool snapshot；Shell 汇总 4789 passed / 1 failed / 13 ignored。隔离同 binary 的旧源码探测 `/tmp/grow-262-restore-authority-probe.log` 为 1 passed / 0 failed，31.87s；该单次通过不能排除竞态，也不是修改后验证。本项没有重复运行旧 binary。

源码顺序建立的唯一 readiness fact 是 **root handle 已由 load owner 发布到 control lookup 使用的 map**：

- `session/actor/spawn.rs:3458` 启动 session-local `run_session`；`actor/run_loop.rs:1105` 启动后台 MCP 初始化，它可以向 fixture 发出 initialize 请求。
- `session/actor/spawn.rs:3985` 才通过 `init_tx` 把 handle 交给 load owner。
- `agent/mvp_agent/agent_ops.rs:2083` 等待 `InstallCoordinationBackend` ACK，之后在 `:2119` 把 handle 插入 `sessions`。
- `agent_ops.rs:826` 的 `control_session_handle` 仅查询当前 `sessions` / `active_child_sessions`，不等待发布。`acp_agent.rs:1235` 先 await spawn/register，再于 `:1309` 执行树恢复。

所以 HTTP initialize 通知与 map publication 没有“通知之后已发布”的先后保证。旧 fixture 收到通知后停止驱动 `load_future`，立即查 map；合法的 `None` 会被当作 command executor 不响应。旧失败日志将这个分支和即时 snapshot 的 100ms 超时合并成同一断言，无法从该日志认定具体命中了哪一支；这里只确认源码证明的可达 fixture race，不声称重现了失败调度。

归档契约 `openspec/specs/tool-authorization/spec.md` 的 “Cold child grants wait for initial parent MCP discovery” 要求 discovery 等待期间 command executor 可响应，没有定义 100ms 响应延迟。原设计 `openspec/changes/archive/2026-11-04-await-parent-mcp-before-cold-child-grant/design.md` 同样要求 gate 关闭期间即时 snapshot 可响应。

## Implementation and preserved assertions

- initialize 已到达但 root handle 未发布时，保持 handshake gate 关闭，在既有 `cold_recovery_stage("authority.cold_load", ...)` 的 10 秒总体期限内循环查 root control handle，同时 `select!` 驱动 `load_future` 与 `yield_now`。若 load 提前结束，保留其实际结果，并让原 pending/response 断言失败；不重复 poll 已完成 future。
- 发布完成后才发即时 pool snapshot。正向 liveness 响应直接 await，使用同一个既有 stage deadline，不引入新 budget，也不把任意更大的毫秒数用作替代标准。若 executor 被握手阻塞，gate 不会先被释放，该 stage 仍超时失败。
- 保留 initialized snapshot 的 100ms pending 观察、调用方 cancellation 与 100ms 返回检查；保留 cold load 的 100ms pending 观察。gate 的唯一显式释放仍在这些观察之后、原断言之前，原失败清理意图保持。
- 保留 initialize reached、load remained pending、immediate snapshot responded、initialized snapshot cancelled 四项断言，以及全部 birth membership、pending permission、fresh identity/permit、descendant、forbidden extra、missing/empty/unavailable 等原有场景。格式整理限于本测试阶段。

## Verification and handoff

最终测试文件 SHA256：`7278dcd5d3b55fcfdeb056e4e616bdd96be2b65c0674965f7e8bbc5b98584c57`。

已执行：

- `rustup run 1.93.1 rustfmt --check --edition 2024 crates/codegen/shell/src/agent/mvp_agent/tests/subagent_restore_authority_tests.rs`：exit 0，检查最终文件格式及可解析语法。
- `git diff --check -- crates/codegen/shell/src/agent/mvp_agent/tests/subagent_restore_authority_tests.rs`：exit 0。
- 最终 diff 自查：仅上述 gated stage 发生 Rust 改动；没有生产变更；源码已经冻结，不再与父编译窗口交叉修改。

OpenSpec 全量 strict 校验：`openspec validate --all --strict --no-interactive` exit 0，23 passed / 0 failed，日志 `/tmp/grow-262-authority-fixture-openspec.log`。未运行 Cargo、编译、测试 binary 或概率重试；当前不能声明修改后测试通过。父 owner 负责使用冻结后的最终源码重编 Shell，运行 exact lifecycle / Shell full suite 并将结果并入发布证据。若重编后仍失败，应依据新的具体阶段和断言继续归因，不能用旧隔离通过覆盖全量失败。
