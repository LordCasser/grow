## ADDED Requirements

### Requirement: Restored hook registries validate event identity
HookRegistry 反序列化 SHALL 验证每个 Hook 的 event 与所属 map 键一致，并通过既有 HookSpec.validate 约束；任一失败 SHALL 拒绝整个快照，不静默改派事件或丢弃 handler。

#### Scenario: 事件索引矛盾
- **WHEN** map 键与其中 Hook 的 event 不同
- **THEN** 恢复返回明确错误，不产生可执行 registry。

#### Scenario: 恢复非法失败策略
- **WHEN** 非 admission 事件的 Hook 设置 on_failure block
- **THEN** 恢复返回配置错误。

#### Scenario: 合法多事件快照
- **WHEN** 所有 Hook 与所属事件一致且通过已有校验
- **THEN** 恢复保留事件分组与组内顺序。
