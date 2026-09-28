## MODIFIED Requirements

### Requirement: Every provider attempt settles all applicable usage before readmission

主/子 agent 模型步骤的每次真实 provider attempt SHALL 具有独立于 Goal 存在与否的归属，并向所有适用的现有消费账本结算，包括最终未被接纳的候选。结算 SHALL 按归属幂等并等待确认；成功响应接纳 SHALL NOT 再累计已结算的消费。总 token 消费用量未知时 SHALL 保留未知状态，精确预算下关闭准入。full input 与 output 已知但缓存分类缺失时 SHALL 结算精确总消费和未知分类，不因此关闭总 token 预算准入。重试 SHALL 重新检查剩余子任务输出额度。

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
- **WHEN** 已开始的 attempt 无法取得可信的完整总 token 消费且受精确 Goal 或子任务预算约束
- **THEN** 记录用量不完整并关闭后续准入，不把未知消费当零。

#### Scenario: Unknown spend without an exact budget
- **WHEN** 总 token 消费用量不完整但没有精确预算约束且其他恢复条件均满足
- **THEN** 确认记录未知状态后可继续有界恢复，已知下界不冒充完整用量。

#### Scenario: Remaining output grant shrinks
- **WHEN** 失败 attempt 的已知输出消费减少子任务剩余额度
- **THEN** 下一 attempt 的输出上限不超过新余额，余额耗尽则不派发。

#### Scenario: Known totals with missing cache details
- **WHEN** attempt 的 full input 与 output 可确认，但 cache read 或 write 没有报告
- **THEN** 所有适用账本结算总消费各一次并保留分类未知，精确 token 预算按该总量检查，完整响应不因分类缺失重新生成。

## ADDED Requirements

### Requirement: Cache usage preserves field availability across protocols

采样归一化 SHALL 分别保留缓存读取和写入字段的可用性，区分未报告与明确报告 0。别名只在主字段缺失且含义已验证时采用；冲突或越界的缓存分类 SHALL 标为不可用并保留诊断事实，不覆盖独立可信的总输入输出。full input SHALL 表示全部输入，不能把仅含 uncached input 的值当作完整输入。

#### Scenario: Missing and explicit zero
- **WHEN** 两个 Chat、Responses 或 Messages 响应分别省略 cache read 与明确给出 read=0，且适用协议没有缺失等价于零的可靠约定
- **THEN** 前者保留未知，后者保留已知零；后续归一化和结算仍能区分二者。

#### Scenario: Independent cache read and write
- **WHEN** response 只报告 read 或只报告 write，包括 Responses 的非零 cache_write_tokens
- **THEN** 已报告桶保留精确数值，未报告桶保持未知，write 不计入 cache hit。

#### Scenario: Alias precedence and conflict
- **WHEN** 主缓存字段明确为 0、兼容别名也存在，或两者报告不同数值
- **THEN** 不把 0 当缺失；一致时只计一次，不一致时将该桶标为未知并保留冲突证据。

#### Scenario: Messages start and delta
- **WHEN** Messages 的 delta 省略缓存字段或明确给出 0
- **THEN** 省略沿用 start 中该字段的数值或未知状态，明确 0 覆盖 start；从未报告的桶不被默认成 0。

#### Scenario: Full input cannot be reconstructed
- **WHEN** 协议只报告 uncached input，缺少构成 full input 所必需的缓存桶，且没有独立总量或该协议缺失等于零的可靠约定
- **THEN** 总输入保持不完整，沿现有未知消费结算与预算路径处理，不伪造完整输入，也不因此重采样已合法完成的响应。

#### Scenario: Cache counts exceed full input
- **WHEN** 独立总量可信但缓存读写违反适用协议的输入桶约束
- **THEN** 受影响缓存分类不参与命中统计，总输入输出仍按其可信值结算，原始响应证据保留。

证据入口：`crates/codegen/sampling-types/src/{types,messages,conversation}.rs`；`crates/codegen/sampler/src/stream/{chat_completions,responses,messages}.rs`。字段 presence 和冲突 fixtures 为本 change 待实现验证。
