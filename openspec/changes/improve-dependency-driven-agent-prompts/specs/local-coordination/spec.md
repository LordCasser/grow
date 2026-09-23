## ADDED Requirements

### Requirement: Coordination guidance preserves audience ownership

Grow SHALL 为 primary 和 subagent 提供英文协作指引，分别保留整体交付责任与有界任务责任。指引 SHALL 服从现有用户约束、工具资格、active Behavior 和权限边界，不因委派或询问创建新的授权或会话所有权。

本组要求规范模型可见的协作指引及其可评估目标，不宣称 prompt 是确定性的依赖调度器、文件锁或强制行为执行器。

#### Scenario: Primary retains overall responsibility
- **WHEN** 为 primary session 生成协作指引
- **THEN** 指引要求其掌握任务级理解、核对影响全局的证据、协调委派成果并负责最终验收，不把整体问题与交付责任全部转交子任务。

#### Scenario: Child retains bounded responsibility
- **WHEN** 为 subagent session 生成协作指引
- **THEN** 指引允许其在范围和能力内自主调查、实现和验证，同时要求报告父级决策边界，不接管父会话或扩大权限。

#### Scenario: Role composition varies
- **WHEN** 同一 audience 使用 Extend 或 Full role composition，或切换具体 Agent role
- **THEN** 协作责任仍来自对应 audience，父级专属调度指引不作为所有 child 都必须遵循的共享 role 正文，动态任务状态不进入稳定 System head。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；`crates/codegen/agent/src/prompt/context.rs` 的 `render_with_renderer` / `render_role_with_renderer` 与现有装配测试。

### Requirement: Delegation guidance names sufficient inputs and acceptance

Grow SHALL 指导委派方提供与任务规模相称的目标、必要输入、待确认假设、产物、写入范围和验收条件，并以真正需要的事实、接口、决策或产物表达依赖。指引 SHALL 允许简单任务保持最小闭环，不要求固定 JSON、完整 DAG 或新增持久化任务记录。

#### Scenario: Consumer needs only a confirmed interface
- **WHEN** 一个任务实现所需的接口已确认，但接口生产方的其他工作仍在进行
- **THEN** 指引允许依据该已确认接口推进独立工作，不默认依赖生产方整个任务结束。

#### Scenario: Task is too small to benefit from delegation
- **WHEN** 一个任务可直接完成，拆分会增加协调和整合成本
- **THEN** 指引允许直接完成，不为了并发数量强制创建 subagent。

#### Scenario: Task-specific context is required
- **WHEN** 委派任务依赖父 Agent 尚未写明的决定或证据
- **THEN** task 工具说明要求显式提供这些信息，不假定 child 自动知道父级计划，也不以统一的压缩规则描述所有 host 的项目规则传递。

证据与实施入口：`crates/codegen/agent/prompts/audience/primary.md`；`crates/common/tool-types/src/task.rs::build_task_description`；`crates/codegen/agent/src/prompt/context.rs::agents_md_user_reminder`。

### Requirement: Coordination guidance distinguishes exploration execution and acceptance

Grow SHALL 指导 Agent 区分已确认事实、工作假设、候选产物和已接受成果，并分别判断是否可以探索、实现或验收采用。未知条件 SHALL 只阻塞真正依赖它的工作；基于假设的准备 SHALL 有界且可放弃，不将该假设用于修改正式共享状态或产生相应外部副作用。

#### Scenario: Design input is unresolved
- **WHEN** 实现依赖的接口或决策尚未确定，但相关机制可以独立调查
- **THEN** 指引允许限定范围的调查或验收准备，并要求实现等待所需输入，不把预测当作已确认契约。

#### Scenario: Candidate result has returned
- **WHEN** 子任务返回代码或结论
- **THEN** 指引要求核对适用前提和必要验证后才采用，不将 runtime completed 自动等同于语义验收。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；验收采用是任务上下文判断，不新增 task status。

### Requirement: Primary guidance advances independently ready work

Grow SHALL 指导 primary 在任务启动、结果返回、出现阻塞或关键前提改变后的自然检查点重新选择工作，优先处理交付瓶颈和能解锁后续工作的成果。等待 SHALL 基于实际依赖和现有工具语义；指引不得要求忙轮询、重复已委派工作或通过扩大范围保持忙碌。

#### Scenario: One result arrives before an unrelated task
- **WHEN** A 的结果已返回且可以解锁下一项工作，B 仍运行但与该工作无关
- **THEN** 指引要求及时检查 A 并推进被它解锁的工作，不默认等待 B。

#### Scenario: Useful independent work remains
- **WHEN** child 正在后台执行，parent 还有不重复且不依赖该 child 结果的有价值工作
- **THEN** 指引要求 parent 继续推进该工作，并在自然检查点处理结果。

#### Scenario: No useful independent work remains
- **WHEN** 下一步所需结果尚未到达且没有值得执行的独立工作
- **THEN** 指引允许使用真实等待机制，不制造任务或反复请求无变化快照。

#### Scenario: Multiple task wait is selected
- **WHEN** 工具说明介绍多 task ID 配合正超时的等待
- **THEN** 保持 wait-all 说明，并指导仅在下一步需要全部结果时使用该方式，不把它描述成 wait-any。

证据与实施入口：`crates/codegen/agent/prompts/audience/primary.md`；`crates/common/tool-types/src/task.rs::build_task_output_description`；既有运行时 `crates/codegen/tools/src/implementations/grow_build/task_output/mod.rs::wait_all_event_driven` 不修改。

### Requirement: Coordination guidance bounds concurrency and write ownership

Grow SHALL 指导主从双方尊重明确的写入范围，并依据独立就绪工作、资源约束和验收整合能力决定并发。工作区隔离 SHALL NOT 在指引中被描述为已经消除接口与共享假设的语义依赖。

#### Scenario: Results accumulate faster than review
- **WHEN** 待验收成果积压、共享修改冲突增加或资源竞争拖慢关键任务
- **THEN** 指引要求优先验收、整合或解除冲突，减少新任务派发。

#### Scenario: Parent would overlap an active writer
- **WHEN** primary 想接管仍由 child 负责的修改范围
- **THEN** 指引要求先确认安全交接，不将消息接收回执当作 writer 已停止或写入权已释放。

#### Scenario: Shared premise changes
- **WHEN** 接口或关键输入变化使部分在途工作或已有成果的前提失效
- **THEN** 指引要求识别并停止受影响工作、重新判断相关产物，允许不受影响的工作继续，不自动丢弃其他执行者的修改。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；现有隔离和干预语义来自 `crates/common/tool-types/src/task.rs::TaskToolInput` 与 `crates/codegen/tools/src/implementations/grow_build/task/interaction.rs`。

### Requirement: Child guidance respects inquiry and delivery boundaries

Grow SHALL 指导 child 按真实能力进行澄清和成果交付，不将 frozen-context inquiry 回答当作目标前台已采取行动、权限扩大或新验证完成。没有可用的主动中间成果发布通道时，指引 SHALL 允许通过有价值的阶段性任务结果交付，不假设不存在的通道。

#### Scenario: Clarification cannot perform required parent action
- **WHEN** child 的继续工作需要 parent 调查、重排计划或执行操作，而现有 Sideband 澄清无法提供该行动
- **THEN** 指引要求返回已完成部分、证据和具体阻塞，不无限等待 parent，同时不自行越过范围或权限。

#### Scenario: Intermediate evidence is queried
- **WHEN** parent 通过 inquiry 获取 child 冻结上下文中已存在的事实
- **THEN** 指引要求按快照范围使用该信息，不声称 inquiry 执行了新的工具验证或提交了整体计划。

证据与实施入口：`crates/codegen/agent/prompts/audience/subagent.md`；`crates/codegen/tools/src/implementations/grow_build/task/interaction.rs` 中已有 ask / send 工具描述保持。

### Requirement: Delegated results carry evidence and bounded conclusions

Grow SHALL 指导 subagent 返回自然语言的完成程度、可定位产物、关键输入前提、实际执行的验证及结果、未验证部分和待决问题。primary 指引 SHALL 要求按风险验收实际产物，并完成必要的跨模块验证，不无差别重做所有局部工作。

#### Scenario: Local tests pass but integration remains
- **WHEN** child 完成局部实现和相关测试
- **THEN** 指引要求区分局部验证与整体集成，返回剩余集成条件，不宣称整个父任务已完成。

#### Scenario: Exploration reaches sufficient evidence
- **WHEN** explore child 已取得足以回答委派问题的证据
- **THEN** role 指引要求停止扩展调查，返回事实、路径、不确定性和覆盖边界，负面结论限定在实际检查范围。

#### Scenario: Returned assumptions become stale
- **WHEN** parent 在验收时发现影响结论的输入已经变化
- **THEN** 指引要求重新检查受影响结论和产物，不以旧报告的成功措辞替代当前验证。

证据与实施入口：`crates/codegen/agent/prompts/audience/{primary,subagent}.md`；`crates/codegen/agent/prompts/agents/{general-purpose,explore}.md`。模型实际遵循程度另按 change 的场景评估记录，不由字符串测试推导。
