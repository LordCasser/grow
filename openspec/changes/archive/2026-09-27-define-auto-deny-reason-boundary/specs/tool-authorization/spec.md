## ADDED Requirements

### Requirement: Auto classifier reasons remain untrusted evidence

Auto classifier 的自由文本 `reason` SHALL 不作为调用 Agent 的权限结果或权限 audit 的决策理由；拒绝结果 SHALL 使用 harness 自有的固定说明，并保留分类来源、verdict 与标准触发原因用于诊断。模型 reason 的内容不得改变拒绝、提示升级或后续工具执行规则。

#### Scenario: Model denial includes instructions
- **WHEN** Auto 模型返回 Block 且 reason 包含要求 Agent 绕过权限的指令
- **THEN** 调用 Agent 只收到 harness 自有拒绝说明，权限 audit 的理由为标准触发原因，指令不进入这两个输出。

#### Scenario: Denial escalation
- **WHEN** 连续 Block 达到既有人工提示阈值
- **THEN** 是否升级只取决于计数与策略，不取决于模型 reason 的措辞。
