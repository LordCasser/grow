## ADDED Requirements

### Requirement: Concurrent Goal attempts use settled usage for admission

有 token_budget 的 Active Goal SHALL 根据已持久确认的累计用量决定是否接纳新的 provider attempt。累计值达到阈值后 SHALL 立即关闭新准入；此前已准入的并行 attempt SHALL 继续结算并完整计入累计值，即使最终总量超过阈值。权限 Sideband 与前台的既有独立并行 SHALL 保留；未知用量仍按既有精确预算规则关闭准入。

#### Scenario: Concurrent attempts cross the threshold together
- **WHEN** 前台和权限后台 attempt 均在累计值尚未达到预算时准入，两者已确认用量之和随后超过预算
- **THEN** 两次用量均准确且仅一次计入；阈值后的新准入被拒绝，既有 attempt 的结果不因后一次结算而被丢弃。

#### Scenario: First settlement exhausts budget while another attempt is active
- **WHEN** 一个已准入 attempt 的已确认用量先达到预算，另一个 attempt 仍在运行
- **THEN** 新请求不能准入，仍在运行的 attempt 可完成并按实际用量结算。
