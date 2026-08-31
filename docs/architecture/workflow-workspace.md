# Workflow Workspace 架构

> **Status**: Implemented
> **Date**: 2026-08-12
> **Scope**: shell / workflow / pager

Workflow 由四个所有权边界组成，而不是一段“可变的后台脚本”：

1. **Workflow Behavior** 是 Workflow 的唯一协作协议。只有 turn 捕获的 Behavior 与
   实时 Behavior 都是 Workflow 时，Grow 才能搜索、创建、编辑、验证、发布、启动或管理
   Workflow。`deep-research` 由 builtin extractor version-managed 到
   `~/.grow/workflows/deep-research.rhai`，每次启动幂等核验后按普通 User workflow 由 Registry 扫描；不会
   引入额外的 scope、Behavior 或私有运行机制。
2. **Workflow Workspace** 归 session 所有，持久化多个草稿、唯一 Definition 焦点、派生
   来源、基线、当前内容哈希、验证哈希、保存提示和发布冲突。它不持有运行中执行器。
3. **Workflow Definition** 是可搜索、可编辑和可复用的 Rhai 定义，scope 为 Session、
   Project 或 User。修改已保存 Definition 时先派生 session 草稿；发布要求显式
   选择 Project 或 User，并使用原子写入与基线哈希检查。
4. **Workflow Run** 是 Definition 内容与启动参数的不可变快照。`WorkflowManager`、
   `WorkflowTracker` 和 `WorkflowRunStore` 分别拥有执行、状态和持久化；同一 Definition
   可同时产生多个独立 Run，句柄按 `name`、`name-2` 递增。

Rhai 引擎仍然是确定性执行层：脚本禁用运行时求值和睡眠，host 调用按序列与哈希记录
journal；`agent()`、`parallel()`、`phase()`、`complete()`、`pause()`、`await_user()` 和
`budget()` 构成编排接口。Definition 的改变只影响下一次 Run，暂停/恢复也始终使用原始
Run 快照，而不是重新解析当前 Definition。完整的脚本写作契约（meta、函数签名、Agent
选项、限制与最小示例）见 [workflow-rhai.md](../workflow-rhai.md)。

Run 的启动从实时 Workflow Behavior 复核到预检、validated hash 提交和
`WorkflowManager::launch` 共用同一 admission 临界区。启动参数只解析一次；预检与 Run
快照使用同一 JSON 值。Rhai 引擎及 Host 函数是同步协议，Host 返回值通过
`blocking_recv` 等待，因此任何 async 启动入口都必须把完整预检放进阻塞线程域，不能在
session runtime worker 上直接执行。阻塞任务异常、预检失败或 Definition hash 漂移均在
validated hash 与 Run 生成之前 fail closed。

每个 Run 在 admission 时冻结默认模型及完整 catalog sampler route。每个 catalog identity 的
provider model、endpoint transport、backend、header/query 契约、输出与温度参数、context window、
retry/stream/compaction/doom-loop policy 和 reasoning effort 属于同一快照；transport identity 与
effort 只从这份 sampler 快照派生，不再维护第二套窄 route map。`None` 是明确关闭 reasoning，而不是
“以后从 Agent Definition 或模型默认值继承”。Definition 显式选择其他模型以及 resume source model
都只能使用该 Run 启动时存在且通过普通 Task model selection 契约的 sampler；hidden、disabled 或被
`allowed_models` 排除的 catalog entry 不得借 Workflow 绕过。当前 session 默认 sampler 始终作为
Run 的基线显式纳入。

Run manifest 只持久化无 credential 的请求投影与完整契约指纹：API key、URL userinfo/query value、
literal header/query value 和 live auth callback 留在进程内的 runtime lease。catalog 热更新只影响未来
Run；已启动 Run 即使模型被删除也继续使用原 lease。进程重启后只有当前 catalog 能安全重建相同的
sensitive transport contract 时才重新附着实时 credential，否则 fail closed，绝不换成相似模型或
采入新的 sampler 字段。

admission 在写入 `Workflow::Spawned` 之前，用 writer 自己的 canonical encoder 对 credential-free
初始 manifest 执行一次精确预检；当前上限为 512 KiB。Spawn 还冻结 canonical script/args 的 BLAKE3
摘要。任一预检或 Timeline commit 失败都在 Spawn 前统一回滚 tracker 与 store，不留下 ghost Run。

Timeline 的 Spawn seed 与 lifecycle 是恢复权威；`state.json` 只是同一冻结 Run 契约下的可变进度
sidecar。恢复 resolver 总是先校验 seed 的版本、Run identity、Definition provenance、runtime route、
phase metadata 和 journal path，再接受冻结字段一致且语义有效的 sidecar；sidecar 缺失、损坏或漂移时
回退 seed，script/args 文件则必须匹配 Spawn 摘要。JSONL loader 与 Trajectory 共用这一个 resolver，
不能各自发明恢复规则。一个无效 Run 只被隔离并告警，不阻断同 session 其他有效 Run。

`<session>/workflows/<run-id>/script.rhai` 是该 Run 的不可变执行快照，不是另一个可发现
Definition，也不是自动释放到 `.grow/workflows` 或 `~/.grow/workflows` 的来源。Registry
discovery、Workspace state 和 Run snapshot 只有单向所有权关系，不做双向同步。

## 发现与编辑

Workspace 访问分类必须覆盖真实副作用：`Search` 使用 observational open，不创建目录、不恢复 publish、
不刷新 hash，也不清理 draft；`Inspect` 因持久化唯一 focus 明确投影为 `ReadWrite`。Edit、Validate、
Publish 等变更动作使用 reconciled open；ControlRun 不借由 workspace open 产生隐式写入。Tool descriptor、
call projector 和 action 矩阵测试共享这组分类。

未指定 Definition 时，主 Agent 先判断焦点是否与请求相关，再按 `name`、`description` 和
`when_to_use` 搜索 session、project、user 元数据。唯一明确匹配可说明来源和参数
后直接使用；歧义候选必须让用户选择。只有参数变化时复用 Definition；阶段、编排或 Agent
提示变化时派生草稿；没有候选时才新建草稿。

Grow 只原生编辑 session 草稿。Project/User Definition 文件对 Grow 是发布目标而不是编辑
目标：即使已处于 Workflow Behavior，也必须先派生、修改并验证草稿，再通过 `publish`
原子替换。外部编辑器仍可直接维护保存文件，Registry 会在下一次扫描时重新发现并报告诊断。

Registry 快照同时返回有效 Definition 与结构化诊断。错误文件不会静默消失；语法错误、
meta/文件名不一致、不可信路径、同 scope 重名和发布冲突都保留来源与错误码。

## 用户界面

Workflow 外只提供 `/workflow [prompt]` 与 `/behavior workflow` 快速入口。Workflow 内提供
`/workflows`、`/workflow-run` 和可运行 Definition 的动态命令。`/workflows` 分区显示
Definitions 与 Runs：前者显示焦点、scope、临时/已保存、dirty、validated、conflicted；
后者显示句柄、Definition scope/hash、状态、阶段与 Agent 进度。

动态命令只由已保存的 Project/User Definition 生成。Session 草稿属于编辑 Workspace，仍在
Definitions 分区显示并可通过显式 Run 操作执行，但不会创建同名斜杠命令，也不会遮蔽其来源
Definition。

有 Active Run 时离开 Workflow 需要重复确认，Run 继续后台执行；重新进入 Workflow 后
才能管理。暂停、预算受限和未保存草稿不会阻止切换。`deep-research` 与其他 Definition
一样出现在 Registry、动态命令、Workspace、Runs、transcript、tasks pane 和 activity
projection 中；所有显示和管理面只消费统一的 `WorkflowUpdated` 投影。
