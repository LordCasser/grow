## Why

图片投影把内部技术句写成 assistant 正文，导致主模型复述并在 TUI 显示。真实会话 01a15403-e361-7992-beff-a0f5db47ed4a 的主模型请求含 12 条此类历史，原始 SSE 随后返回同句；该次图片描述实际已成功。用户明确要求这些内部状态不要进入界面。

## What Changes

- 图片工具调用与关联 carrier 的投影保留原有源信息清理和配对边界，但不再制造技术性 assistant 话语。
- 两种完整内部占位正文从模型请求的派生历史及子 Agent 背景上下文中排除，不改原始 Timeline、provider evidence 或 canonical response replay 记录。
- normal/minimal 与子 Agent 共用的 assistant 文本块隐藏完整占位消息，流式前缀暂存到可判别；正常解释、用户输入、工具输出与 Converted 操作行保留。
- 验证三个协议的 wire body、实时/冷回放投影及 UI 分块输入，避免仅用字符串隐藏掩盖模型历史污染。

## Impact

涉及 sampling-types 的图片 shadow/request segment、Shell 子上下文归一化、Pager AgentMessageBlock 及相关测试。复用既有 ImageProjection 与文本块，不增加存储格式、运行时服务或 MCP 改动。
