# 验证记录

环境为 macOS arm64、Rust 1.93.1。使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=33554432 cargo test --locked -p shell -p pager -p chat-state --lib --no-run` 构建，再对生成的测试二进制按模块或唯一名称运行，未使用 0-test 成功代替验证。

## 故障回归

- `tool_runtime_failures_preserve_original_error_and_stop_recovery`：1 passed，内部逐项注入 ToolStarted、ToolResult publication、ToolCompleted ACK 的永久 StorageFull 故障。每项检查原错误、tool-execution 分类、仅一次 provider 请求、有效冷恢复前缀、无伪造 ToolCompleted/TurnEnded。completion requirement 配置允许 3 次重试，实际不得启动 recovery。
- `storage_full_failure_poison_closes_the_actor_without_retrying_forever`：1 passed，并验证 actor 关闭后 pending identity 保留原始 storage message。
- 编写测试过程中修正输入 admission、preview barrier、read_file 的 target_file 参数及 workspace binding；暂时性 I/O 错误会按既有规则重试，改用实际永久 StorageFull 验证 fail-stop。未改重试策略。
- 实跑发现 buffered result 失败后原始存储错误先于 pipeline Err 丢失，补存 pending diagnostic 后三故障通过。最终还增加 terminal fault 必须来自实际 dispatch 的断言，收尾运行记录补在末尾。

## 相邻生命周期回归

- `turn_pipeline_v2_tests`：15 passed，包含 cancel/panic 收尾及 Goal/control ownership。
- `event_tracker::tests`：10 passed，包含工具 ACK 屏障、terminal exact retry、取消与并发终态。
- `skill_loading_tests`：14 passed，包含实际模型工具路径、拒绝、提交 ACK 与结果生命周期。
- `sampling_failure_recovery_tests`：1 passed，正常已知采样失败继续沿原有恢复和额度逻辑。
- Plan projection/control/retained gate：21 passed；手动及 Goal 控制保持既有行为。

没有运行全工作区测试或 hosted Windows CI；跨平台 CI 触发与发布依赖由 Python 契约检查验证。

## 最终收尾运行

新增 actual-dispatch 断言后再次构建并运行故障回归：1 passed（内部三个故障全部通过）；`finalization_failure::tests` 3 passed，现有 certainty 与 I/O kind 判定不变。Pager ACP 模块最终重跑 352 passed / 0 failed / 1 ignored。`cargo fmt --all -- --check`、`git diff --check` 通过；Python 全部 18 passed。

## OpenSpec 归档核对

2026-10-19 已通过 `openspec archive <change> --yes` 归档；全部 tasks 完成。归档前全量严格校验 23/23；归档后 `openspec validate --all --strict --no-interactive` 16/16，`openspec validate --archived --strict --no-interactive` 634/634。F2 先于 F3 归档，核对主规范同时保留 cursor merge 与 child lifecycle/实际历史顺序场景。开发说明和 backlog 已同步，无 MCP 实现修改。
