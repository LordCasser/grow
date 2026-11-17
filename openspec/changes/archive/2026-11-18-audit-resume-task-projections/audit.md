# Resume 状态投影审计

基于 `4421ab39a` 和工作区中的直属子任务终态修复。此前已用故障会话副本核对 5 个 Completed、2 个 Cancelled，挂起检查点只含根会话。上一修复处理的是加载响应已选中 child、且已包含 Ended 的情况。

恢复有三个交接点。冷启动用 working state 和 committed tail 建执行现场。Shell 用同次加载的 Timeline cut 提供读正文与必要状态。Pager 分别维护正文窗口和能驱动按钮、等待状态及导航的实体。旁路状态遗漏时，读得到历史事实的代码也可能用不出当前事实。

## 能复现的缺口

### 1. 冷恢复后，归档子任务出现在正文，详情入口打不开

`HistoryPageCut::capture` 对 indexed 路径从 `prepare_indexed(...).updates` 计算 child coverage。这批更新是 indexed page 的补充层，published prefix 由 `IndexedPages` 另行读取。(非 indexed 路径用整份 prepared history，反而会把完整历史的 child 候选带进加载响应。) `include_replayed_children` 只按这份 coverage 补 exact Spawned/Ended，因此已离开执行 working set、只存在于 published prefix 的 child 可以出现在 Latest 页而缺少导航所需的归属。Before/After/Around 也只回正文行。Pager 安装这些行时使用 `TranscriptProjection`，不会建立 `subagent_views` 或任务索引。详情入口没有已注册 child view 时直接返回。

两个临时探针分别验证了 Shell 和 Pager 的边界。Shell 在真实 JSONL 发布与冷载夹具中验证同一 child 的 Finished 不在 capture 使用的 prepared updates，却确实出现在最近页。Pager 从实际 HistoryPage reducer 采用含 Finished 的页，生成正文行，随后调用详情入口时没有打开 child。现有 Shell 测试手工从实际页收集 child 再调用 `include_replayed_children`，因此能验证 exact lookup，却绕过了生产 coverage 输入。 [Capture](../../../../crates/codegen/shell/src/agent/mvp_agent/history_paging.rs#L135) · [原测试](../../../../crates/codegen/shell/src/session/storage/descendant_recovery/tests.rs#L195) · [Pager 历史页](../../../../crates/codegen/pager/src/app/agent_view/history_window.rs#L216) · [详情入口](../../../../crates/codegen/pager/src/app/agent_view/render.rs#L83)

### 2. 深层 child 的持久归属和展示源不同

生产派发把孙任务的 Spawned/Ended 写进根 Timeline，`security_parent_session_id` 则标注它的直接委派者。恢复时根 ownership 只保留直属 child。后代遍历打开直接父的 Timeline，尝试从其中再找孙任务的 canonical Spawned；根中保存的事实来不到这个查找。完整流式重放或已经存在的 live view 可以掩盖这个缺口。真实嵌套树的整链确认应同时核对根 ledger、直接父通知来源、子 Seed 的根引用和每一条 edge 的身份。

另一个 Pager 反例验证了更窄的状态问题。即便后代恢复批次已经拿到孙任务的 canonical Ended，如果输入只有 Spawned 展示行，Pager 仍生成 Running 和 TurnRunning。直属任务的 terminal 补齐只在根循环执行，后代循环主要按展示通知驱动。该反例使用受控恢复批次，尚未在真实崩溃或整树恢复入口注入缺失的展示通知。 [生产派发归属](../../../../crates/codegen/shell/src/agent/mvp_agent/subagent_coordinator.rs#L134) · [根 ownership 过滤](../../../../crates/codegen/shell/src/session/storage/descendant_recovery.rs#L149) · [后代遍历](../../../../crates/codegen/pager/src/app/subagent.rs#L429) · [根状态应用](../../../../crates/codegen/pager/src/app/subagent.rs#L605)

### 3. 子会话的局部 TODO 会丢在 ACP 路由

Shell 的 `hydrate_local_plan` 把已提交的 Planning 同时装进工具资源并送出 ACP Plan。Pager 的根路由更新 `agent.todo`，子路由和延期读取 child transcript 则把相同消息交给 `AgentSession::handle_update`。该入口转发 tracker，tracker 忽略 Plan。因此 child 的 TODO 面板可能为空，实时更新也走相同分支。ACP 路由探针对同样的 Plan 更新做了根、子对照，根新增一个 TODO，已注册的 child 仍为零。该缺口影响展示与检查，Shell 的 Planning snapshot 和工具资源仍属于原 actor。 [Shell hydrate](../../../../crates/codegen/shell/src/session/actor/planning.rs#L488) · [根路由](../../../../crates/codegen/pager/src/app/acp_handler/mod.rs#L348) · [child 路由](../../../../crates/codegen/pager/src/app/acp_handler/mod.rs#L708) · [延期重放](../../../../crates/codegen/pager/src/app/subagent.rs#L229) · [Tracker](../../../../crates/codegen/pager/src/acp/tracker.rs#L1354)

## 由源码确认的遗漏条件

- **可选 ownership 超过 1 MiB 预算。** Shell 整份省略 payload。Pager 使用本地已验证历史恢复 canonical spawns，后者的 `replayed_children` 从空集合开始。标准分页采用的正文又不会注册 child，导致空白视图缺少用来从备选 ownership 里选择 child 的身份。payload 已包含 prompt 等字段，多次派发或大 prompt 可以达到预算。应验证同 cut 下大元数据回退与正常响应选择相同的可打开 child，避免为恢复整个旧会话重读无关正文。 [预算](../../../../crates/codegen/shell/src/session/storage/descendant_recovery.rs#L180) · [Pager 回退](../../../../crates/codegen/pager/src/app/subagent.rs#L386)
- **前置失败已提交 Ended，但没有 child Result 引用。** worktree 建立失败和 child 持久化初始化失败会显式提交父 Ended，`result_ref=None`。`DescendantOwnership` 的全量、exact 和 wire shape 都用存在 result_ref 当作保存 terminal 的条件，故即使能选中 Spawned，也可能丢掉失败终态。常规 reconciliation 在见到同一 Ended 时能为这类结果生成纯展示 Finished。复现应使终态离开执行 working set，且终态展示缺失或位于另一页。终态显示与打开不存在的 child、恢复执行、读取结果三种能力需各按原证据判断。 [失败提交](../../../../crates/codegen/shell/src/agent/subagent/handle_request.rs#L1355) · [精确所有权](../../../../crates/codegen/shell/src/session/storage/descendant_recovery.rs#L84) · [Shell 展示](../../../../crates/codegen/shell/src/agent/subagent/mod.rs#L2861)
- **resident Workflow 的新 viewer 只能看到最近已持久化的显示版本。** 冷启动会再次发布 execution-required runs，历史关闭 runs 通过明确的 Workflow 管理查询恢复。resident attach 的当前状态握手没有 workflow tracker snapshot。child 完成、活跃 child 计数和恢复阶段可走 `emit_ephemeral`，其中 manifest 会更新，transcript 只有较早的显示版本。现有客户端的 cursor reconnect 可以保留自己的 stash。新 viewer 或断线期间跨过这些瞬态变化的客户端可能看到旧阶段与旧 child 状态，具体窗口仍需双客户端实测。 [启动发布](../../../../crates/codegen/shell/src/session/actor/spawn.rs#L2216) · [Workflow 瞬态路径](../../../../crates/codegen/shell/src/session/workflow/host_service.rs#L750) · [load 握手](../../../../crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs#L1512)
- **resident 定时任务没有独立的当前快照。** 冷启动 scheduler 有 `announce_existing_tasks`，它知道 one-shot 结算后的实际状态。常规分页加载只有背景命令的 task-state replay。已运行的 scheduler 在新 viewer attach 时不会重启，已有客户端错过 `Fired` 时也可能持有旧的下次触发时间或 child 链接，后者是瞬态通知。定向检查应围绕同一个已运行的 scheduler，对照实际查询结果与新 viewer 的 Tasks。 [Scheduler 启动](../../../../crates/codegen/tools/src/implementations/grow_build/scheduler/actor.rs#L441) · [分页提交](../../../../crates/codegen/shell/src/agent/mvp_agent/mod.rs#L1214) · [Schedule display 分类](../../../../crates/codegen/shell/src/session/history/grow_facts.rs#L595)

## 其他被检查的恢复路径

| 状态 | 所见入口与边界 |
| --- | --- |
| 后台命令 | 分页的 state-only 单独从 published Task 索引、Runtime 合成终态和 tail 取数。open start 仍放在执行恢复所取的 sources，冷恢复还可补中断状态。现有 indexed、task state、分页用例覆盖这些场景。 |
| 前台 Turn、挂起树和等待 | actor 的 foreground 查询、原 Turn/checkpoint、恢复树实例与 release gate 独立于正文；子任务运行时恢复走 checkpoint 身份及 canonical 关联。故障样本只记录根节点，现有反例集中在展示与导航。 |
| Goal、Behavior、模型、权限和用量 | Control/Behavior 确认快照、Plan approval 恢复入口和 usage ledger 不靠相关正文页。历史显示单独按 cut，不能取代这些状态。 |
| 可执行 Workflow 与已关闭 Workflow | 冷启动保留 open/suspended，关闭 runs 由明确管理查询读取。这里单独追查的是 resident viewer 的当前版本。 |
| Ask、队列与协调 | 已接纳输入及 pending 交互来自 Timeline 和 actor；队列和协调有自己的读/通知入口。手工逐路径核对到发布端与消费端。 |
| Hooks | indexed reader 用 canonical Hook occurrence 重建页内展示，完整流式加载另发快照。现有用例覆盖晚到快照、工具归属、并行和子视图；页外缺工具行和重新出现时的交互只做了路径核对。 |

## 验证记录

- 固定 Rust 1.93.1。用临时探针断言故障现状，Shell 的 indexed fixture 与 Pager 的 page reducer、descendant reducer、child ACP 路由共 4 个探针均通过。探针移除后保留原有生产文件和上一轮修复。
- 同一轮测试产物中，Shell `descendant_recovery` 10 项、`history_paging` 6 项、`background_task` 12 项通过。Pager `history_window` 10 项、`reconnect` 76 项、`hook_resume` 7 项、`scheduled_tasks` 9 项通过，另有 2 项原手工 reconnect 测试忽略。
- Atlas 的 Rust impl 入口查询未解析到该符号，调用边界以源码和上述探针为准。本轮没有跑完整 workspace 测试，也没有对 Workflow 和 scheduler 做新双客户端实验。
