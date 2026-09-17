## ADDED Requirements

### Requirement: Sampler shutdown preserves admitted attempt settlement

Sampler 正常关闭 SHALL 先停止新请求准入并向活跃 provider 工作发送取消，再等待所有已准入 request task 完成可用 attempt evidence 和所有适用用量账本的确认结算。正常关闭 SHALL NOT 通过 abort request task 跳过这些独立结算。只有显式有界关闭期限到期后才能强制终止，且该结果 MUST 报告为关闭失败而非成功的 graceful frontier。

#### Scenario: Shutdown while provider work is active

- **WHEN** sampler 正常关闭时已准入 attempt 仍在 provider polling 或取消收尾
- **THEN** sampler 阻止新准入、取消 provider 工作并等待该 attempt 的 evidence 与适用用量结算完成后才报告正常关闭成功。

#### Scenario: Shutdown while settlement acknowledgment is blocked

- **WHEN** provider 工作已经结束或取消，但 evidence 或任一适用用量账本的确认仍在等待
- **THEN** 正常关闭继续等待原 attempt 的确认，不 abort request task、不重复结算，也不把缺失确认当成已完成。

#### Scenario: Forced shutdown deadline expires

- **WHEN** 已准入 request task 在 owner 的明确关闭期限内仍不能完成
- **THEN** owner 强制终止剩余工作并返回关闭失败；调用方不能把该结果当作成功跨越的最终持久化边界。

证据入口：`crates/codegen/sampler/src/actor/mod.rs` 的 actor shutdown 与 `SamplerOwner::shutdown_bounded`，`crates/codegen/shell/src/session/actor/teardown.rs::shutdown_sampler`。
