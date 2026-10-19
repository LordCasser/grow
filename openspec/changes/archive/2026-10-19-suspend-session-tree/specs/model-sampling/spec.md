## ADDED Requirements

### Requirement: Suspension settles the current attempt before saving continuation

快速挂起 SHALL 停止新采样准入并中止活跃 provider 请求，保留可得 evidence 与已知或 incomplete 用量结算。只有结算已确认或在合法恢复所有权下核对完毕，才能确认对应挂起位置；取消或超时 SHALL NOT 代替结算 ACK。挂起中断不代表 child 任务取消，已经接纳的 assistant response SHALL 保持 canonical。未接纳步骤的重新采样 SHALL 继续检查 typed attempt 事实、输出可撤销性、外部副作用安全、结算、当前 owner/预算准入及剩余恢复额度；挂起与 resume SHALL NOT 重置原逻辑采样的总 attempt 上限或绝对期限。

#### Scenario: Sampling is interrupted before response admission
- **WHEN** 活跃流仅有尚未接纳的 candidate 时发生挂起
- **THEN** 保存 attempt 中断与用量证据，不把部分输出变成成功回复；恢复核对全部重新采样条件，仅在满足时于原逻辑步骤发起新 attempt，不重放历史工具副作用；条件不满足时明确报告受阻或沿既有失败路径处理。

#### Scenario: Provider-side effects remain uncertain after suspension
- **WHEN** 未接纳的 provider 请求可能执行有副作用的托管工具，且无法证明重放安全
- **THEN** 即使 Grow 本地工具尚未执行，也不自动重采样；保留原 attempt 的未知副作用事实，等待核对或明确处置。

#### Scenario: Retry allowance expires while suspended
- **WHEN** 恢复时原逻辑采样总 attempt 额度已耗尽或绝对期限已到
- **THEN** 不以新运行实例或重新提交为由重置额度/期限，不再派发该步骤的自动重试，并沿既有额度或期限失败路径处理。

#### Scenario: Response admission already committed
- **WHEN** response 已接纳但其工具尚未全部执行时发生挂起
- **THEN** 恢复复用该 canonical response 和调用身份，先核对或补齐既有展示提交，再续接剩余工具，不重新请求同一已接纳回复。

#### Scenario: Attempt settlement is blocked
- **WHEN** evidence 或用量确认超过挂起等待期限
- **THEN** 挂起返回未确认，保存已知身份，不以丢弃 request task、零用量或重复结算制造成功保存边界。

目标入口：`crates/codegen/sampler/src/actor/mod.rs`、`crates/codegen/shell/src/session/actor/teardown.rs::shutdown_sampler` 和 response admission 路径。
