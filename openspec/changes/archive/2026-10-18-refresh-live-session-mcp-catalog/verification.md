# Verification — 2026-10-18

## Scope and root cause

- 仅修复并验证源码；未替换本机可执行文件、重启真实 session 或连接用户配置的 MCP。
- Pager in-process consumer 原先只接受 `ModelsChanged`，丢弃 `McpCatalogChanged`；也未将动态 session cwd 接到既有 watcher。
- 迟建 `.grow` 的端到端回归另暴露路径身份问题：macOS 目录别名或符号链接 cwd 与 filesystem event 的规范路径不同，导致 watcher 目录维护和 reload 范围匹配失败。
- 修复复用 Shell catalog reload、配置合并、信任过滤、MCP diff/init/ToolBridge 注册及现有状态推送，不新增 catalog owner。

## Isolation

当前工作区包含其他进行中变更。工作区命令 `cargo test -p pager --lib acp::spawn::tests --locked --offline` 在无关 planning 接口处编译失败，包括 `tool_types::planning`、`TaskToolInput.parent_step_id` 与 `SubagentCompletedOutput.planning_summary`。日志：`/tmp/grow-mcp-refresh-green-workspace.log`。不将其记为本次改动通过整体验证。

可执行回归在 HEAD `8cc20454da9e62874170fb3ed1cc532591954947` 的临时 `git archive` 快照上仅叠加本次生产变更及测试。Cargo 使用共享 target、4 个 build jobs、关闭 incremental。测试完成后清理临时快照及本次测试可执行文件和对应对象文件。

ACP 场景复用 `run_with_isolated_grow_home`，每项在独立 exact-test 子进程中运行，避免进程级 GROW_HOME OnceLock 串扰；只连接临时本机 HTTP MCP mock，不发模型采样请求。

## Reproduction and passing regressions

原 `spawn.rs` 加入相同回归：2 项均在新增 `global-server` 等待 ready 时失败；原 client server 已 ready 且可调用，新目录行仅配置可见、session disabled。日志：`/tmp/grow-mcp-refresh-red.log`。

修复后：

```text
cargo test -p pager --lib acp::spawn::tests --locked --offline
6 passed, 0 failed

cargo test -p shell --lib --locked --offline -- config::reloader config::watcher session::mcp_catalog cwd_matches_resolves_aliases --test-threads=1
25 passed, 0 failed
```

日志：`/tmp/grow-mcp-refresh-green.log`、`/tmp/grow-mcp-refresh-shell.log`。

| Scenario | Evidence |
| --- | --- |
| 已有 session 新增 global MCP | 真实 initialize、tools/list，ToolBridge 中 echo 可见，ACP direct call 返回原值 |
| 动态 cwd、迟建项目目录 | 第二个 session 通过符号链接 cwd 创建，之后建立 `.grow/config.toml`，仅该 session 能看到并调用 project echo |
| 目录别名及监听移除 | watcher alias 单测验证迟建、后续修改和按 alias unwatch；范围单测验证双向 alias 匹配及 sibling 拒绝 |
| 监听关闭 | project watch disabled 时 global 新增仍可调用，project 服务不握手、不 ready |
| 幂等 | 相同 global 配置重写不增加 initialize 次数 |
| client 优先级 | global 与 initial client 使用相同 URL 合并键，client 名称、工具和原连接保留 |
| disabled、信任过滤 | disabled mock 不握手；既有 untrusted workspace 合并回归通过 |
| 取消退出 | 明确断言 worker Joined；退出后新增 enabled 配置不发起 initialize |

## Contract checks

归档前 `openspec validate --all --strict --no-interactive`：22 passed，0 failed。归档后同一命令：21 passed，0 failed；`openspec validate --archived --strict --no-interactive`：584 passed，0 failed。日志：`/tmp/grow-mcp-refresh-spec-after.log`、`/tmp/grow-mcp-refresh-archived.log`。

## Current workspace follow-up — 2026-10-19

重新核对 MCP 补丁后，在当前工作区执行：

```text
cargo test -p pager -p shell --lib --locked --offline -- acp::spawn::tests config::reloader config::watcher session::mcp_catalog cwd_matches_resolves_aliases --test-threads=1
```

编译仍在无关 planning 接口失败：`chat-state` 3 个错误、`tools` 13 个错误，包括 planning 类型导出、`TaskToolInput.parent_step_id` 和 `SubagentCompletedOutput.planning_summary`。本轮未进入 MCP 测试执行，不替代上面的隔离基线通过记录。日志：`/tmp/grow-mcp-refresh-workspace-20261019.log`。本轮未改变生产源码或本机可执行文件。
