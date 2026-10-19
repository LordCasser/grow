## ADDED Requirements

### Requirement: Live source inquiry startup and queries share an admission boundary

source SHALL 使首次 durable startup admission、runtime 运行记录登记与同源 inquiry 查询一致可观察。查询跨越启动提交时 SHALL 等待接纳结果或返回真实的当前运行状态，不因暂未登记 runtime 记录而把活启动误判为重启遗留。目标派发 SHALL 发生在 source durable ACK 后；接纳失败 SHALL 不派发、不写入虚构完成事实。查询 SHALL 不触发执行或自动重发。

#### Scenario: Query overlaps a durable startup acknowledgement
- **WHEN** source 的 `OutgoingStarted` 已落盘，接纳 ACK 仍被等待，并发查询同一 inquiry
- **THEN** 查询返回当前启动状态，不返回冷恢复专用的 `SourceUnavailable`，目标在 ACK 前没有收到询问。

#### Scenario: Startup admission fails or caller cancels
- **WHEN** source 接纳返回失败，或调用方在接纳期间取消
- **THEN** 失败不派发且释放接纳边界；取消成功接纳后的询问沿原取消身份终结，不派发到目标，不将查询视为重试。

#### Scenario: Cold source has only a historical start
- **WHEN** 新 runtime 只有重启前的 `OutgoingStarted` 且没有终态或当前运行记录
- **THEN** 查询继续返回 `SourceUnavailable`，不重跑；原 payload 的显式重试与冲突拒绝仍遵守 durable 身份约束。

证据与实现入口：`crates/codegen/shell/src/coordination/runtime.rs` 的 `ask_session_with_admission` / `get_inquiry`，`crates/codegen/shell/src/agent/mvp_agent/coordination.rs` 的 `ask_with_id`；回归见 runtime identity tests 和 `scripts/test_local_coordination.py`。
