## ADDED Requirements

### Requirement: Control context identity survives activation and replay
Timeline SHALL 为一次控制事件中的每条模型上下文保留独立且稳定的身份，立即投影、延迟激活与恢复重放一致。

#### Scenario: 空会话恢复时重建上下文
- **WHEN** 会话包含一次控制事件的多条上下文，恢复需要补入项目指令
- **THEN** 完整上下文替换可持久化，保留的事实可再次重放，不能因重复身份报 shadow set 不完整。

#### Scenario: 延迟激活只保留部分上下文
- **WHEN** 同一事件不同层的上下文在 step 或 turn 边界激活，或同层被后续控制取代
- **THEN** 每个激活项保留原事件内身份，模型 Surface 与 branch 投影保持一致。

证据范围：`crates/codegen/chat-state/src/timeline.rs`。
