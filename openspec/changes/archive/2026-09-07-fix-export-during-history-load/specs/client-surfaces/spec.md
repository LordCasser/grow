## ADDED Requirements

### Requirement: Interactive export waits for history replay completion
交互式完整会话导出 SHALL 在历史回放未完成时拒绝输出，并提示用户稍后重试，避免将部分历史作为完整导出。

#### Scenario: History still loading
- **WHEN** session.loading_replay 为 true 且已有部分可见消息
- **THEN** 导出只反馈加载中，不写文件或剪贴板。

#### Scenario: History loaded
- **WHEN** 历史回放完成并清除 loading_replay
- **THEN** 恢复正常导出，包含已加载的全部可导出消息。
