## ADDED Requirements

### Requirement: Auto model permission judgments obey one end-to-end deadline

主会话和子 Agent 的 Auto 模型权限裁决 SHALL 从请求提交给主会话分类通道开始，共享一个有界总期限；该期限 SHALL 覆盖通道排队、准备、模型尝试与结果结算。首次尝试 SHALL 获得多于一半的剩余预算，允许恢复的第二次尝试 SHALL 不延长总期限。超过期限或请求方已离开后 SHALL 不采纳允许结果。

证据目标：`crates/codegen/workspace/src/permission/auto_mode.rs` — `LlmPermissionClassifier::classify`；`crates/codegen/shell/src/session/actor/turn/sampling.rs` — `wire_permission_auto_llm_classifier`。

#### Scenario: 较慢的首次结果仍在总期限内
- **WHEN** 首次模型尝试超过旧的平均半额但在当前总期限内返回有效裁决
- **THEN** 裁决被使用，不因保留完整第二次尝试预算而提前取消。

#### Scenario: 排队或准备耗尽期限
- **WHEN** 分类请求在通道排队或 Sideband 准备期间耗尽总期限
- **THEN** 该精确调用不获得允许；迟到响应不能改变结果或写入许可。

#### Scenario: 子 Agent 与主会话超时后续
- **WHEN** 子 Agent 的 Auto 模型裁决超时
- **THEN** 当前工具调用失败且不打开人工提示，子 Agent 可继续其他调用；主会话自己的 Auto 分类超时仍进入既有的人工提示后续路径。
