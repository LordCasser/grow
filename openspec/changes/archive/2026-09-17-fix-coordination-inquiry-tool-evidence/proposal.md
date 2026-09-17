## Why

父 Agent 在截图中追问子 Agent 已执行的 `ask_parent`，子 Agent 的 Sideband 回答却否认调用。源码核对发现 `handle_coordination_inquiry` 无条件连续删除尾部 Assistant(tool_calls) 与 ToolResult，会抹掉已完成的询问及其他工具证据。现有 local-coordination 约定冻结上下文与隔离回答，尚未明确冻结输入中的工具证据保留。

## What Changes

- 协调询问保留冻结边界内已配对的工具调用、参数、结果、附件与 Assistant 正文。
- 未返回的调用不输出悬空协议，不伪造结果；同批完成的调用继续保留。
- 用真实 actor 和 loopback provider 回归核对 wire 输入、主 Surface 隔离和无工具边界，更新开发者说明。
- 核对截图对应本地会话事实，区分显式询问与自动 PermissionJudgment；不改变权限策略、主对话通知或协调 UI。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `local-coordination`: 协调询问的冻结输入保留已经提交的工具证据。

## Impact

修改 `shell/src/session/actor/coordination.rs` 及其测试，复用既有 `sampling_types::project_portable_history`。不增加依赖、数据实体、持久化字段或授权渠道。工作树中其他 change 的 compaction、recap 和 sampling-types 改动不属于本次范围。
