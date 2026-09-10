## REMOVED Requirements

### Requirement: Ordinary turns require explicit completion intent
**Reason**: 用户要求尊重 provider 原生正常结束。强制 FinishTurn 引入额外失败路径，且不能证明任务完成。
**Migration**: 普通 Turn 依据完整 provider 终止与工具/控制状态收尾；不再提供 FinishTurn 或缺失声明恢复。历史工具事实保留，不重放。

## MODIFIED Requirements

### Requirement: Portable boundaries keep tool exchanges together

Portable prefix 与 live suffix 的切点 SHALL NOT 将同一完整工具往返拆成被删除的调用和孤立结果。生成请求时 SHALL 在现有消息/native span 边界内闭合连续工具结果；wire 转换、请求 token 估算及投影证据 SHALL 使用一致的范围。

#### Scenario: Unsigned response precedes tool execution
- **WHEN** 完整 Messages 响应的 unsigned thinking 使 native 撤下，assistant 持久化后工具执行并追加结果
- **THEN** 下一请求保留该调用和结果各一次，不带无效 thinking/signature，已执行工具不重放。

#### Scenario: Several results straddle the prefix
- **WHEN** 同一批工具的多个结果分处 prefix 两侧
- **THEN** 投影保留完整配对及图片，估算与实际投影一致，不吞并后续 native span 或新消息。

#### Scenario: Normal final answer
- **WHEN** 普通模型请求收到完整、合法的正常 provider 终止和可见回复，且没有待执行调用或独立宿主继续理由
- **THEN** 正常结束本 Turn，不要求 FinishTurn；末尾标点和正文是否像行动预告不参与完成判定。

## ADDED Requirements

### Requirement: Provider termination remains distinct from host control

三 backend SHALL 保存实际收到的原生 terminal 信息，并保留独立的中性终止语义。工具调用的存在 SHALL NOT 覆写拒绝、截断或其他 provider 原因。provider 结束仅表示该响应结束，SHALL NOT 自动完成 Goal。原始终止事实 SHALL 与所属 request/attempt 关联并在候选拒收后仍可追溯。

#### Scenario: Natural stop without a completion tool
- **WHEN** Chat stop、Messages end_turn 或 Responses completed 返回正常可见回复且无业务调用
- **THEN** 普通 Turn 可正常结束，不因缺少 FinishTurn 再次采样，原始 backend 字段保留。

#### Scenario: Natural stop with complete calls
- **WHEN** 合法响应带完整可执行调用
- **THEN** 宿主按工具协议执行后续步骤，原始终止仍保持 provider 给出的原因。

#### Scenario: Refusal after complete tool output
- **WHEN** provider 拒绝响应同时包含完整工具调用
- **THEN** 记录拒绝及调用事实，生成明确未执行的配对结果，不执行该批业务工具，不把拒绝转换成普通完成或无声明恢复。

#### Scenario: Terminal received but candidate rejected
- **WHEN** 已观察到 provider terminal，随后参数/协议校验失败或宿主拒收候选
- **THEN** attempt evidence 同时保留已观察的原生 terminal 和宿主拒收/失败结果，不能将 terminal 改成未收到。

#### Scenario: No provider terminal received
- **WHEN** EOF、传输故障或取消发生且未收到原生终止
- **THEN** 不合成 provider 成功终止；按既有失败/恢复预算处理，宿主生命周期单独关闭。

#### Scenario: Provider switch after a response
- **WHEN** 响应 A 结束后切换到端点或模型 B
- **THEN** A 的 terminal 仍绑定 A 的 request/attempt 和原始 backend，后续宿主终态不能用 B 的配置覆盖它。
