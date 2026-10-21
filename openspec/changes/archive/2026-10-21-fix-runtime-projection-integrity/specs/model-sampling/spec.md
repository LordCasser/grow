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

#### Scenario: Model skill metadata separates completed results
- **WHEN** Model SkillLoad 紧邻其匹配结果且后面还有同批真实结果，历史重建或 native prefix 重置
- **THEN** 请求保留全部真实调用和结果各一次，元数据不截断批次、不产生孤立 output，原 native 内容不变。

### Requirement: Provider-required portable reasoning recovery is bounded

系统 SHALL 将明确的 Responses `reasoning_text` 或 Chat Completions `reasoning_content` 回传拒绝表示为类型化 attempt 事实。兼容状态只有在拒绝指向当前 backend、存在会改变 wire 的有效历史且当前路由尚未启用时才可改变；自动重提交 SHALL 使用同一 logical sampling 的剩余 attempt 上限与绝对期限。

#### Scenario: First explicit rejection enables replay
- **WHEN** 当前请求因缺少要求的 reasoning 字段首次被明确拒绝，且存在可改变的有效历史与恢复余额
- **THEN** ChatState 确认启用当前路由投影模式后静默重提交，不把失败候选加入 Surface。

#### Scenario: Repeated rejection does not loop
- **WHEN** 回放模式已经启用后端点仍返回相同拒绝，或不存在可改变的有效历史（Responses 仍要求完整工具往返前的非空 reasoning）
- **THEN** 系统不再次声明状态已改变，不重置 logical sampling 预算，并按既有其他恢复或终态路径处理。

#### Scenario: Chat history has no visible reasoning
- **WHEN** Chat 历史 assistant 没有可见 reasoning，端点明确拒绝缺失 `reasoning_content`
- **THEN** 当前 Chat 路由可确认一次空字段编码恢复，不能为满足字段要求虚构 reasoning 正文。

#### Scenario: Rejection identifies a different backend
- **WHEN** 错误要求的 reasoning 字段属于另一 backend，或错误不同时满足 400、thinking mode 与明确回传要求
- **THEN** 不启用当前路由的 reasoning 兼容状态。

验证入口：`sampling-types/src/conversation.rs` 的 wire 投影、`sampling-types/src/error.rs` 的分类、`chat-state/src/actor/tests.rs` 的路由状态测试与 `shell/src/session/actor/turn/sampling.rs` 的恢复测试。

#### Scenario: Runtime-only Chat history requires reasoning replay
- **WHEN** Chat 路由明确要求 reasoning_content，而当前历史只有会生成 assistant 的非空 AgentMessage 或用户 SkillLoad
- **THEN** 可确认一次启用，portable 与新增 live Runtime assistant 均编码空 reasoning_content，不借用无关 reasoning，不改变既有恢复额度；空收件批次或 Model-only 技能元数据不构成启用依据。

### Requirement: Runtime agent messages project to complete tool exchanges

有来源的 runtime agent-message context item SHALL 在 Chat/Messages 请求中投影为完整、可配对的专用收件工具调用/结果，使用从 receipt 确定的稳定身份；Responses SHALL 使用明确标记来源的独立 user 上下文，携带完整收件 JSON 并声明不是直接用户指令或人工授权，不生成模拟模型调用或 reasoning。该投影 SHALL NOT 冒充模型主动执行或触发工具 dispatch，不得追加孤立 ToolResult 或篡改其他真实工具的结果。canonical Surface 坐标 SHALL 不因 provider 一对多展开改变。

#### Scenario: Receiving a message during a tool batch
- **WHEN** 消息到达而现有真实工具批次尚未闭合
- **THEN** 消息在安全边界加入请求，完整收件调用/结果对不插断原工具配对。

#### Scenario: Provider switch and portable history
- **WHEN** 含已消费 agent 消息的请求切换现有 provider adapter 或供 Sideband 冻结
- **THEN** 完整正文、来源和稳定身份保留，portable projection 不把它作为孤立结果丢弃，也不执行历史调用。

#### Scenario: Compaction and source permissions
- **WHEN** 对含 runtime agent message 的 Surface 进行 compaction 或权限判定
- **THEN** input_ref 仍指向 canonical 事实，消息不获得 DirectUser/Interjection 权限证据；一对 provider item 不生成两份消费坐标。

#### Scenario: Responses thinking receives a Runtime message
- **WHEN** Responses thinking 路由收到有来源的 AgentMessage
- **THEN** 正文及 receipt/source/target/reply 身份完整出现一次，不生成 function_call/output 或伪造思考，canonical 权限和消费坐标不变。
