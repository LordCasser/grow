## ADDED Requirements

### Requirement: Plan completion retains an inspectable durable result

Complete SHALL 在原审批执行契约下要求完整 report。原始报告 SHALL 通过已有不可变存储保全，原计划身份和报告引用 SHALL 与退出 Normal 在同一 Control 提交中确认，随后才发布成功并恢复正常采样以生成简短总结。写入或提交失败 SHALL 保留原 Plan 并返回失败，不发布完成状态。完成引用 SHALL 只在下一次成功手动 Behavior 选择时清除，包括同模式选择；自动控制转换与恢复不得清除。fork SHALL 不继承父 session 的完成展示身份。

#### Scenario: Persist completion
- **WHEN** Executing Plan 完成，原始报告写入成功且 Control 提交确认
- **THEN** 当前协议为 Normal，完成引用仍可恢复，后续 Normal 回复提供简短总结，canonical 查询读取原计划及原始报告。

#### Scenario: Completion cannot be committed
- **WHEN** 报告写入或 Control 提交失败
- **THEN** 原 Executing 状态保留，不宣称完成，不发布成功总结或无效引用。

#### Scenario: Manual selection clears the completed result
- **WHEN** 完成后下一次手动 Behavior 请求成功接纳，即使目标仍为 Normal
- **THEN** 清除完成展示引用并持久确认；请求未接纳则保持原引用。

#### Scenario: Restore and fork
- **WHEN** 原 session 恢复，或从包含完成结果的历史创建 fork
- **THEN** 原 session 保留完成展示，fork 保留历史事实但不继承原 session 的完成展示身份。
