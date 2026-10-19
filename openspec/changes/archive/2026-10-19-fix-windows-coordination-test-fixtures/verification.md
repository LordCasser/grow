# Verification

## 失败证据与范围

[Windows 协调 job 117882718528](https://github.com/LordCasser/grow/actions/runs/39197297359/job/117882718528) 的 Shell coordination filter 为 **56 passed / 2 failed**。本机日志 `/tmp/grow-inquiry-windows-coordination-ci.log` 保留原始 job 输出。

- `ask_session_tool_blank_ids_create_inquiries_and_explicit_ids_remain_strict` 在公共 helper 构造阶段 `AbsPathBuf::new("/tmp")` 返回 `NotAbsolute`，尚未测试工具调用。
- `coordination_observer_preserves_live_sideband_and_new_writer_recovers_once` 在注入滞后 Summary 时调用路径型 `write_summary_sync` 触发 Windows error 32，尚未进入 observer 断言；改用原 adapter bound directory 的 `write_atomic` 注入相同数据，不释放活 writer、不改变 sharing 权限。
- 新增的启动 query、ACK 前取消、admission 失败三项竞态测试均已在 Windows 通过。此处修的是两个夹具，不把它们记作产品 runtime 的第二个故障。
- 原发布重跑 [39197315035](https://github.com/LordCasser/grow/actions/runs/39197315035) 已主动取消，避免在原生 CI 尚未完整通过时公开版本；没有创建 Release。

## 本机验证

- Rust/Rustdoc 1.93.1、两编译任务、16 MiB 测试线程栈，禁用 incremental/dev/test debug。
- `cargo test --locked -p shell --lib coordination -- --test-threads=4`：**58 passed / 0 failed**，包括原来的两个失败测试，日志 `/tmp/grow-windows-fixtures-coordination.log`。
- 隔离 GROW_HOME 执行当前新构建 Shell binary 的 `agent::mvp_agent::tests` filter：**127 passed / 0 failed**，日志 `/tmp/grow-windows-fixtures-mvp-agent.log`，覆盖公共 helper 的其余使用方。
- 现有全部业务断言保留，没有跳过或 ignore Windows 测试；不通过放宽产品绝对路径或文件共享策略使测试通过。
- rustup cargo 的 fmt check、`git diff --check`、Ruby YAML 解析及 push/PR 路径一致检查通过。
- active OpenSpec strict：**17 passed / 0 failed**，日志 `/tmp/grow-windows-fixtures-openspec-active.log`。

## 远端验证与归档

[main push 协调 CI 39200121968](https://github.com/LordCasser/grow/actions/runs/39200121968) 在 commit `ea598d1ecb120d6316a6a5b78ad9a89ce37df11a` 三个 job 全部 success，完整日志 `/tmp/grow-windows-fixtures-native-ci-final.log`。每个平台的真实进程步骤均为 success，逐一核对 10 个 PASS 场景。

| Runner | Shell IPC | Shell coordination | Pager coordination | Minimal coordination | 真实进程场景 |
| --- | ---: | ---: | ---: | ---: | ---: |
| ubuntu-24.04 | 7 | 58 | 45 | 4 | 10 |
| windows-2025 | 13 | 58 | 45 | 4 | 10 |
| macos-15 | 7 | 58 | 45 | 4 | 10 |

表中均为 passed，所有测试组 0 failed；不把这些 filter 数量当作三平台完整 Shell/Pager 包总量。Windows 夹具和生产真实进程路径已在原生 runner 验证，不以本机结果代替。

只读对抗性复核确认：路径值始终用于同一个 helper cwd/LocalFs/ToolContext；Summary 夹具使用现有 writer 能力，保留活 writer 及所有原断言，未改变产品共享权限。本次仅测试和 workflow 路径维护，无行为 delta。

归档在固定 `ea598d1e` 的干净 worktree 完成，独立于根工作区的并行 MCP 修改。归档前 active strict **17/0**、archived **609/0**；归档后 `openspec validate --all --strict --no-interactive` 为 **16 passed / 0 failed**，`openspec validate --archived --no-interactive` 为 **610 passed / 0 failed**。
