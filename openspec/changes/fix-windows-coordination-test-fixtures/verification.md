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

本机结果不代替 Windows 原生运行。提交后将由相关 main push 的协调 CI 验证三平台库测试与真实进程脚本，完成后再记录结果并归档。
