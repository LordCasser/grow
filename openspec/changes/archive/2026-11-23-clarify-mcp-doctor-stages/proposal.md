## Why

HTTP doctor 目前在本地创建客户端后就报告 `server started`，握手超时又丢失了阶段名称。握手后的 `tools/list` 缺少覆盖所有页的期限，服务挂起时 CLI 可能无法输出结果或退出。

## What Changes

- 文本和 JSON 均显示实际完成或失败的阶段，并保留协议版本、目录数量和超时信息。
- Doctor 工具发现沿用会话初始化的总预算。用真实 CLI 进程验证成功和失败的退出码。

## Impact

涉及 Shell 的 MCP doctor、会话初始化预算的共享计算，以及 CLI 用户指南和开发者说明。
