## Why

`send_tool_call_start` 的通配分支仍把已注册的 `lsp` 工具显示为 `Tool call`，动态工具也只有泛称。`acp_tool_update` 的通配分支会对 `ContextRecall` 和动态结果返回 `None`，使 Pager 的工具行缺少终态。新增工具变体时两个分支都不会触发编译检查。

## What Changes

- 为 LSP 开始事件显示操作和文件，为动态工具显示实际 wire name。
- 为 ContextRecall 和动态结果发布原 tool-call ID 的终态更新。
- 移除这两个封闭枚举的通配分支，使新增变体必须显式选择展示策略。

## Impact

只更改 Shell 到 ACP/Pager 的展示投影；工具执行、模型结果和授权不变。
