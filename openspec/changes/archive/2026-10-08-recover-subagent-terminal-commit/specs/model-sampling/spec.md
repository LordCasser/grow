## ADDED Requirements

### Requirement: Local finalization failure preserves admitted attempt settlement

本地子任务收尾失败或 caller 超时 SHALL 保留已经准入的 attempt evidence、用量结算与关闭责任，继续遵循既有 cancellation/shutdown 屏障。此类失败 SHALL NOT 单独触发 provider 重采样、工具重执行或提前关闭 child Timeline。强制关闭期限到期 SHALL 报告关闭未完成，不能声明已成功结算。

#### Scenario: Provider completed before result storage failed
- **WHEN** provider 已正常返回，随后本地结果写入或父回执提交失败
- **THEN** 恢复只核对本地事实与交付，不重新请求模型生成相同结果。

#### Scenario: Cancellation arrives during usage settlement
- **WHEN** 子任务取消时已准入 attempt 的证据或用量 ACK 尚未确认
- **THEN** 系统保持可观察的 settling 状态，不以 caller 超时为由提交 child terminal；既有关闭期限到期则报告关闭失败并保留未确认边界。

#### Scenario: Tool side effects cannot be confirmed
- **WHEN** 工具可能已产生外部副作用，而完成记录或响应不明
- **THEN** 收尾恢复不自动重执行工具，也不将缺失结果当作该工具从未执行。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/shell/src/session/actor/teardown.rs`、`crates/codegen/sampler/src/actor/mod.rs`；既有 sampler/child cancellation 结算契约继续适用。
