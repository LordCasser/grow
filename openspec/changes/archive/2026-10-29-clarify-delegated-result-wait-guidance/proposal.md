## Why

主 Agent 后台派发子任务、完成独立工作后，有时另起 sleep 命令或空任务等待。用户截图中，一个仍运行的子 Agent 之外又出现两个等待任务。现有任务输出工具已经支持事件驱动的阻塞等待，但 primary 指引仅泛指等待机制，后台启动回执也没有完整正超时调用示例。

## What Changes

- primary 和 spawn 指引明确：独立工作耗尽且需要既有 child 结果时，等待原 task ID，不另起 sleep、轮询循环或空任务。
- 后台启动回执给出当前工具名、实际参数名和当前等待上限内的 JSON 示例；恢复后台回执沿用同一生成路径。
- 说明有界等待返回、失败和验收的边界，保持多个 ID 的 wait-all 语义。
- 更新 subagent 用户指南，复用现有 View 等待呈现，不新增 Task 行。

## Capabilities

### Modified Capabilities

- `local-coordination`：委派结果等待的模型可见指引与回执。

## Impact

影响 primary audience、共享 task 文案和回执、task 调用方与恢复回执调用方。不修改工具输入、等待调度、Timeline、View 状态或配置；不新增等待工具、sleep 检测器或无限等待 API。已有结构化 `PlanWait` 不变。
