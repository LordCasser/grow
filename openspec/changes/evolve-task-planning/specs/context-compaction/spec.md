## ADDED Requirements

### Requirement: Planning context projects current local obligations

模型上下文 SHALL 从权威投影提供当前局部计划、直接相关变化、有效等待与未验收交付的有界摘要及必要引用，不在每轮复制整棵 Agent 树。完成细节 MAY 压缩；继承 transcript 或旧 summary SHALL NOT 覆盖当前 owner 和计划。压缩自身 SHALL NOT 触发额外模型续跑。

#### Scenario: CC-01 Large child tree has a small local context
- **WHEN** root 有多个 child 和大量已完成步骤，当前只等待一个必需结果
- **THEN** 当前上下文保留该等待、相关局部计划和结果引用，其他已完成细节可摘要；不因压缩自动规划或加载整个后代列表。

#### Scenario: CC-02 Inherited history mentions a parent plan
- **WHEN** child 的 fork 历史或压缩摘要含父 TODO，child 已有自己的当前计划
- **THEN** 历史保留为来源，模型得到明确的当前 child 投影；旧文本不成为可写父计划或当前列表。

证据与实施入口：`crates/codegen/shell/src/session/actor/compaction.rs`、`crates/codegen/shell/src/session/helpers/compaction_context.rs`。

### Requirement: Compaction cannot acknowledge or erase pending replanning

压缩发布及下一安全执行边界 SHALL 重新核对当前计划和未闭合责任。并发到达的重规划、Consumed 但未 Handled 的变化及新结果 SHALL 保留；压缩摘要 SHALL NOT 冒充处理确认或重新消费旧消息。

#### Scenario: CC-03 Parent update arrives during compaction
- **WHEN** 压缩开始后父项变化或 child 结果到达
- **THEN** 发布后的当前投影仍含新责任，旧摘要不能覆盖它，也不能将其标为已处理。

#### Scenario: CC-04 Consumed update survives a compacted history
- **WHEN** 已消费的重规划消息进入被压缩范围，但 child 尚未提交处理确认
- **THEN** 未处理引用和所需要求仍可获得；同一历史消息不重复注入，处理确认只能由合法 child 计划命令产生。

证据与实施入口：`crates/codegen/shell/src/session/actor/compaction.rs` 的 boundary publication、`crates/codegen/chat-state/src/timeline.rs` 的通知与 context identity。
