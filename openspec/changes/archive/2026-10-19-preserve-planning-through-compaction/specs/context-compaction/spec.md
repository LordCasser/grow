## MODIFIED Requirements

### Requirement: Planning context projects current local obligations

模型上下文 SHALL 从权威投影提供当前局部计划、直接相关变化、有效等待与未验收交付的有界摘要及必要引用，不在每轮复制整棵 Agent 树。完成细节 MAY 压缩；继承 transcript 或旧 summary SHALL NOT 覆盖当前 owner 和计划。压缩自身 SHALL NOT 触发额外模型续跑。Pager 收到 compaction 完成通知 SHALL 保留当前 TODO 投影；只有明确的 Planning/Plan 更新可替换该投影。

#### Scenario: CC-01 Large child tree has a small local context
- **WHEN** root 有多个 child 和大量已完成步骤，当前只等待一个必需结果
- **THEN** 当前上下文保留该等待、相关局部计划和结果引用，其他已完成细节可摘要；不因压缩自动规划或加载整个后代列表。

#### Scenario: CC-02 Inherited history mentions a parent plan
- **WHEN** child 的 fork 历史或压缩摘要含父 TODO，child 已有自己的当前计划
- **THEN** 历史保留为来源，模型得到明确的当前 child 投影；旧文本不成为可写父计划或当前列表。

#### Scenario: Compaction completion preserves the visible local plan
- **WHEN** root 或 child 收到同步或异步 compaction 完成通知，且当前 session 已有 TODO 投影
- **THEN** compaction 只更新自身状态与用量，现有 TODO 保持可见；之后收到明确的 Planning/Plan 更新时仍按该更新刷新列表。
