# 验证

- 新增 `dashboard_stop_key_*` 三项测试在旧实现全部失败，结果均为空 effects；从真实 AppView Ctrl+X 到 dispatch 覆盖 child、child+Goal、Goal-only 与 Workflow-only。
- 修复后 Dashboard stop 定向 15 passed；最终 `cargo test --locked -p pager --lib dashboard -- --test-threads=1`：673 passed、2 ignored，含不可管理/终态 Workflow 不被误停止、peer/root draft 保留、未确认时不能删除。
- `cargo test --locked -p shell --lib cancel_forwards_pause_goal_meta -- --test-threads=1`：1 passed。
- 使用同次 Shell test binary、CI 同样的 `RUST_MIN_STACK=16777216`，执行 GoalPause 真实 actor command、Goal parser、Workflow management parser、run-owned cancellation 和 selector match 测试：7 passed。首次直接运行 giant actor fixture 未设置 CI stack 时发生 stack overflow；按已有 CI 配置重跑通过，未修改或放宽测试。
- Workflow host control 的 run-id selector 已核对生产 parser、manage_workflow_run 与 manager.cancel；本次没有新增后端协议。未声称完成线上模型或真实多进程完整停止旅程，后续统一隔离 PTY 验证。
- rustfmt、git diff --check 通过；归档前全量 strict 17 passed、0 failed；归档后全量 strict 16 passed、0 failed，archive strict 768 passed、0 failed。
