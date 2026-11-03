## Why

Session `01a1a0fc-f83a-70e0-aefa-339ca4f054b4` 的五项 required delivery 无法完成验收。模型把 task id、单独 seq 或 null 当成 resultId，反复被拒；todo_write 的模型正文只输出任务摘要，不提供可复制的准确绑定。验收错误早于长文本粘贴。长 followup 的模型投影截断由独立 change `preserve-long-steering-input` 修复。

## What Changes

- todo_write 的未闭合交付摘要及每次采样的规划上下文提供准确的 review 绑定字段和使用说明。
- 错误 resultId（包括 released）在 actor 准入时拒绝，并返回该 link 的准确绑定；不将可纠正参数错误推到持久化因果校验。
- 保留显式验收、结果身份、revision、执行处置及重复阻塞停止规则，不自动接受或清除历史义务。

## Capabilities

### Modified Capabilities
- `task-planning`: 可执行的验收绑定投影与错误回执。

## Impact

修改 Todo 模型摘要、SessionActor 规划投影/命令验证和相关回归；不改 Timeline schema、输入接纳、规划 owner 或用户真实会话数据。
