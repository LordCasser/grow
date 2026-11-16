## ADDED Requirements

### Requirement: Plan uses ordinary tool authorization

Plan 各阶段 SHALL 与 Normal 共用同一工具授权链。仅因 Plan phase 为 Drafting、AwaitingApproval 或 Amending，系统 SHALL NOT 额外禁用终端、编辑、MCP 或子 Agent 能力。工具目录、显式 deny、权限审批、子 Agent 上限、Sandbox 和路径保护 SHALL 继续生效；Plan 审批和 Goal/Workflow 互斥 SHALL 继续由原会话协议所有者判断，不能被普通授权绕过。

#### Scenario: Terminal research under matching permissions
- **WHEN** 相同会话配置、工具和调用参数在 Normal 可进入普通权限判定，Plan 处于规划阶段
- **THEN** Plan 进入相同权限判定，不因阶段在此前返回 Plan 专属终端禁用。

#### Scenario: Ordinary policy denies a planning call
- **WHEN** Plan 调研调用被显式策略、能力上限或 Sandbox 拒绝
- **THEN** 原拒绝保持有效，Plan 不能据“调研需要”跳过普通授权。

#### Scenario: Write and delegated capability use the same gate
- **WHEN** Plan 规划阶段调用编辑、MCP 或请求具有执行能力的调研子 Agent
- **THEN** 其工具能力按普通策略判定，而非依据 Plan phase 降级或拒绝；Plan 提示词仍约束调用用途，不自动批准实施。

证据入口：`crates/codegen/shell/src/session/actor/tool/{authorization,preparation}.rs` 与 `tool/mod.rs` 中的普通授权和 Plan admission 回归；`crates/codegen/shell/src/session/behavior.rs` 的阶段提示与协议判断。
