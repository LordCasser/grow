## ADDED Requirements

### Requirement: Runtime skill results have legal provider projections
Runtime 来源的技能结果 SHALL 在各受支持请求协议中形成合法且完整的上下文表示，不含孤立工具结果、不拆开已有工具批次、不冲突 call identity。协议投影 SHALL 不改变内部真实来源，不生成虚假的采样请求、响应、用量或可调度模型调用。原始用户消息 SHALL 不包含自动追加的技能正文。

#### Scenario: Portable provider request
- **WHEN** Chat Completions、Responses 或 Messages 请求包含用户触发的技能加载结果
- **THEN** 请求包含完整加载内容及协议要求的配对身份，没有孤立或重复结果，Timeline 仍记录 Runtime 来源。

#### Scenario: Existing model tool batch
- **WHEN** 一个模型批次存在多个尚在结算的工具调用，同时收到技能插话
- **THEN** 加载投影放在批次安全边界，不把 Runtime 工具往返插进尚未完成的调用结果配对之间。

#### Scenario: Native continuation
- **WHEN** 已有 native continuation 的下一次请求需要加入 Runtime 技能结果
- **THEN** 使用可安全表达该结果的增量路径或现有 portable 重建路径；不能表达时明确终止该次执行并结算尚未消费的加载贡献，不遗漏正文继续请求。

证据入口：`crates/codegen/sampling-types/src/conversation.rs` 的 AgentMessage 配对展开、工具 ID 去重和各 backend 转换；该模式是参考，本条新增 Skill 事实投影。

### Requirement: Loaded skill content must fit its dependent request
Loaded SHALL 表示完整准备正文已获上下文发布确认，不等于后续采样必然成功。运行时 SHALL 在权威发布点及首次关联请求构造时复核当前 Surface revision、模型 / 路由与完整请求预算，覆盖用户输入、附件、技能正文、协议包装、工具定义及输出预留。首次关联步骤获得已接纳响应前，恢复重试 SHALL 保留全文；不得以静默截断、blob 地址或延后自主读取代替交付。历史 Loaded SHALL 不等于永久正文可用。

#### Scenario: Expanded content exceeds available budget
- **WHEN** 有界准备后的必要正文与完整请求无法被当前上下文预算容纳
- **THEN** 明确返回预算失败，不宣称 Loaded 或执行依赖输入。

#### Scenario: Large original user prompt
- **WHEN** 原始用户输入触发既有长提示存储策略，同时具有技能加载结果
- **THEN** 用户输入保持该策略，独立技能结果仍满足完整交付约束，不被旧 skill inline budget 静默截断。

#### Scenario: Later compaction
- **WHEN** 首次关联步骤已获得包含完整加载内容的请求所对应的已接纳响应，历史随后被正常压缩
- **THEN** 保留当时的加载事实语义，不据此承诺完整正文仍在当前上下文；新的显式加载仍可提交新的正文结果。

#### Scenario: Model or route changes before first response
- **WHEN** 正文已发布，首次关联步骤尚未获得已接纳响应，模型缩窗或路由变化使其无法完整投影
- **THEN** 重新核对预算和投影；无法满足时明确终止该次执行，保留 Loaded 历史并撤下用户输入及正文组，不以截断内容采样，也不让残留组进入后续独立请求。

#### Scenario: Model tool result cannot be delivered
- **WHEN** 模型自主加载的已发布结果在首次消费前无法继续交付
- **THEN** 用同 call identity 的简短交付错误替换当前 Surface 中的结果，保持真实工具配对及原始历史，不留下孤立调用；替换无法确认时停止采样。

#### Scenario: Retry before an accepted response
- **WHEN** 首次含全文请求失败后恢复重试，尚无已接纳模型响应
- **THEN** 全文保护继续有效，不因第一次请求已发出而解除；终止本次执行时先结算未消费内容，再允许后续独立输入。

#### Scenario: Existing image projection updates a pending group
- **WHEN** 首次已接纳响应前，用户组的附件按既有图片投影规则更新 Surface identity
- **THEN** 组关联沿来源关系跟随当前项，继续保护技能正文全文；若随后终止，撤下当前用户项及其技能结果，不因原 ID 被替换而留下失败输入。

证据入口：`crates/codegen/shell/src/session/actor/prompt_build.rs` 的 LARGE_PROMPT_THRESHOLD / SKILL_INLINE_BUDGET 与现有 context admission/compaction 路径。本条不更改文件读取的 1 MiB 上限。
