## ADDED Requirements

### Requirement: Goal status is queried on demand
Goal 行为提示、续跑上下文与查询工具说明 SHALL 明确优先使用已提供的目标和预算，只有上下文缺失或需要最新状态时查询，不要求例行轮询。业务完成审计 SHALL 保留原有证据要求，不能由读取 Goal 状态代替。

#### Scenario: Continuation has Goal context
- **WHEN** 续跑已提供目标和预算
- **THEN** 指令要求直接进行业务完成审计或推进工作，不为重复确认目标而调用 get_goal。

#### Scenario: Fresh budget needed
- **WHEN** 需要上下文中不存在的最新状态或预算
- **THEN** get_goal 仍可用并返回当前读模型，不自动暂停或完成 Goal。
