## ADDED Requirements

### Requirement: Sampling recovery uses typed attempt facts

采样 SHALL 区分缺少完成证据的流中断、确定性协议违例、完整但无效的生成、远端传输/服务失败和本地生命周期失败，并保留实际结束方式。重新采样资格 SHALL 同时依赖错误事实、输出交付状态、未接纳状态、外部副作用安全性、结算确认、所有者准入及恢复额度；内部决策 SHALL NOT 通过序列化诊断文本重建这些事实。

#### Scenario: Provider stream ends before completion evidence
- **WHEN** Chat 缺少 choice finish_reason、Responses 缺少 terminal response 或 Messages 缺少完整结束序列就到达 EOF，且此前不存在确定性协议违例
- **THEN** 候选被拒收并归类为不完整流，在输出可丢弃、结算确认、无不明外部副作用且准入/额度允许时重试原有效输入。

#### Scenario: Idle timeout before completion
- **WHEN** 未完成响应发生 idle timeout
- **THEN** 与不完整流共用上述恢复判定，超时不会把半截工具调用变为可执行结果。

#### Scenario: Protocol conflict precedes EOF
- **WHEN** 流已出现 response/tool 身份冲突、非法索引或矛盾状态后结束
- **THEN** 保留确定性协议违例并停止，不因尾部 EOF 降格为可自动恢复的流中断。

#### Scenario: Local stream producer disappears
- **WHEN** Grow 内部事件生产者结束且没有约定的终态
- **THEN** 报告本地生命周期失败，不能作为远端 EOF 自动重发。

#### Scenario: Missing optional usage after valid completion
- **WHEN** 已取得合法完成证据且候选有效，但有界尾部等待未取得完整 usage
- **THEN** 不因缺少 usage 重新生成正文，按未知用量结算规则和响应接纳规则处理。

#### Scenario: Legitimate semantic terminal result
- **WHEN** provider 明确返回合法 length、content filter、pause 或上下文窗口终止结果
- **THEN** 保留原始语义与完整性校验结果，由 session 执行既有继续、压缩或停止逻辑，不作为缺失终态原地重采样。

#### Scenario: Uncertain provider-side effects
- **WHEN** 请求可能执行有副作用的 provider 托管工具且无法证明重放安全
- **THEN** 自动重采样被拒绝，即使 Grow 本地工具尚未执行且输出可撤销。

### Requirement: Every provider attempt settles all applicable usage before readmission

主/子 agent 模型步骤的每次真实 provider attempt SHALL 具有独立于 Goal 存在与否的归属，并向所有适用的现有消费账本结算，包括最终未被接纳的候选。结算 SHALL 按归属幂等并等待确认；成功响应接纳 SHALL NOT 再累计已结算的消费。未知用量 SHALL 保留未知状态，精确预算下关闭准入。重试 SHALL 重新检查剩余子任务输出额度。

#### Scenario: Rejected attempt followed by success
- **WHEN** 第一次调用产生已知消费但候选被废弃，第二次调用成功
- **THEN** prompt/session/model、Goal 和子任务输出账本中所有适用账本包含两次消费各一次，上下文锚点仅由被接纳响应更新。

#### Scenario: No active Goal
- **WHEN** 没有 Goal 的请求在内部恢复前产生消费
- **THEN** 普通账本和子任务预算仍按 attempt 结算，不因 Goal scope 为空而跳过。

#### Scenario: Partial or lost settlement acknowledgment
- **WHEN** 部分账本已写入但其他结算失败或确认丢失
- **THEN** 禁止下一次 provider 准入，仅核对或幂等补交原 attempt 的结算，不能重复计费或为恢复账本重新推理。

#### Scenario: Unknown spend with an exact budget
- **WHEN** 已开始的 attempt 无法取得完整用量且受精确 Goal 或子任务预算约束
- **THEN** 记录用量不完整并关闭后续准入，不把未知消费当零。

#### Scenario: Unknown spend without an exact budget
- **WHEN** 用量不完整但没有精确预算约束且其他恢复条件均满足
- **THEN** 确认记录未知状态后可继续有界恢复，已知下界不冒充完整用量。

#### Scenario: Remaining output grant shrinks
- **WHEN** 失败 attempt 的已知输出消费减少子任务剩余额度
- **THEN** 下一 attempt 的输出上限不超过新余额，余额耗尽则不派发。

### Requirement: Automatic recovery shares a logical sampling budget

同一个未接纳模型步骤的自动恢复 SHALL 共享总 provider attempt 上限和绝对期限，包含 sampler 重试、doom 重采样及 session 修复后的重提交。分类上限只能收紧总上限。取消、owner epoch 失效、服务端 veto 和预算关闭 SHALL 阻止下一次派发；backoff SHALL 可取消。语义修复 SHALL 由 session 持有，sampler 不修改会话历史。

#### Scenario: Different recovery causes alternate
- **WHEN** 同一步骤依次发生传输失败、非法生成和 native continuation 修复
- **THEN** 每次真实调用都消耗同一总额度和期限，不因切换分支、更新配置或重提交重置。

#### Scenario: Recovery disabled
- **WHEN** 请求显式禁用自动恢复
- **THEN** 仅执行初次准入的调用，任何分类特例都不能额外派发。

#### Scenario: Cancellation during discard or backoff
- **WHEN** 旧 attempt 被关闭之后、下一次 provider poll 之前发生取消或 owner 失效
- **THEN** 不再调用 provider，已有 attempt 的独立结算仍完成或保留待确认状态。

#### Scenario: Request state changes for recovery
- **WHEN** session 确认需要刷新凭据、清空 native continuation、转换图片输入或压缩上下文
- **THEN** session 确认所需状态变化后按新修订提交，并沿用当前逻辑采样剩余恢复额度；半截候选不加入输入。

#### Scenario: Logical deadline expires
- **WHEN** backoff 或 provider polling 到达调用者更早期限或逻辑采样绝对期限
- **THEN** 停止后续模型活动和准入，不重新起表，也不硬中断独立用量结算。

## MODIFIED Requirements

### Requirement: Malformed completed tool arguments recover without execution

Chat Completions、Responses 和 Messages 中结构及身份有效且完整结束的响应若含非法工具 JSON，sampling SHALL 拒收整个候选，并依照统一 attempt 恢复条件重采样最后有效输入。无效调用及有效 sibling SHALL NOT 成为可执行工具或被接纳的 native continuation。该恢复 SHALL NOT 使通用 serialization、明确的 incomplete status 或身份冲突自动可重试；也 SHALL NOT 绕过不可撤销输出、结算或预算保护。

#### Scenario: Bad arguments followed by valid output
- **WHEN** 第一次完整候选包含非法工具 JSON，输出可废弃且统一恢复条件满足，下一 attempt 有效
- **THEN** 发出 Retrying 诊断并有序废弃旧 attempt，仅接纳有效候选，TUI 保持运行活动，不通过终端失败暂停 Goal。

#### Scenario: Repeated invalid generation
- **WHEN** 非法工具 JSON 持续出现
- **THEN** 按三次总 attempt、配置分类上限及逻辑采样剩余总额度中的更严格限制停止；显式禁用自动恢复时不重采样。

#### Scenario: Accounting and evidence before retry
- **WHEN** 被拒收 attempt 具有已知或未知消费
- **THEN** 既有证据和所有适用账本结算先确认，失败或 Goal 准入关闭阻止下一 attempt。

#### Scenario: Cancellation during recovery
- **WHEN** 下一 attempt 前发生请求取消
- **THEN** 不再发起 provider 请求。
