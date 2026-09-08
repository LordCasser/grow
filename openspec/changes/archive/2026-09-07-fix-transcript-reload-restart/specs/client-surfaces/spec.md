## ADDED Requirements

### Requirement: Incremental transcripts restart after owner reload
Minimal 在途 transcript SHALL 在原 agent 重连时使旧分帧结果失效，等待 reload 结束后从最终正文重新构建，不输出暂存期间跳过条目形成的前缀。

#### Scenario: Reload spans a render frame
- **WHEN** 原 agent 正在 reload 且 transcript 尚未生成文件
- **THEN** 保留请求但不消费临时 staging 条目；期间新请求同样等待。

#### Scenario: Reload resolves between frames
- **WHEN** 完整 replay、cursor 成功或失败回滚在下次 pump 前完成
- **THEN** 旧前缀和 ID 失效，从最终正文的全部当前条目重新开始。

#### Scenario: Other view or agent changes
- **WHEN** 用户切换标签或其他 agent reload
- **THEN** 不改变原构建 owner，未 reload 的原构建继续推进。
