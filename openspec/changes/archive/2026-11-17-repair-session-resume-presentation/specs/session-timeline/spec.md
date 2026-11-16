## ADDED Requirements

### Requirement: Suspended logical turns restore their accounting attribution

原逻辑 Turn 的已确认用量 SHALL 跨会话挂起与冷恢复保留。恢复同一 Turn 时，其结果用量 SHALL 包含挂起前后按原归属计入的主 attempt、辅助 attempt 及子 Agent 结算，且每份事实至多计入一次。Session-only 迟到消费 SHALL 保持原归属，不得因恢复时存在活动 prompt 而改记到该 Turn。用量恢复 SHALL 复用已验证执行投影与后缀结算，不以加载展示历史为前提。

#### Scenario: One logical turn spans two incarnations
- **WHEN** 同一 Turn 挂起前已确认 100 token，恢复后再确认 50 token，然后完成
- **THEN** 该 Turn 的结果为 150 token，lifetime 只累计这两份事实一次；两个 incarnation segment 保留各自结算边界。

#### Scenario: Restore finishes without another model request
- **WHEN** 同一 Turn 挂起前已有主模型、辅助或子 Agent 用量，恢复后直接完成而无新模型调用
- **THEN** 最终结果仍保留该 Turn 原已确认用量，不返回空账单或精确零。

#### Scenario: A child bill has only session ownership
- **WHEN** 子 Agent 账单的持久归属明确为 session-only
- **THEN** 恢复后它仍只进入原 session 账本，不根据最近 prompt 推断新的 Turn 归属。

#### Scenario: Rewind reuses a prompt position or client prompt id
- **WHEN** rewind 后新 Turn 复用旧 prompt_index 或客户端 prompt_id，旧 attempt 或 child 迟到结算
- **THEN** 按原稳定 Turn 身份保留归属，旧消费不进入新 Turn；相同结算键改换 Turn owner 视为冲突，不能覆盖原事实。

#### Scenario: A historical attribution cannot be proven
- **WHEN** 既有记录不足以证明某份已知 lifetime 消费属于恢复中的 Turn
- **THEN** 保留 lifetime 事实，Turn 结果明确不完整，不伪造归属、丢弃已知消费或从正文估计精确数。

证据入口：`crates/codegen/chat-state/src/actor/{state,mutations}.rs`、`session_runtime.rs`、working publication；`crates/codegen/shell/src/session/actor/turn/{admission,mod}.rs`。

### Requirement: Completed turn usage remains an observational result

逻辑 Turn 的完成用量 SHALL 随既有完成结果持久留存并可按原身份有界查询。它 SHALL 保留冻结时的完整性、各模型用量和缓存分类，不因后续展示页加载、resume 或 session-only 后台结算而改写。读取完成结果 SHALL NOT 重投 completion、结束 live Turn 或再次累计 token。

#### Scenario: A completed result is outside the initial page
- **WHEN** 历史完成结果不在最近页中，随后通过滚动或时间线加载到该结果
- **THEN** 显示原 Turn 的相同用量与完整性，无需先读取全部历史或重新运行计量。

#### Scenario: Background usage settles after result freeze
- **WHEN** 已冻结完成结果之后出现原本 session-only 的后台用量
- **THEN** lifetime 按原结算累计，已冻结 Turn 结果保持原值，不伪称此前已包含该消费。

证据入口：`crates/codegen/shell/src/session/history/grow_facts.rs::GrowHistoryFact::TurnCompleted`、`session/storage/jsonl/` 的完成历史索引与原 usage 结算路径。
