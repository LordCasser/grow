## ADDED Requirements

### Requirement: Fresh skill results remain complete until their first consuming step settles
显式技能结果从上下文发布到首次关联模型步骤获得已接纳响应之间，SHALL 作为完整必需内容保留。pre-prune、前后台压缩与请求重建 SHALL 不裁剪、摘要替换或丢弃这些结果；用户来源的所属输入 SHALL 与结果保持关联。该保护 SHALL 基于已有 Surface identity 与请求 / 响应事实，首次消费完成或执行明确终止并结算后解除，不形成永久技能状态。

#### Scenario: Pre-prune selects a fresh oversized result
- **WHEN** 新技能结果超过普通单项裁剪预算，但尚未获得首次关联模型响应
- **THEN** pre-prune 保留该结果全文，只能选择其他可裁剪历史；预算无法满足时明确失败，不带截断结果继续采样。

#### Scenario: Async compaction publication overlaps fresh content
- **WHEN** 后台压缩准备后，新加载内容进入 Surface，发布时选区与尚未首次消费的内容冲突
- **THEN** 在现有 authority / revision 边界重新校验并拒绝冲突发布，不覆盖新输入或技能正文。

#### Scenario: First consuming step completes
- **WHEN** 包含完整加载内容的请求获得已接纳响应
- **THEN** 该加载结果可按普通历史参与后续压缩，历史 Loaded 不承诺正文常驻。

#### Scenario: Execution ends before consumption
- **WHEN** 首次消费前执行明确终止或取消
- **THEN** 先确认撤下用户组或为模型结果提交配对的交付错误，再解除保护；原始历史保留，后续独立请求不重新激活已终止内容。

#### Scenario: Cold interruption closes an unconsumed execution
- **WHEN** 技能组已持久化而首次响应尚未接纳，进程退出且没有有效的挂起 checkpoint
- **THEN** 普通中断恢复在关闭原 Turn 前结算未消费正文，保留原输入和 Hook 身份，不让正文进入下一独立 Turn；有效挂起仍保留原 Turn 的正文和首次消费保护。

证据入口：`crates/common/compaction/src/prune.rs` 的 plan_tool_result_pruning、`crates/codegen/shell/src/session/actor/compaction.rs` 的 maybe_pre_prune_at / 后台发布，以及 `crates/codegen/chat-state/src/actor/mutations.rs` 的 Surface 修改。本条只增加首次消费所需的短期约束。
