## ADDED Requirements

### Requirement: Transcript viewing rejects incomplete initial history
首次会话历史加载未完成时，transcript SHALL 提示稍后重试，不将当前部分正文作为完整会话打开。

#### Scenario: Initial loading in any screen mode
- **WHEN** loading_replay 为 true 且不是 minimal 重连等待窗口
- **THEN** 不创建分页器文件或新分帧构建，显示加载提示。

#### Scenario: Initial loading completes
- **WHEN** 历史加载完成后再次请求 transcript
- **THEN** 按当前完整正文正常创建 Markdown 文件或 minimal 条目快照。

#### Scenario: Minimal reconnect remains pending
- **WHEN** minimal 原 agent 有活动 SessionReload
- **THEN** 保留既有等待和最终正文自动重建，不因首次加载保护丢弃请求。
