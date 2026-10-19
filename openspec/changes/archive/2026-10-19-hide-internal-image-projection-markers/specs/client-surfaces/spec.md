## ADDED Requirements

### Requirement: TUI omits complete internal image projection placeholders
normal、minimal 及子 Agent TUI SHALL 在共用 assistant 文本入口隐藏整条消息精确等于两种内部图片投影占位句的正文。流式到达的可能前缀 SHALL 暂存至可以判别；前缀分叉或完成时仅是未完成片段 SHALL 原样恢复。隐藏 SHALL 同时适用于 live、deferred/replay、raw 显示和复制的派生正文，不改 canonical replay 的 digest/updates。图片恢复的英文操作行 SHALL 保持原有展示。

#### Scenario: Complete marker arrives in several chunks
- **WHEN** 两种完整占位消息之一经任意 chunk 边界或 deferred 路径到达
- **THEN** 界面不先闪现技术句，完成后显示与复制正文为空，minimal 不提交该句到终端历史。

#### Scenario: Message turns out to be ordinary text
- **WHEN** 已暂存前缀随后分叉、标记后接解释（包括块 finish 后的晚到追加），或结束时只是未完整前缀
- **THEN** 原文完整恢复，不吞字符，不影响 Markdown、用户或工具输出。

#### Scenario: Actual image conversion succeeds
- **WHEN** 图片描述完成
- **THEN** 既有 `Converted 1 image` 等操作行继续显示，内部图片投影状态不增加一条 assistant 对话。
