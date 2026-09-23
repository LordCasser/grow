## Why

跨 Session 通信需要让 agent 交换意见，同时保持询问和正式消息的边界。现有 ask 已使用 Sideband，send 已有 durable receipt，但还没有双向意见回复和专用的接收工具结果。TUI 的投递机制文案与 plain-text 详情也增加了阅读负担。

本变更实现跨 Session 通信的上下文、回执与 TUI 契约。用户已明确：固定 UI 使用英文、沿用 Grow 风格、去掉常驻上下文/投递模式解释；send 回执只确认接收，意见进入双方上下文。运行时代码、工具 schema、请求投影与 Pager 在本 change 内一并落实；验证结果见 verification.md。

## What Changes

- 保持 ask 的独立 Sideband 语义；正式意见使用 send / reply，在接收方安全步骤进入上下文。比较方案与输入表示见 [exchange-design.md](exchange-design.md)。
- 完善发送回执：透传真实 receipt ID，用 typed outcome 区分 received / rejected / unconfirmed；不等待模型回答，不添加自动重发。既有事实、恢复及错误边界见 [receipt-design.md](receipt-design.md)。
- TUI 只显示交互、参与方、状态和正文；使用英文固定文案、既有 bullet/缩进/主题/详情/footer。Markdown 与原文保真见 [tui-design.md](tui-design.md)。
- 第二阶段接入 runtime agent-message context item，将接收消息投影成完整专用工具调用/结果对；沿已有持久消息关系提供反向 reply，不赋予 child 新的人类授权或上行指挥能力。

不新增讨论数据库、消息总线、消费回执、自动协商循环或专用通信页面。不把 peer ask 的读取许可升级为任意 peer send；任意 peer 初始消息接纳与权限扩展单独处理。

## Capabilities

### New Capabilities

无。使用现有协调、Timeline、模型请求和 Pager 能力。

### Modified Capabilities

- `local-coordination`: ask 上下文边界、明确的接收回执、沿已授权消息关系的反向意见回复。
- `client-surfaces`: 简洁英文通信行、Markdown 及原文访问；移除常规 UI 中强制显示投递模式的旧要求。
- `session-timeline`: 第二阶段消息消费与有来源 context item 原子提交，复用 receipt 恢复。
- `model-sampling`: 第二阶段 runtime agent message 的完整工具交换投影与跨 provider/compaction 保真。

## Impact

第一阶段修改 Pager 的正文投影与详情，以及发送 adapter 的 receipt/outcome；第二阶段涉及 notification admission、sampling-types 和 request projection、reply 授权。两个部分分别验证，输入协议变化作为明确行为变更记录。

保持现有 Timeline 所有权、inbox exactly-once 消费和 Sideband 身份。新 context item 是必要的语义表达，不增加独立账本；不维护双发送协议或虚构历史字段。实施时同步 `docs/architecture/local-coordination.md` 并链接规范。

已有 `fix-sideband-consumed-surface-validation` 于 2026-09-22 独立归档；子任务取消/resume 和协作 prompt 各有独立 change，本提案不合并其实施范围。
