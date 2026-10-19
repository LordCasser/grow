## ADDED Requirements

### Requirement: Explicit skill references are input execution prerequisites
已解析的用户技能引用 SHALL 成为该输入执行的必需前提，按引用顺序准备；全部成功后 SHALL 在一次权威发布中消费该输入并追加原用户消息及独立正文结果。任一项失败或发布前取消 SHALL 不发布该组的输入、参数或技能正文，不开始该输入的任务采样。原始输入与显示身份 SHALL 保留，不生成重复用户消息。重复引用 SHALL 保留各自参数和结果。

#### Scenario: Single explicit skill
- **WHEN** 用户提交包含有效斜杠技能引用的输入
- **THEN** Runtime 直接派发加载操作，只有全部门控与提交成功后才执行该输入；历史中保留一条原始用户输入及关联的独立加载行。

#### Scenario: Multiple references with partial failure
- **WHEN** 同一输入部分技能已准备，后续引用确定失败
- **THEN** 停止派发余项，整组不进入模型上下文；已准备项作为历史证据保留并说明未注入，错误明确指出失败项，不显示 Loaded。

#### Scenario: Same skill with different arguments
- **WHEN** 同一输入多次引用同一技能并携带相同或不同参数
- **THEN** 每个引用按顺序准备独立结果，不按技能名称合并；全部通过后随同一输入统一发布。

#### Scenario: Builtin and ordinary slash text
- **WHEN** 输入解析为内建命令、无技能引用的特殊命令分支或普通文本
- **THEN** 保持既有路由，不仅因出现斜杠文本而发起技能加载。

证据入口：`crates/codegen/shell/src/session/slash_commands.rs` 的 resolve、parse_skill_refs 与 `actor/turn/admission.rs` 的输入消费；本条替代运行时 prompt expansion 的准入行为。

### Requirement: Selected skill commands retain explicit invocation intent
已选择的斜杠技能命令 SHALL 在输入中保留显式调用意图和所选来源，首次服务端接纳 SHALL 复核该来源。失效选择 SHALL 明确失败，不降级为普通 prompt、不重新绑定同名技能。该意图 SHALL 不构成权限批准，普通未知 slash 文本保持既有路由。

#### Scenario: Source changes before first admission
- **WHEN** 用户已选中技能，首次接纳前其来源消失、禁用或无法唯一匹配
- **THEN** 返回显式加载失败，不执行普通任务或替代来源；仅编辑参数不改变所选身份。

#### Scenario: Forged selection metadata
- **WHEN** 客户端提供显式技能意图或显示标记
- **THEN** Shell 仍复核真实来源和调用资格并执行正常授权；单独的 displayAsSkill 不授予加载权限。

证据入口：`crates/codegen/pager/src/slash/acp_command.rs`、`crates/codegen/pager/src/app/root/dispatch/queue.rs` 当前只传文字及显示元数据；`crates/codegen/shell/src/session/slash_commands.rs` 的目录解析。本条新增意图保留要求。

### Requirement: Queued skill loads follow the consumed input revision
排队技能输入 SHALL 在实际消费时加载，遵守编辑 hold 和输入顺序；已持久接纳的引用 SHALL 保存选定来源，执行前复核。尚未提交的本地编辑 SHALL 以最终提交版本解析。保存排队编辑 SHALL 沿用新 input identity 的持久替换规则，成功后才解除 hold；修改或取消的旧输入 SHALL 不发起加载。

#### Scenario: Queue wait and edit
- **WHEN** 技能输入在队列等待或处于编辑 hold，随后参数被修改
- **THEN** 等待期间不读取正文，不显示加载中；轮到消费后只执行最终版本及对应引用计划。

#### Scenario: Cancel queued input
- **WHEN** 输入在开始消费前被取消
- **THEN** 不发起技能加载或工具 Hook，不产生虚假 Loaded 记录；已完成的输入接纳 Hook 不被否认或重跑。

#### Scenario: Selected entry removed while queued
- **WHEN** 持久接纳的来源在消费前移除，但存在另一个同名或同路径条目
- **THEN** 该引用明确失败，不重新绑定替代条目。

证据入口：`crates/codegen/pager/src/app/root/dispatch/queue.rs` 的 drain 与编辑门控、`crates/codegen/shell/src/session/actor/turn/admission.rs` 的 durable consume，以及主规范 Durable input recovery。

### Requirement: Skill prerequisite failure settles only its owning input
加载失败或取消 SHALL 结算所属输入的前提，不静默继续该输入、重复运行输入 Hook 或自动重新排队。插话加载 SHALL 在既有安全执行边界发生，不拆开模型工具批次；插话失败 SHALL 不取消原任务。会话持久化致命错误仍 SHALL 遵守既有停止边界。

#### Scenario: Failed foreground input
- **WHEN** 前台输入存在确定失败的必需加载项
- **THEN** 该输入明确失败且不启动任务采样，不由 Goal 自动续跑代替其执行，也不留下永久占用的输入消费锁。

#### Scenario: Failed steering input
- **WHEN** 运行中插话的技能加载在安全边界确定失败
- **THEN** 该插话及整组准备正文均不进入可执行任务上下文，原任务可继续；准备诊断只保留为历史，输入 Hook 不重复执行。

#### Scenario: Later input after a failed prerequisite
- **WHEN** 技能加载失败的输入已保留在历史，随后另一条独立输入成功接纳并执行
- **THEN** 先前失败的输入及整组正文、参数只保留为可查历史，不因构造后续模型上下文而重新加入或被执行。

#### Scenario: Cancel during a multi-skill load
- **WHEN** 用户在若干项已准备、整组尚未发布时取消输入
- **THEN** 结算在途操作并停止派发余项，整组正文不进入 Surface，已开始项显示取消而非 Loaded；准备快照可供历史诊断。

#### Scenario: Termination after publication before first consumption
- **WHEN** 整组已发布但尚未得到首次关联模型步骤的已接纳响应，此时取消或无法继续执行
- **THEN** 保留 Loaded 的历史提交事实，明确记录后续执行未完成，并将所属输入与正文组从可执行 Surface 撤下；撤下未获确认前不启动后续采样，不自动重排队该输入。

证据入口：`crates/codegen/shell/src/session/actor/interjection.rs` 的 consume_steer_inputs、`actor/turn/admission.rs` 的输入结算及既有工具批次边界。
