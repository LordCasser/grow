## MODIFIED Requirements

### Requirement: Runtime skill results have legal provider projections
Runtime 来源的技能结果 SHALL 在各受支持请求协议中形成合法且完整的上下文表示，不含孤立工具结果、不拆开已有工具批次、不冲突 call identity。协议投影 SHALL 不改变内部真实来源，不生成虚假的采样请求、响应、用量或可调度模型调用。原始用户消息 SHALL 不包含自动追加的技能正文。

#### Scenario: Portable provider request
- **WHEN** Chat Completions、Responses 或 Messages 请求包含用户触发的技能加载结果
- **THEN** 请求包含完整加载内容；Responses 使用独立 user 上下文表达 Runtime 加载结果，保留结构化来源和加载身份，不生成模拟 function_call/output 或 reasoning；Chat Completions 与 Messages 保留协议配对。没有孤立或重复结果，Timeline 仍记录 Runtime 来源。

#### Scenario: Existing model tool batch
- **WHEN** 一个模型批次存在多个尚在结算的工具调用，同时收到技能插话
- **THEN** 加载投影放在批次安全边界，不把 Runtime 工具往返插进尚未完成的调用结果配对之间。

#### Scenario: Native continuation
- **WHEN** 已有 native continuation 的下一次请求需要加入 Runtime 技能结果
- **THEN** 使用可安全表达该结果的增量路径或现有 portable 重建路径；不能表达时明确终止该次执行并结算尚未消费的加载贡献，不遗漏正文继续请求。

证据入口：`crates/codegen/sampling-types/src/conversation.rs` 的 AgentMessage 配对展开、工具 ID 去重和各 backend 转换；该模式是参考，本条新增 Skill 事实投影。

#### Scenario: Thinking route receives a user skill without model reasoning
- **WHEN** Responses thinking 路由首次或在 portable/native 历史后收到用户加载的技能，且该 Runtime 事实没有模型思考
- **THEN** 完整技能结果作为独立 user 消息出现一次，原始用户输入不改写，不依赖先触发 400、学习 reasoning 回放或用户再发继续；既有 native reasoning 与真实模型工具调用保持不变。

#### Scenario: Model-triggered skill load
- **WHEN** Responses 请求包含模型真实调用产生的技能加载事实及匹配工具结果
- **THEN** 仅保留真实调用/结果，不追加用户技能消息或重复技能正文。
