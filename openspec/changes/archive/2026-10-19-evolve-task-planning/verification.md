# Verification

## 最终验收 — 2026-10-19

TP/ST/BG/TA/CS/CC 的契约已按局部计划 CAS、直接父边、稳定来源责任、等待准入、结果验收和只读投影分别核对。父项在 Link 之前修改时，child 初始提示获得同次提交的选中父步骤；Link 之后修改则保留精确重规划责任，父项完成不额外重规划。`task_link_and_parent_commit_exchange_order_without_losing_latest_requirements`、`stop_wins_when_the_last_planning_dependency_resolves`、Received/Handled ACK-loss 冷前缀测试、三层 canonical result/Accepted 摘要、Goal 无轮询和权限/预算门控均在最终 Shell126 逐名通过。`suspended_workflow_rebinds_the_same_open_child_before_original_turn_releases` 也已通过 focus125 和 Shell126，不再是待补门禁。

嵌套结果测试现在在独立子进程初始化临时 GROW_HOME，canonical writer 与生产 reader 使用同一隔离根；不再依赖初始化之后修改进程环境。曾误写的三份纯测试会话已校验 ID/cwd/Timeline 并完整移存 `/tmp/grow-v2-4-0-isolated-test-artifacts/nested-plan-workspace`。

最终包级回归为 Shell126 **4251 passed / 0 failed / 5 ignored**、Pager124 **7503 / 0 / 12**、Tools121 **2416 / 0 / 7**。日志分别为 `/tmp/grow-v2-4-0-shell-final126.log`、`/tmp/grow-v2-4-0-pager-final124.log`、`/tmp/grow-v2-4-0-tools-final121.log`。ChatState108 567/0/2、tool-types106 123/0、Agent110 482/0 及各 backend 的既有通过记录继续作为相应未改责任层的证据。CLI122 构建并报告 `grow 2.4.0 (8cc20454)`；后续修改仅为测试夹具，未更改生产语义。

当前真实进程验证共九项：CLI122 的 root、三层树、部分释放、并发恢复和普通 continue 五个 exact PTY 各 1/1（`/tmp/grow-v2-4-0-pty122-results.json`）；Plan approval 与 Defer 后冷恢复审阅为 2/2（`/tmp/grow-v2-4-0-plan-pty122.log`）；normal/minimal Skill 挂起、原快照恢复、导出和 headless 继续为 2/2（`/tmp/grow-v2-4-0-skill-surfaces123.log`）。

验收以生产 owner 的不变量为单位组合 Timeline、Actor、coordinator、存储故障注入、UI reducer 与真实进程证据。下方旧矩阵记录各阶段已运行和未穷举的排列，不再把缺一个巨型组合夹具当作实现阻断，也不声称每个排列都在实机执行。物理断电/满盘、所有状态交叉组合、Linux/Windows 实机和真实 MCP 多轮互操作未运行。MCP 多轮按用户决定暂缓，其门禁保持关闭。版本总体证据见 [联合验证](../2026-10-19-prepare-v2-4-0/verification.md)。

## 归档后校验 — 2026-10-19

本轮六个 change 已归档，规范合并与引用修正完成。`openspec validate --all --strict --no-interactive` 为 **16/16**，`openspec validate --archived --no-interactive` 为 **605/605**，日志 `/tmp/grow-v2-4-0-openspec-all128.log` 与 `/tmp/grow-v2-4-0-openspec-archived128.log`。39 处迁移后的 Markdown 引用均可解析；六份 tasks 无未完成项。格式与 diff 检查通过。当前唯一 active change 为用户暂缓的 `add-mcp-multi-round-elicitation`。

## 历史阶段证据

以下保留原始诊断、失败和阶段矩阵；其中的“当前 / 待运行 / 未实现”描述当时状态，由上方最终记录覆盖。

## Current evidence reconciliation — 2026-10-19, Shell111 / focus118 / focus120 / core108 / CLI113

本节是当前证据权威，更新下方 10-18/19 历史审计中的“未执行”“未发现入口”和旧 clean suite 基线。测试范围按真实具名入口记账；包级总数只说明该次包测试整体无失败，不替代具体场景断言。

| 证据 | 实际结果 | 覆盖含义与边界 |
| --- | --- | --- |
| `/tmp/grow-v2-4-0-tool-types106.log` | tool-types 123 passed, 0 failed。 | LocalPlan revision、差异、tombstone、immutable/coalesced replan、scope/release 与 child revision local-owner 组件覆盖。 |
| core108 | ChatState 567 passed, 0 failed, 2 ignored；Tools 2416 passed, 0 failed, 7 ignored。 | Timeline/canonical result 与 Tools package 当前全量结果。 |
| `/tmp/grow-v2-4-0-shell-final111.log` | Shell 4244 passed, 0 failed, 5 ignored。 | 下表只把此日志中逐名 `ok` 的 actor、waiter、Goal、Workflow 与 completion 测试记作运行证据。 |
| `/tmp/grow-v2-4-0-pager-final111.log` | Pager 7497 passed, 0 failed, 12 ignored。 | Pager111 当前全量通过；planning acceptance 中列出的 live/replay 与完整客户端行为仍需逐项对应测试，不能由总数推定。 |
| CLI113 + `/tmp/grow-v2-4-0-pty113-{root,tree,partial,concurrent,baseline}.log` | 5 个 exact PTY tests 各 1/1 通过：root resume 10.82s、nested tree 11.26s、partial release 10.39s、concurrent resume 4.53s、ordinary continue 7.76s。CLI113 相比 CLI111 仅加入 Plan diagnostic trace。 | 证明当前新进程树恢复路径；PTYS 未构造 Planning wait/replan 或 Workflow pending-child 责任。组件证据仍逐项按下表判断。 |
| `/tmp/grow-v2-4-0-focus103-results.json` | 三项各 1/1：`compaction_reminder_uses_current_timeline_plan_references`、`idle_goal_with_live_dependency_emits_no_retry_permits_or_provider_requests`、`paused_goal_wait_does_not_resample_when_dependency_wakes`。 | Timeline 当前计划引用进入 compaction reminder；活动依赖等待不产生 retry permit/provider request；暂停等待在依赖唤醒时不重采样。长时间 idle 调度计数和权限/依赖 simultaneous arrival 未被这些三项本身覆盖；Stop/最后依赖优先级由 focus118 单独覆盖。 |
| `/tmp/grow-v2-4-0-focus108-results.json` | `received_replan_ack_loss_cold_retry_reuses_receipt_and_consumed_input` 与 `handled_receipt_ack_loss_cold_retry_preserves_new_replan` 各 1/1。 | 已持久化后 ACK 丢失时重建 ChatState Timeline prefix，重试复用原 receipt，且保留更新的待处理来源责任。不是 OS 进程退出，也未重建完整 SessionActor/coordinator。 |
| `/tmp/grow-v2-4-0-focus109-results.json` | `nested_planning_result_and_summary_follow_direct_parent_edges` 1/1。 | 实际 root→child→grandchild canonical 结果沿直接父边传递；包含明确 Accepted review 与 bounded summary。未覆盖整棵树冻结/恢复或各层 ACK 故障。 |
| `/tmp/grow-v2-4-0-focus118-planning-spawn.log` | `task_link_and_parent_commit_exchange_order_without_losing_latest_requirements` 1/1。 | 真实 TaskTool 与 actor control gate 交换 link/父计划提交顺序；child 要么绑定已提交要求，要么留下精确 replan responsibility。 |
| `/tmp/grow-v2-4-0-tools-final121.log` and `/tmp/grow-v2-4-0-coordinator-race13.log` | `cancel_parent_session_rejects_late_spawn_until_admission_reopens` and `child_finishing_after_freeze_is_joined_once_before_retirement` each `ok`。 | 冻结后 late spawn 不启动；冻结期间完成的 child 被 join 后只 retire 一次。与上行 parent commit 顺序测试共同覆盖 TP-05/S-03 的提交与执行准入边界。 |
| `/tmp/grow-v2-4-0-focus118-stop-wake.log` | `stop_wins_when_the_last_planning_dependency_resolves` 1/1。 | 已接受 Stop 后才到达最后依赖；原 Turn 结束、无下一次 provider/approval，答案仍可观察且规划责任保留，覆盖 TP-25/BG-03 的竞态门禁。 |
| `/tmp/grow-v2-4-0-focus120-late_observation_after_unknown_result_preserves_the_original_call_on_resume.log` | `late_observation_after_unknown_result_preserves_the_original_call_on_resume` 1/1。 | OutcomeUnknown 原调用恢复后接收 late canonical evidence；原调用结果只保留一份并记来源观察，关闭 suspend change 的 S-17 结果边界。 |

focus103/108/109 的具名测试有 focused 通过结果；Shell111 又逐名通过下列规划相关测试，包括 ACK 冷重建、等待、required-link review、父子 replan、结构化输出等待和三层结果摘要。focused 与 full-suite 证据均为组件/actor 层，不等于 acceptance.md 的整行验收。

| 验收组 | 已有具名运行证据 | 仍未覆盖的精确边界 |
| --- | --- | --- |
| TP-06–09、TP-16、TP-19 | tool-types106 的 LocalPlan diff/replan/tombstone/scope 测试；Shell111 的 `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned`；focus118 的 `task_link_and_parent_commit_exchange_order_without_losing_latest_requirements`。分层覆盖计划差异、payload immutable/coalesce、删除身份、父内容变更→child Handled→完成状态不重派，以及父计划提交与 Task link/spawn 顺序。 | 仍没有活跃执行 owner 下 scope-change disposal 的副作用计数；没有已知错误路径要求将其与所有其他状态组合成单一 fixture。 |
| TP-20–22、TP-25、BG-03/09 | Shell111 的 `task_and_command_waits_apply_any_all_terminal_and_identity_rules`、`planning_wait_rejects_missing_and_foreign_dependency_identities`、`cancelled_and_unavailable_task_outputs_wake_without_success_claim`、`question_and_permission_answers_wake_the_real_planning_wait`；StructuredOutput wait `structured_output_completion_preflight_waits_before_next_provider_step`；focus103 的 paused dependency wake 不重采样；focus118 的 `stop_wins_when_the_last_planning_dependency_resolves`。 | 状态、身份、失败、人类唤醒及 Stop 与最后依赖同到达由 actor/component 路径覆盖。waiter 在等待前订阅交互变化，Task wait 串行查询终态，Command wait 对已完成快照立即返回；没有已知剩余竞态要求把所有注册顺序组合进单一 fixture。 |
| TP-26–28、BG-04/05 | Shell111 `required_link_result_stays_blocked_until_current_requirements_are_reviewed`，以及 `goal_complete_command_enforces_required_delivery_but_ignores_unrelated_todo`；tool-types106 optional/required link gate 测试。 | Gate 测试使用已持久的 Planning facts；三层用例覆盖结果传递但未覆盖 worktree integration 与多执行弃用处置。 |
| ST-01–05、ST-14/15 | Shell111 的 `received_replan_ack_loss_cold_retry_reuses_receipt_and_consumed_input` 与 `handled_receipt_ack_loss_cold_retry_preserves_new_replan`，均经过真实 Planning handler/Timeline prefix 重建；focus118 的 Task link/父提交交错测试另覆盖两种提交次序。 | 不模拟 OS kill/full coordinator graph；Planning source commit 前写失败、容量反压、Consumed 未 Handled 后终态/旧实例未知仍是测试边界，但没有证据表明这些缺项造成已知责任丢失。 |
| ST-16、BG-08 | Shell111 的 `nested_planning_result_and_summary_follow_direct_parent_edges` 验证 root→child→grandchild canonical result 沿直接父边传递并保留 Accepted review/有界摘要；CLI113 的 tree/partial PTYs 验证 suspend/release 和原 Turn 结果恢复。 | 缺少的交叉点仅由 suspend-session-tree S-25 跟踪：Workflow 等待同一原 child 时穿过树挂起。两层身份和结果责任各有真实运行证据，不要求再做包含所有规划状态的树型巨型 fixture。 |
| CC-01/03 | focus103 实际从 Timeline 读取当前计划引用进入 compaction reminder；Pager31 `async_compaction_notification_preserves_latest_todos` 验证压缩中较新 TODO 胜出。 | 尚无 fork 后 child projection/权限隔离入口，也未把压缩发布与 Planning 更新、Consumed 未 Handled 恢复责任放进同一持久发布竞争。 |

`acceptance.md` 中仍标为“未执行”的状态表示该 ID 的完整输入、竞争与结果断言尚未全部通过，不表示该 ID 下没有任何测试运行。已具名通过的局部路径按上表记录，不要求组合巨型 fixture。实现任务 2.3 的差异/重规划链、4.2 的身份与 Any/All 结果规则、4.5 的 required-link review 和 5.4 的树挂起路径已有分层运行证据；完整验收仍受少量明确竞态边界限制。CLI113 PTY 与 Shell111 actor/component tests 按持久 identity 和交接事实组合核对。

### 最小剩余跨 owner 门禁

已由具名运行测试覆盖的 parent commit/spawn 顺序、冻结后拒绝新 spawn、Stop 与最后依赖竞争及 late OutcomeUnknown evidence 不再列为待补门禁。以下仍不能单靠已有分层事实推出：

| 验收 ID | 现有证据为何还不能推出 | 最小补充断言 |
| --- | --- | --- |
| S-25 | `released_root_cold_start_pauses_unstarted_workflow_for_explicit_same_run_resume` 与 `accepted_final_response_resumes_original_turn_without_provider_or_prompt_replay` 覆盖同 Run Workflow 恢复和原 Turn gate；新增 `suspended_workflow_rebinds_the_same_open_child_before_original_turn_releases` 夹具现已修正为在 SuspendRequested 前写入原 Turn/response。 | 该新测试尚未运行。下一次统一构建/执行需确认挂起后同一 Run 重新 Query 原 child、无 replacement Spawn，且原 Turn 仅在恢复 owner 安装后释放。 |

Task/Command registration 的其他任意排列、所有 Goal 状态 cross-product、OS 物理断电/磁盘写满及单个包含所有 child/Workflow/Goal 状态的巨大 fixture 均不作为追加门禁。其他行保留具体覆盖限制，但没有单独的已知不变量缺口时不作为发布阻断。

### Stop/dependency final-wakeup regression — added, not run

`stop_wins_when_the_last_planning_dependency_resolves` 使用真实 Messages provider fixture，在 StructuredOutput turn 停于 Question + Permission 两依赖时接受 Stop，再于 TurnEnded 持久后记录两个答案；它断言只结束原 Turn、无第二次 provider 请求或 PlanApproval interaction、旧 continuation/counters 不变，答案仍可观察且 planning wait 责任保留。该测试已在 focus118 通过，见上表。

## Implementation in progress — 2026-10-18

用户已授权本版实现。局部计划由 `SessionActor::handle_planning_command` 和 Timeline `Planning` CAS 事实持有；Task 关联、直接父级交付、弱关联重规划、依赖等待和必需交付验收已接入生产路径。来源责任与 canonical child result 摘要沿直接边传递，未以父列表覆盖 child 计划。

截至本记录，pinned core 运行通过 ChatState 560 项、tool-types 121 项、tools 2412 项（日志 `/tmp/grow-v2-4-0-core-tests.log`）。之后新增的摘要、交互唤醒和挂起联合代码仍需重新验证。Shell 诊断回归为 4091 passed、25 failed、5 ignored、5 filtered；大部分失败来自本轮测试 helper 的 LocalSet/Weak 所有权回归，修复后尚待重跑，不能算为生产通过。真实树恢复 PTY 后续已有 3/3 精确场景通过（见下方最新运行记录）；本 change 仍 active，不归档。

下方 Scope 和 Not run 是最初文档交付的历史记录，并非本轮实施状态。

## Scope

- 日期：2026-10-18。
- 文档核对 HEAD：`dd034d2dae82c03bd9715a3f19d1c17b49402a80`。
- 产品代码核对基线：`0a0dd07cd3aa35d39365ae971df8f9b316185c9d`，后续相关提交为 OpenSpec 文档。
- 仅新增 `openspec/changes/evolve-task-planning/`。未实现产品代码、未修改主规范、未归档；按用户后续要求，仅提交本 change 文档。
- 开始编写前工作区已有未跟踪的 `fix-null-chat-usage-details/` 与 `unify-skill-loading/` change；它们不属于本次范围。验证期间观察到前者被并行工作归档，另有 sampler/sampling-types、开发指南和 model-sampling 主规范变化。本次未写入这些路径，也未撤销或修复这些并行变化。已有 `suspend-session-tree`、`retain-plan-for-later-review` 等不修改。

## Evidence review

已按源码核对 TODO 的 Resources 写入与异步保存、task owner 与直接父级、nested `surface_completion=false`、事件 waiter、parent message Received/Consumed、child runner 终态、Goal 内外层 idle 准入、turn-end 假完成 UI 及 compaction reminder。入口见 [design.md](design.md) 的 Current implementation 表；当前能力与未来需求分别标识。

主规范核对范围：session-timeline 的持久权威、消息、结果/收尾/清理与恢复；behavior-goal 的执行权威、预算、revision 与全目标审计；tool-authorization 的直接关系与权限；client-surfaces 的 replay/恢复/观察；context-compaction 的发布与历史来源。冻结交互对照未实现的 `suspend-session-tree`，不将其当作当前已有能力。

## Review and static checks

按项目要求由 Luna 进行一次只读对抗性审查及修正后复核，以下六项已闭合；复核未发现阻断性文档缺口：

| 审查发现 | 修正与验收落点 |
| --- | --- |
| Consumed 未 Handled 的普通退出可能被误读为可恢复 | ST-04 只允许合法冻结继续；ST-15 对终态记未应用，未知实例不提前制造终态 |
| 已 Received 消息不能由发送方单方 supersede | 保持原消息 identity/payload，ST-14 按消费边界和已消费来源集合确认覆盖 |
| 直接父级交付缺少 root Goal 验收联系 | ST-16/BG-08 使用逐级局部交付摘要，不复制 grandchild 模型回执，不授予 Task child Goal authority |
| 多次 yield 可能每次重新取得预算/max-turns | TP-31 明确原 execution owner 跨唤醒和恢复累计 |
| 父 TODO 取消与执行取消、解除义务混淆 | design 区分三种操作，TP-32 验证通知未达时在途工作仍可能继续 |
| 暂停/权限等待可能被更新唤醒绕过 | BG-09 明确只保留责任，执行仍由原 owner/control 准入 |

静态验证结果：

| 检查 | 结果 |
| --- | --- |
| `openspec validate evolve-task-planning --strict --no-interactive` | 通过 |
| `openspec validate --all --strict --no-interactive` | 19 passed，0 failed（当前工作区，含并行工作当时状态） |
| `openspec status --change evolve-task-planning` | 4/4 planning artifacts complete，仅表示 proposal/specs/design/tasks 齐全 |
| Python 静态映射/文本检查 | 12 个文件、6 个 capability delta、23 个 Requirement、69 个唯一 Scenario；与 acceptance.md 的 69 行一一对应；本记录形成时均保留未执行状态。当前局部运行证据与完整 ID 状态见文首 reconciliation。 |
| `git diff --check` | 通过；另用 Python 覆盖尚未跟踪的新 change 文件 |
| 范围核对 | 本次所有写入均位于新 change；与开始时 OpenSpec 哈希快照对比发现的其他变化仅记录为并行工作，不宣称整个工作区未变 |

OpenSpec 结构校验和文档审查不证明运行行为已经实现。仅 tasks 第 1 节勾选，其余实施、运行验证与归档全部未勾选。

## Not run

[acceptance.md](acceptance.md) 中所有运行场景尚未执行。未运行 Rust 编译、单元/集成/端到端测试、真实冻结恢复或产品 UI 验证；本阶段没有代码实现可验证。未运行 archive。

## Acceptance evidence audit — 2026-10-18

以下为本轮只读源码审查和测试名映射，没有运行 Cargo。前面记录的 ChatState、tool-types、tools 与 Shell 诊断属于之前的局部测试轮次；它们没有逐项执行 acceptance.md 中的输入竞争、崩溃、恢复和实际 UI 场景，不能据此标记任何 TP/ST/BG/TA/CS/CC 场景通过。主要实现证据位于 `crates/common/tool-types/src/planning.rs` 的 LocalPlan/CAS 规则、`crates/codegen/chat-state/src/timeline.rs` 的 Planning 事件验证、`crates/codegen/shell/src/session/actor/planning.rs` 的改动派发/等待/交付，以及 subagent 的直接父级结果路由。

### Local planning and weak links

| 场景 | 现有相关测试/实现 | 覆盖判断 |
| --- | --- | --- |
| TP-01～TP-05 | `subagent_spawn_preserves_local_plan_step_association`；Timeline Planning CAS 与 actor `handle_planning_command`/`commit_local_plan`。 | 没有父子 owner 三项隔离、跨 turn 保持、并行执行身份，或 spawn/提交 barrier 的集成测试。 |
| TP-06～TP-09 | tool-types `effective_diff_only_replans_for_intent_changes`、`requirements_priority_noncompletion_status_and_remove_are_triggers`；`old_replan_ack_does_not_clear_newer_pending_revision`、`pending_replans_coalesce_but_received_payloads_remain_immutable`、`dispatching_replan_payload_is_immutable_after_a_new_source_change`；Shell actor `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned`。 | 新增真实 actor 链：父项内容更新 → direct-child Dispatching → child Received → child 修改自己的 LocalPlan 并 Handled → 父接收 receipt；随后仅把父项标为 Completed，断言没有第二次 replan。此测试尚未由本轮 Cargo 执行。 |
| TP-10～TP-13 | `receive_plan_guidance` 将父变化写成 child 本地 AgentMessage/Planning 事实；`planning_completion_blockers` 保留未处理来源；actor 结果与等待测试见下表。 | 调查保留、命令等待中的父变更、用户信息等待不被 receipt 关闭均无对应运行测试。 |
| TP-14～TP-15 | `advance_replan` 与 immutable payload 状态转移；`late_undispatched_ack_is_recorded_once_without_faking_a_result`。 | 有旧 ACK/late ACK 局部证据；连续来源更新、A 处理中 B 到达与真正 Received/Consumed/Handled 故障恢复链未覆盖。 |
| TP-16～TP-17 | `replace_tombstones_omitted_steps_and_never_reuses_ids`、Timeline `planning_timeline_preserves_delete_and_replan_identity_transitions`、`undispatched_link_is_idempotent_unapplies_replans_and_requires_release`。 | 删除/tombstone/no-reuse 与 undispatched 状态有纯状态覆盖；与子任务启动/完成并发及副作用计数未测。 |
| TP-18～TP-19 | `receive_plan_guidance` 验证 child session；`deliver_planning_changes` 按 direct link 派发；`scope_change_requires_open_work_disposal_but_keeps_terminal_history_valid`。 | 缺三层仅沿局部边传播、循环阻断和活跃义务下范围变更的 runtime 测试。 |

### Waiting and acceptance

| 场景 | 现有相关测试/实现 | 覆盖判断 |
| --- | --- | --- |
| TP-20～TP-22 | actor `planning_result_requires_a_canonical_child_reference`、`planning_result_requires_expected_owner_but_allows_unlinked_task`；`validate_plan_wait`/`plan_dependency_ready` 验证真实 owner；`question_and_permission_answers_wake_the_real_planning_wait`。 | Question/Permission wake 测试现检查唤醒前后复用同一 `TurnContinuation`（含 tool/sampling/stop/schema 预算），并断言 Timeline 仍只有一个 `TurnStarted`，覆盖“不重新开 turn/不补预算”的 actor 边界；尚未执行。仍未覆盖 Task/Command 注册前中后完成的 any/all 矩阵，以及失败/取消/Interrupted/OutcomeUnknown/丢产物的完整唤醒结果。 |
| TP-23～TP-24 | `await_planned_dependencies` 持有当前 admitted turn，child/grandchild 等待通过 task waiter 唤醒；无 wait 时返回 `NotWaiting`。 | 没有模型 turn 结束后仍保持 child 非终态、也没有正文“等待测试”不生成执行的端到端证据。 |
| TP-25 | `await_planned_dependencies` 使用原 `PromptOrigin`；GoalContinuation 在等待前后检查真实 FIFO，并返回 `YieldGoalForFifo`；测试 `pending_fifo_yields_goal_wait_before_dependency_is_ready`。 | 有“等待未就绪时 FIFO 可先执行”的 actor 测试；Stop/预算/max-turns 竞争与 provider 请求/预算累计没有运行覆盖。 |
| TP-26～TP-30 | `planning_completion_blockers` 检查 required link、review、pending replan、wait；`accept_planning_task_result` 校验直接关联 child canonical reference；`child_planning_summary_is_bounded_and_keeps_snapshot_identity`。 | 有必需义务与 summary 的模型规则；没有父显式验收、复核宽松版本、多个 TODO 阻塞范围、弃用执行、worktree 未集成等行为测试。 |
| TP-31～TP-32 | `TurnContinuation`/runtime 累计状态的其他局部测试在前轮日志；本轮未重跑。父取消时 `Planning` 事实保留 child execution owner。 | 缺 child 多次 yield+恢复不重置预算/owner 的运行证据，以及通知不可达时父项取消后在途命令仍可见的测试。 |

### Durability, delivery, restoration, authority and surfaces

| 场景 | 现有相关测试/实现 | 覆盖判断 |
| --- | --- | --- |
| ST-01～ST-03 | Timeline `planning_timeline_preserves_delete_and_replan_identity_transitions`、`planning_timeline_accepts_completion_only_revision`；actor commits 用 `record_timeline_event_durably`。 | 有 Timeline 纯验证；写前后崩溃、Resources 篡改重建和 ACK 失败没有故障注入。 |
| ST-04～ST-15 | Runtime checkpoint/notification inbox/coordinator 各有局部测试；后续 suspend-session-tree 有真实 PTY 的 nested freeze/cold-resume、partial release、concurrent claim 3/3，以及 coordinator child-after-freeze retirement 1/1。 | 多节点同身份冻结/恢复的这些精确场景已有运行证据；其余 ACK 故障边界、冷崩溃/fork/rewind、容量反压、压缩竞争和 ST-16 三层结果集成仍缺整体验收。 |
| ST-16 | tool-types `child_planning_summary_is_bounded_and_keeps_snapshot_identity`；Timeline `canonical_child_result_retains_the_original_bounded_planning_summary`；direct parent result helper复用 canonical summary。 | bounds、snapshot identity 与 canonical reuse 有纯模型/Timeline 证据；没有三层验收后 child final result 到 root 的集成验证。 |
| BG-01～BG-02 | Goal `every_continuation_audits_the_full_goal_before_planning_a_local_slice`；`await_planned_dependencies` 以 notification/watch 和最长 25 秒 waiter 等待，不直接调用模型。 | 没有长时间无事件时 provider 调用数、actor idle 调度数和 blocked-turn 数的测试；也没有“一项等待、另一项可做”的执行级公平性覆盖。源码存在定时重检，因此仅凭静态结构不能宣称无重排/无忙循环已证明。 |
| BG-03 | `pending_fifo_yields_goal_wait_before_dependency_is_ready`；`question_and_permission_answers_wake_the_real_planning_wait`。 | 测试覆盖 Goal 自动续跑边界及 Question/Permission wake 的原 cursor 保留；没有控制/Stop/暂停/权限排队同时到达的完整优先级竞争，或结果事件与 FIFO 同时到达时的完整集成矩阵。 |
| BG-04～BG-09 | `planning_completion_blockers`、Goal 审计测试、`accept_planning_task_result`；`receive_plan_guidance` 与暂停状态沿用现有准入。 | 缺必要交付最终 gate、无关未完成项、Goal revision 隔离、已批准 Plan 边界、三层验收和 Paused/预算关闭时零采样/零批准的 actor/端到端测试。 |
| TA-01～TA-04 | `validate_plan_wait`、expected parent-step/result reference 核验、`receive_plan_guidance` 的 child identity 检查；workflow/permission gate 仍在既有入口。 | 有部分 fail-closed 逻辑，缺 sibling/历史引用、越权子任务、workflow child 与挂起 child 的集成权限验证。 |
| CS-01～CS-04 | 已有 turn 状态、Timeline project、bounded context 的局部投影；本轮未识别到针对 acceptance 的端到端测试函数。 | 所列 UI/headless/replay 场景全部仍未证明。 |
| CC-01～CC-04 | LocalPlan context bounded 到有限 step/link/replan/wait 项；ST-16 summary 有界；Timeline 保留 source facts。 | 没有压缩、fork、迟到消息与 Consumed 未 Handled 的组合恢复测试；有限投影不是整项验收证据。 |

### Adversarial findings

本轮补充了父非完成修改的真实 actor replan 链、Question/Permission waiter 和 StructuredOutput 实际 runloop 覆盖。StructuredOutput 真实 runloop、父子 replan actor 链均有 focused 通过结果；Question/Permission cursor 测试仍需当前运行记录。它们均不足以将 acceptance 整行标为通过。发布前至少要执行最终 Shell 全 suite；还缺 Task/Command 注册时序与 any/all/失败终态矩阵、Stop/预算/max-turns 与唤醒竞争、Goal 长等待的 provider/idle 调度计数、必需交付的完整 gate、树恢复以外的 ACK/崩溃边界，以及 ST-16 三层验收集成。Retained Plan 的 P-17 故障注入和 P-20 在途查询边界另见 retain change verification。结构/编译检查或局部历史日志不能替代这些运行门禁。

## Historical execution evidence — 2026-10-19, before focused follow-up runs

本节只记录父任务报告的实际运行结果，不将 suite 数量解释为整张 acceptance matrix 通过。最新确认的包级结果为 Pager 7477/0、tools 2412/0、ChatState 561/0、tool-types 122/0、sampler 259/0、sampling-types 306/0、Minimal 94/0。Shell 的真实 runloop 测试 `structured_output_completion_waits_for_live_question_and_permission_without_resampling` 已 focused 通过；它让真实 provider 先返回 StructuredOutput 调用，在 Question 和 Permission 均未解决时确认 provider 请求仍为 1、Turn 未终态，解除等待后同一 Turn 完成。该证据覆盖 StructuredOutput 分支的等待继续，不覆盖 TP-20～25 的完整注册时序、失败状态、Stop/预算竞争或 idle 计数矩阵。

父子重规划 actor 链 `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned` 已在 `/tmp/grow-v2-4-0-replan-focused-final2.log` focused 通过。此前 Shell 诊断轮有 4153 passed、2 failed、5 ignored，其中两项正是该链测试和上述 StructuredOutput 测试；两项后续 focused 回归均通过，但修复后的 Shell 全 suite 尚未确认。此前关于 `question_and_permission_answers_wake_the_real_planning_wait`、`pending_fifo_yields_goal_wait_before_dependency_is_ready` 和其他测试的静态映射不变；没有本轮新执行结果可将其所在验收行整体改为通过。

因此 acceptance.md 中 TP/ST/BG/TA/CS/CC 行仍保持未整体验收，tasks.md 第 2—6 节也不勾选：已通过的包级 suite、actor 和 PTY 子场景尚不足以满足各项持久失败注入、三层交付、端到端和客户端行为。当前最小剩余门禁是：修复后的 Shell 全 suite；Task/Command 注册竞态、非成功结果及 any/all；长等待 provider/idle 计数和 Stop/累计预算/max-turns；BG-04～09 必需交付/Goal 权限；剩余 ACK 故障注入与 ST-16 三层验收；以及 CS/CC 客户端、压缩和 replay 覆盖。

## Current source mapping — 2026-10-19

以下补充映射对应 `crates/codegen/shell/src/session/actor/tests/planning_dispatch_tests.rs`。其列出的测试源码均有明确断言；本节下面的旧运行记录保留为历史，当前通过状态以上方 Shell111 逐名结果为准。Acceptance 状态保持未执行，任何一项测试都不把整行场景自动升级为通过。

| 覆盖主题 | 真实断言与边界 |
| --- | --- |
| Task/Command any/all、终态及身份（TP-20～22） | `task_and_command_waits_apply_any_all_terminal_and_identity_rules` 使用 actor 的真实 dependency waiter 和实际启动的 Terminal 命令：Any 在 Failed Task 终态、Command 仍运行时解除；All 仍等待，命令被 kill 后以已完成的 Interrupted 命令解除；已终态非零退出码命令可在注册后立即解析。Task 状态来自 `PlanningTaskSnapshots` 测试 backend，不经过真实 task coordinator/child runner。`planning_wait_rejects_missing_and_foreign_dependency_identities` 断言缺失 Task/Command 与另一 session 所有的命令拒绝且不写入 wait；尚无注册前/注册中/注册后完成的同步竞态矩阵，也没有 Task 成功/取消/OutcomeUnknown、命令 OutcomeUnknown 或产物丢失的逐类断言。 |
| FIFO、Question/Permission 与真实等待（BG-03、TP-25） | `pending_fifo_yields_goal_wait_before_dependency_is_ready` 断言 GoalContinuation 在依赖未就绪时让出给已排队用户输入，wait 保留。`question_and_permission_answers_wake_the_real_planning_wait` 通过实际 actor pending-interaction guard 触发两类 waiter 唤醒，断言 wait 清理、复用原 `TurnContinuation`（含预算游标）且 Timeline 只有一个 `TurnStarted`。`structured_output_completion_waits_for_live_question_and_permission_without_resampling` 另有真实 runloop/provider 等待分支证据：仍有活动 Question/Permission 时 provider 请求数为 1、Turn 不终态，解除后同一 Turn 完成。它们没有覆盖 Stop、预算/max-turns 与 waiter 同时到达，也没有长时间等待期间 provider 请求数、idle 调度数或 blocked-turn 数的计数断言；普通正文“等待测试”仍不等于真实等待的反采样端到端场景（TP-24）。 |
| 父项重规划与直接回执（TP-06～09、14～15、18） | `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned` 使用两个 SessionActor、各自 timeline 与 session backend channel，断言父内容修改派发带稳定 id 的 Dispatching、未确认时重试、child durable Received 后停止父侧重试、child consume 后修改自己的 LocalPlan 并 Handled、child 向直接父级回执，父收到回执后仅将父项改 Completed 不产生第二个 replan。它是 actor 级直接边链路，不是三层树、真实 coordinator 启动/child completion 竞争、crash/restart ACK 恢复或父项并行 A/B 更新链的故障注入。 |
| 必需结果与 review revision（TP-26～29、BG-04～05） | `required_link_result_stays_blocked_until_current_requirements_are_reviewed` 通过真实 PlanningCommand 先观察必需结果 blocker、精确 revision 的 Accepted review 清除 blocker、要求变化使旧 review 失效，再以新 revision 对同一 result 复核清除 blocker。结果 link 在测试中直接写入 durable Planning fact；canonical child reference 的缺失/外部 child 引用另由 `planning_result_requires_a_canonical_child_reference` 验证拒绝；普通未关联 task 可无收据通过由 `planning_result_requires_expected_owner_but_allows_unlinked_task` 覆盖。尚无 Goal complete command 对所需交付的端到端拒绝/接受，也未覆盖多个 TODO 局部阻塞、弃用执行或三层摘要验收。 |
| tombstone、scope 与控制准入（TP-08、16、19；BG-03、06～09） | `replace_tombstones_omitted_steps_and_never_reuses_ids`、`scope_change_requires_open_work_disposal_but_keeps_terminal_history_valid`、以及状态差异/replan coalesce 系列仍是 `tool-types/src/planning.rs` 的纯计划模型测试；`planning_timeline_preserves_delete_and_replan_identity_transitions` 是 Timeline 投影/身份测试。Goal 侧另有 `goal_cannot_overtake_queued_model_and_agent_controls`、`goal_admission_rechecks_agent_after_an_in_flight_step_control`、`queued_route_control_blocks_goal_continuation_admission` 的 actor 准入测试，但没有将控制/暂停/权限等待与规划 waiter 完成组合成同一竞争矩阵；也没有以 child spawn/启动计数断言 tombstone 或 scope 变更不产生副作用。 |

仍然最关键的 runtime 缺口是注册时序与非成功依赖矩阵、Goal 长等待的 provider/idle 计数、Stop/限额/控制与唤醒竞争、完整 Goal 必需交付 gate、三层父子结果验收，以及客户端/压缩/replay 验收。真实冻结树 PTY 现有 3/3 精确场景通过；它不能代替这些 actor、coordinator 和客户端验收。

## Coordinator freeze race — 2026-10-19

工具库 focused 运行日志 `/tmp/grow-v2-4-0-coordinator-race13.log`：`child_finishing_after_freeze_is_joined_once_before_retirement` **1 passed, 0 failed**。该 coordinator 场景验证 child 在 freeze 后完成时被 join 一次、再 retire 一次；这只关闭对应 race 子场景，不覆盖完整 Task/Planning acceptance 或所有持久化/故障边界。

## Scenario evidence refresh — 2026-10-19

本节按具名场景补充当前可核对的运行证据；`/tmp/grow-v2-4-0-suite-build40.log` 仅证明测试目标编译成功，不是测试运行结果。所有 acceptance 行与 tasks 实施/验收项仍保持未完成。

| 场景/测试 | 最新可核对证据 | 覆盖限制或状态 |
| --- | --- | --- |
| Task/Command 等待结果（TP-20～22） | Shell regression8 中 `task_and_command_waits_apply_any_all_terminal_and_identity_rules`、`planning_wait_rejects_missing_and_foreign_dependency_identities`、`cancelled_and_unavailable_task_outputs_wake_without_success_claim` 均通过。最后一个测试覆盖 Cancelled、CompletedOutputUnavailable 和以 Failed 携带 unknown 提示的 Task，并检查唤醒提示及不写成功结果。 | 依赖 Task 快照来自测试 backend，不是 coordinator/真实 child runner；未证明注册前/注册中/注册后的完成竞争矩阵。该状态矩阵没有专用 `OutcomeUnknown` Task 类型，Interrupted 覆盖的是 Command；不应把三项通过扩成 TP-20～22 整行通过。 |
| 必需结果、复核、Goal 完成（TP-26～29、BG-04～05） | Shell regression8 中 `required_link_result_stays_blocked_until_current_requirements_are_reviewed` 通过；`/tmp/grow-v2-4-0-shell-blockers33.log` 中 `goal_complete_command_enforces_required_delivery_but_ignores_unrelated_todo` 通过，断言未复核时 Goal completion 被拒绝，精确复核后可完成且保留无关 pending TODO。 | Goal 命令测试直接建立 durable Planning result/review facts，没有真实 child result/coordinator；仅覆盖一个 required link 和一个 unrelated TODO，不能证明三层交付、弃用/取消处置、worktree 集成或 BG-06～08。 |
| 人类等待唤醒与真实采样（TP-25、BG-03） | `structured_output_completion_waits_for_live_question_and_permission_without_resampling` 在 `/tmp/grow-v2-4-0-structured-wait-focused2.log` focused 通过；父子变化链 `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned` 在 `/tmp/grow-v2-4-0-replan-focused-final2.log` focused 通过。 | 相同 Shell regression8 中 StructuredOutput wait 与 `question_and_permission_answers_wake_the_real_planning_wait` 因唤醒超时失败，故当前只有 focused 通过、全量并发运行不稳定的证据。Stop/max-turns/FIFO 的两个新 Goal provider 测试在 `/tmp/grow-v2-4-0-final-fixtures41.log` 仍未通过；其失败来自测试 GoalUsageWindow 未绑定当前 Goal。夹具已在 `truncation_recovery_tests.rs::activate_test_goal` 中补同步和身份断言，但该修复尚无运行结果，不能记为通过。长等待 provider/idle 调度计数仍未覆盖。 |
| Task 失败状态唤醒（TP-22） | `cancelled_and_unavailable_task_outputs_wake_without_success_claim` 的定向运行见 `/tmp/grow-v2-4-0-shell-blockers33.log`，1/1 通过；Shell regression8 也列出该测试为通过。 | 真实 `Interrupted` 命令、失败/未知 child 的 canonical 结果和 artifact 缺失均未通过生产 coordinator 路径联合验证；不可用 artifact 目前是 backend snapshot 夹具。 |

当前最小剩余验收缺口：Task/Command 注册竞态与真实 coordinator/child terminal 矩阵；等待唤醒时 Stop/预算/max-turns 同时到达；BG-06～09/TA 权限与控制竞争；真实 worktree 集成；live/headless/replay 和压缩边界。Shell111 已重跑完整套件并通过，故旧日志中的 wait 超时、父回执状态和 fixture 失败均不再列为当前失败。仍不以 suite 总数替代这些未覆盖场景。

## Historical evidence refresh — 2026-10-19, build 81 / focused 82

本节更新前面的历史记录。Shell 全量 `/tmp/grow-v2-4-0-shell-final82.log` 最终为 **4219 passed, 0 failed, 5 ignored**；本轮没有自行运行 Cargo。CLI build 73、library build 81 是编译证据，不代替具名场景证据。

| 验收主题 | 已出现运行证据 | 仍未由该证据证明 |
| --- | --- | --- |
| 等待、终态和身份（TP-20～22） | final82 中 `task_and_command_waits_apply_any_all_terminal_and_identity_rules`、`planning_wait_rejects_missing_and_foreign_dependency_identities`、`cancelled_and_unavailable_task_outputs_wake_without_success_claim`、`question_and_permission_answers_wake_the_real_planning_wait` 均为 `ok`。 | Task 结果仍来自测试快照 backend；没有真实 coordinator/child 结果在 wait 注册前、注册中、注册后到达的 race 矩阵。Command 的 Interrupted 有证据；未知 child 与缺失产物的真实 canonical 结果仍缺。四项通过不等于 TP-20～22 整行验收。 |
| Goal 等待与完成门（TP-25、BG-01、BG-03～05） | final82 中 `goal_dependency_wakeup_respects_turn_limit_before_provider_resampling`、`goal_dependency_wakeup_yields_to_fifo_input_without_resampling`、`budget_limited_goal_wait_does_not_resample_when_dependency_wakes` 均为 `ok`。它们使用一个真实首 provider 请求，等待期间复查请求计数及原 cursor；覆盖 max-turn、FIFO 和预算关闭。`goal_complete_command_enforces_required_delivery_but_ignores_unrelated_todo`、`required_link_result_stays_blocked_until_current_requirements_are_reviewed` 与 `all_final_output_paths_share_blocker_and_turn_limit_decision` 也为 `ok`，覆盖命令 gate、当前 requirements 的复核以及无关 pending TODO 不阻塞。 | provider/cursor/blocked-audit 有计数或稳定断言，但没有 actor idle 重排计数；Stop、Paused、权限/控制与 dependency wake 同时到达的矩阵缺失。Goal 完成 fixture 直接写 durable result/review facts，不覆盖真实三层 child 结果、弃用处置和 worktree 集成。 |
| 子任务等待和结果验收（TP-23～24、26～32、BG-02、06～09、ST-16） | final82 中 `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned`、`structured_output_completion_waits_for_live_question_and_permission_without_resampling` 均为 `ok`；前者覆盖两个 actor 间 direct-child 更新到 Handled/直接父回执，后者覆盖一个真实 provider turn 在 Question/Permission 等待期间不重采样。 | 仍无 child 等待 grandchild 后跨模型 turn 保持原 child owner、或正文“等待”不启动执行的端到端证据；没有三层交付验收和完成摘要从 canonical child result 到 root 的集成链。父子测试也未覆盖 parent update 与 child terminal/取消竞争及 ACK 崩溃恢复。 |
| 树恢复（ST-04、09～10、11、15） | `/tmp/grow-v2-4-0-pty77-results.json` 报 root/tree export、partial release 1–3、concurrent resume 和 baseline 七个进程测试均退出 0；`/tmp/grow-v2-4-0-coordinator-race13.log` 的 `child_finishing_after_freeze_is_joined_once_before_retirement` 为 1/1。 | 这些证明挂起树身份、释放/并发 claim 和一个 coordinator 终态竞争，不证明计划 replan/inbox 在各持久边界 crash 后不重投，也没有在树恢复中组合 Pending、AwaitUser、权限、Goal budget 或 ST-16 验收摘要。 |

截至本节，最小必要未覆盖集合是：

1. **责任链故障边界**：用真实 parent/child/coordinator 验证 spawn 与计划提交竞争、父子消息 ACK/消费的 crash/retry，以及至少一条三层 direct-edge 交付和 canonical result 验收链；现有两 actor replan 测试不代表这些故障路径。
2. **真实等待 owner 矩阵**：覆盖 Task/Command 结果注册前中后到达及真实 child/coordinator 的失败/取消/unknown/产物缺失；再证明同一个 child owner 跨等待继续而不被误判终态。现有状态快照矩阵不能替代 owner 生命周期。
3. **Goal gate 竞争与静默性**：现有 max-turn、预算、FIFO 三个 provider 回归有通过记录；还需最少一个 Stop/Paused 或权限控制与 waiter 同时到达的 gate 检查，以及长时间无依赖变化时 actor idle 重排计数。不要重复所有已覆盖的排列。
4. **产品表面和恢复组合**：live/headless/replay 的等待和 child 状态投影、压缩快照与新责任并发，以及树恢复时待处理 replan/权限/验收状态的组合仍没有相应端到端证据。

final82 全量 Shell 在该历史时点已通过；它关闭当时的 Shell 回归门禁，不代表 acceptance 场景全覆盖。当前 tasks 状态与后续具名证据见文首 reconciliation 和 `tasks.md`。

## Historical acceptance ID evidence audit — 2026-10-19, clean suites 82/32/31

本节是当时逐 ID 对照 [acceptance.md](acceptance.md) 的历史快照，clean suite 基线已由文首当前证据节更新。快照中的逐名测试仍是有效历史证据；“未执行”表示当时该 ID 完整验收未通过，不表示没有组件测试，也不要求所有机制挤进一个巨大 fixture。

缩写测试名均可在上述日志中搜索完整名字。Actor 测试位于 `crates/codegen/shell/src/session/actor/tests/planning_dispatch_tests.rs`，Goal 等待位于 `.../tests/truncation_recovery_tests.rs`，最终输出 gate 位于 `crates/codegen/shell/src/session/actor/turn/mod.rs` 的 `planning_completion_gate_tests`；LocalPlan 纯状态测试位于 `crates/common/tool-types/src/planning.rs`，Timeline 测试位于 `crates/codegen/chat-state/src/timeline.rs`。运行日志中 Shell named actor tests 均为 `ok`；tool-types 与 Timeline 所列测试也均为 `ok`。

### TP-01–TP-19

| ID | 现有真实入口及证据 | 尚未覆盖的验收边界 |
| --- | --- | --- |
| TP-01 | ChatState `subagent_spawn_preserves_local_plan_step_association`；Timeline 保留 spawn 与 LocalPlan step 关联。 | 没有父三项/子三项 owner 隔离并验证父写不能覆盖子计划的 actor 链。 |
| TP-02 | tool-types `child_planning_summary_is_bounded_and_keeps_snapshot_identity`；Timeline `canonical_child_result_retains_the_original_bounded_planning_summary`。 | 没有补充消息/new turn 后 scope 稳定且新旧计划不互盖的 SessionActor 验证。 |
| TP-03 | 当前五份指定日志未发现针对“简单任务无 TODO 不创建 Goal/计划”的具名运行入口。 | 真实普通执行及等待流程中核对没有隐式 Goal/LocalPlan。 |
| TP-04 | ChatState `subagent_parent_and_child_ledgers_close_through_exact_result_ref` 与 `child_result_and_parent_ended_retries_keep_their_canonical_event_once` 覆盖关联结果身份及重试幂等。 | 没有同一父项并行两次执行加一次 retry，并断言各自结果处置互不覆盖。 |
| TP-05 | 现有 Timeline spawn 关联测试和 actor replan 链分层覆盖两端。 | 缺 spawn 与父项 commit 交换顺序的 barrier 测试；需断言新快照或补发至少一种不丢责任。 |
| TP-06 | tool-types `effective_diff_only_replans_for_intent_changes`、`requirements_priority_noncompletion_status_and_remove_are_triggers`；Shell `linked_parent_update_is_dispatched_received_handled_and_completion_is_not_replanned`。 | 改内容/验收/优先级各触发且仅直连活 child 的完整差异组未在同一入口逐类验证。 |
| TP-07 | 同一 Shell 链断言内容更新派发并在仅改 Completed 后不产生第二次 replan。 | 没有独立“只完成状态触发 0 次”与“内容+完成仅 1 次逻辑责任”的成对计数。 |
| TP-08 | tool-types `effective_diff_only_replans_for_intent_changes` 和 `transition_keeps_stale_handled_and_completion_only_is_valid` 覆盖纯状态差异。 | 没有 actor 层 pending/in_progress/cancelled/reopen 全表及重复同值无事件、取消不杀执行的计数断言。 |
| TP-09 | tool-types intent diff；Shell direct-child replan 链；Timeline `planning_timeline_preserves_delete_and_replan_identity_transitions`。 | 排序/UI/usage/review 等无效变更没有汇总到 actor 层并核对零额外模型调用。 |
| TP-10 | 有 required-link gate 和 review tests（见 TP-26–28），但未发现“调查后新增测试要求”对应的精确 fixture。 | 调查项保留、受影响项调整及来源准确性仍需一个实际 LocalPlan 更新验证。 |
| TP-11 | Shell `required_link_result_stays_blocked_until_current_requirements_are_reviewed`；tool-types `replan_identity_cannot_change_payload_and_no_change_needs_a_reason`。 | 无变更且有理由、不得伪修改/下游广播的 actor 路径尚未验证。 |
| TP-12 | Shell `task_and_command_waits_apply_any_all_terminal_and_identity_rules` 启动实际 Terminal 命令并等待；Pager ACP tracker 有 task-output wait 入口。 | 没有在同一真实命令仍运行时改父项并断言不自动取消、可让出、迟到结果仍交付。 |
| TP-13 | Shell `question_and_permission_answers_wake_the_real_planning_wait` 覆盖两种交互唤醒。 | 没有父变更要求 user info 时保持未处理且仅读回执不关闭等待的断言。 |
| TP-14 | tool-types `pending_replans_coalesce_but_received_payloads_remain_immutable`、`old_replan_ack_does_not_clear_newer_pending_revision`；Shell `late_undispatched_ack_is_recorded_once_without_faking_a_result`。 | 消费前三次修改与旧回执的顺序组合没有 actor 集成验证；确认恢复边界仍需 exact source 集合断言。 |
| TP-15 | tool-types immutable received payload/旧 ACK 测试；Shell direct-child 链验证单次更新及 receipt。 | A 被处理时 B 到达、A ACK 重复/乱序且 B 仍待处理的 actor 持久消息竞争未测。 |
| TP-16 | tool-types `replace_tombstones_omitted_steps_and_never_reuses_ids`、`validation_preserves_tombstone_references_and_accepts_incoming_replans`；Timeline 删除/replan identity 测试。 | 省略活跃关联项后在 actor 链复用 ID 并确认身份拒绝/责任保留未测。 |
| TP-17 | tool-types `terminal_child_keeps_unapplied_changes_without_pending_work` 覆盖终态 child 状态转移。 | 编辑与真实 child 完成/取消并发、结果保留且 spawn/start 计数不增加未测。 |
| TP-18 | Shell direct-child update 链；ChatState `subagent_parent_and_child_ledgers_close_through_exact_result_ref`。 | 三层实际 tree 只沿被修改项 direct edge 传播并阻断循环的链未测。 |
| TP-19 | tool-types `scope_change_requires_open_work_disposal_but_keeps_terminal_history_valid` 覆盖纯状态拒绝/处置规则。 | 有真实旧执行/等待 owner 时切 scope、先拒绝再显式处置的 SessionActor 行为未测。 |

### TP-20–TP-32

| ID | 现有真实入口及证据 | 尚未覆盖的验收边界 |
| --- | --- | --- |
| TP-20 | Shell `task_and_command_waits_apply_any_all_terminal_and_identity_rules` 覆盖 Any/All、注册后终态 Task/Command；ChatState `agent_message_consumption_is_exact_atomic_and_one_surface_coordinate` 覆盖消息消费原子性。 | 缺任务结果在 wait 注册前、中、后到达的时序 race；Task 是快照 backend，非真实 coordinator/child owner。 |
| TP-21 | Shell `planning_wait_rejects_missing_and_foreign_dependency_identities` 通过真实 actor wait 命令拒绝缺失/foreign Task 或 Command identity。 | 自等待与祖先环的实际入口及“不留悬空 wait”断言未在指定 clean logs 中找到。 |
| TP-22 | Shell `cancelled_and_unavailable_task_outputs_wake_without_success_claim` 覆盖 Cancelled/OutputUnavailable/unknown提示；`task_and_command_waits_apply_any_all_terminal_and_identity_rules` 覆盖 Interrupted 命令。 | 失败 child、canonical 结果缺失和 artifact 丢失尚未经真实 coordinator terminal result 链验证；不能把快照输入当 owner 故障恢复。 |
| TP-23 | 当前指定日志没有“child 等 grandchild、跨模型 turn 仍由同一 child owner 继续”的具名入口。 | 需要真实 nested waiter + child turn/yield/resume 断言。 |
| TP-24 | Pager 有等待状态呈现测试；它们不创建后台执行。 | 缺正文只写“等待测试”且核对无持续 child/Task 执行及 TODO 呈现的运行入口。 |
| TP-25 | Shell `goal_dependency_wakeup_respects_turn_limit_before_provider_resampling`、`budget_limited_goal_wait_does_not_resample_when_dependency_wakes`、`goal_dependency_wakeup_yields_to_fifo_input_without_resampling` 均通过，分别覆盖 turn limit、预算、FIFO 与 provider 请求/cursor。 | Stop 与唤醒竞争仍缺；以上测试未核对长期无变化时 actor idle 调度计数。 |
| TP-26 | Shell `planning_result_requires_a_canonical_child_reference` 和 `required_link_result_stays_blocked_until_current_requirements_are_reviewed`；Timeline exact result-ref 测试。 | blocker/review fixture 直接提交 durable Planning facts；缺真实 child canonical terminal result 被父接受的集成链。 |
| TP-27 | Shell `required_link_result_stays_blocked_until_current_requirements_are_reviewed` 验证当前 requirements revision 与旧 review 失效后重新 review。 | 当前例验证单 link；没有不同交付要求变化但原结果仍符合的独立保留接受分支。 |
| TP-28 | Shell `goal_complete_command_enforces_required_delivery_but_ignores_unrelated_todo` 与 tool-types `required_delivery_gate_ignores_optional_links_and_needs_review` 分层覆盖必需 gate/无关 TODO。 | 缺“多个局部未勾项并存、一个必需项要修订”的更广实例，但核心组件行为已有测试；不因没有单个大 fixture 将其整体判缺。 |
| TP-29 | tool-types `reasoned_release_closes_gate_before_result_without_ending_child`、`scope_change_requires_open_work_disposal_but_keeps_terminal_history_valid` 覆盖状态处置规则。 | 多执行择一弃用/删除父项时真实 owner 和 waiter 如何处置未由 actor/coordinator 测试。 |
| TP-30 | 指定 clean suites 未发现 worktree 结果尚未集成时阻止目标交付完成的具名验收入口。 | 需要真实目标目录与 worktree 分离状态下验证 gate 不自动 merge。 |
| TP-31 | Shell Goal budget/max-turn wait tests验证 wake 不重置 cursor/limit。 | 没有无 Goal child 跨多次 yield/wakeup 累计同一 execution owner 限额且不另起无限续跑的具名场景。 |
| TP-32 | Shell replan 链有 direct-parent receipt；ChatState 有消息 ACK/重试组件测试。 | 父项取消/移除时命令在途且通知不可达的生产路径竞争和通知恢复后的准入副作用未测。 |

### ST-01–ST-16

| ID | 现有真实入口及证据 | 尚未覆盖的验收边界 |
| --- | --- | --- |
| ST-01 | ChatState `host_completion_result_waits_for_durable_ack_and_reports_failure`、`lost_timeline_ack_retries_the_exact_event_once`。 | 这些是 timeline/host completion 通用 ACK 测试；planning 父项提交后退出并在恢复时补发同源责任未实测。 |
| ST-02 | ChatState `buffered_append_recovers_from_io_failure_without_breaking_sequence`、`lost_timeline_ack_retries_the_exact_event_once`、`source_tool_result_ack_loss_does_not_redeliver_received_message`。 | planning source 写失败与 ACK 丢失的状态/通知顺序尚缺专用注入和 exact event 核对。 |
| ST-03 | ChatState `planning_timeline_preserves_delete_and_replan_identity_transitions`、Timeline 从持久事件派生投影；Pager replay frontier 测试验证 committed branch 边界。 | 未见篡改 Planning Resources/UI cache 后实际 Timeline 重建并断言不造新 change 的运行测试。 |
| ST-04 | 既有 PTY77 树冻结/恢复证据记录在上方历史验证段；本次四份 clean suite 另有 `source_tool_result_ack_loss_does_not_redeliver_received_message`。 | 严格按 acceptance 的 Received/Consumed/Handled 各合法冻结点和 Consumed 后继续工作投影，仍需与规划消息逐边核对；普通退出/冷崩溃不算冻结恢复。 |
| ST-05 | ChatState `parent_receipt_history_retains_consumed_payload_without_model_input`、`source_tool_result_ack_loss_does_not_redeliver_received_message`。 | 新责任与旧确认交错时同一 planning child fact 已提交、父 ACK 丢失、父重启后 exact 原确认的完整链缺失。 |
| ST-06 | ChatState `subagent_parent_and_child_ledgers_close_through_exact_result_ref`、`child_result_and_parent_ended_retries_keep_their_canonical_event_once`。 | 已证明两端账本 exact ref 组件；缺 root-child-grandchild Workflow 真实路径和模型结果仅归直接父的端到端核验。 |
| ST-07 | ChatState actor ACK tests与 Shell `goal_waiter_cannot_outrun_receipt_admission_or_emit_it_twice`（Shell82 named `goal_waiter...` 为 `ok`）覆盖 waiter/receipt 的局部互斥。 | waiter 完成、steering、通知 ACK retry 的同一个 call tool_result 与一次逻辑消费尚未一体核验。 |
| ST-08 | ChatState 通用 durable completion ACK 失败测试；canonical result Timeline tests。 | artifact 缺失、link conflict、parent acceptance failure 三类下真实 terminal state/commit/availability/delivery 分离未测。 |
| ST-09 | PTY77 有树恢复与并发 claim 的运行记录（见前文）。 | 三层树同时含长命令、未处理 replan、待验收结果时退出/resume 并证明命令不重放的组合未有具名结果。 |
| ST-10 | PTY77 有树身份/恢复结果；Shell Goal/FIFO 和 wait tests分别覆盖控制与唤醒组件。 | child suspend 后父变更、resume 后同时有 paused Goal/awaitUser/permission/budget 限制的责任与准入组合缺失。 |
| ST-11 | ChatState `admitted_response_view_survives_repeated_rewind_and_cold_fold`、`admitted_response_view_tracks_branch_ownership_across_repairs_compaction_and_rewind`。 | 覆盖的是 conversation admission/rewind 投影；没有 planning checkpoint 缺失时 crash/fork/rewind/resume/export/replay 不启动旧计划的专项链。 |
| ST-12 | Shell `cleanup_ttl_preserves_runtime_checkpoints_and_local_planning_obligations`。 | 清理保留待处理/未验收引用已有责任；义务收敛后最终释放不永久占用的配对结果需确认。 |
| ST-13 | ChatState 有有界 inbox/timeline 组件 tests，但指定日志未发现 Planning responsibility capacity saturation 的精准入口。 | inbox/存储容量饱和时需证明已提交责任保留、未提交不报成功及有界反压。 |
| ST-14 | ChatState `agent_message_consumption_is_exact_atomic_and_one_surface_coordinate`、`parent_receipt_history_retains_consumed_payload_without_model_input`；tool-types immutable payload/replan tests。 | A Received 后 B 在消费边界前/后到达并核对 A 不变及确认只覆盖已消费 source 的跨 actor 场景未测。 |
| ST-15 | ChatState `agent_message_consumption_is_exact_atomic_and_one_surface_coordinate`；`child_result_and_parent_ended_retries_keep_their_canonical_event_once`。 | Consumed 未 Handled 后正常终态、确认冷崩溃、旧实例状态未知三条分支尚未分别注入，尤其不可复活/不可造终态。 |
| ST-16 | tool-types `child_planning_summary_is_bounded_and_keeps_snapshot_identity`；ChatState `canonical_child_result_retains_the_original_bounded_planning_summary` 和 exact result-ref 测试。 | 有界摘要和 canonical reuse 已分层覆盖；缺三层验收/提前结束到 root 的最终交付集成，且 root 不重复模型回执。 |

### BG-01–BG-09 and TA-01–TA-04

| ID | 现有真实入口及证据 | 尚未覆盖的验收边界 |
| --- | --- | --- |
| BG-01 | Shell `every_continuation_audits_the_full_goal_before_planning_a_local_slice`、预算/max-turn wake tests；Goal completion gate shared final paths 测试。 | provider/cursor 有请求计数；长期静默等待的 actor idle requeue 数仍未被计数，不能由 waiter 结构推断无忙循环。 |
| BG-02 | `every_continuation_audits_the_full_goal_before_planning_a_local_slice` 证明 continuation 执行完整 goal audit 后再规划局部 slice。 | 一个 step 等 child、另一个独立 step 实际推进，以及无计划时 Goal audit 可用，缺 execution 级例子。 |
| BG-03 | Shell `pending_fifo_yields_goal_wait_before_dependency_is_ready`、`question_and_permission_answers_wake_the_real_planning_wait` 与 FIFO/budget provider tests。 | 已队列输入优先级和两类人类唤醒分层覆盖；Stop/control/permission 与 dependency result 同时竞争的完整优先级边界仍缺。 |
| BG-04 | Shell `goal_complete_command_enforces_required_delivery_but_ignores_unrelated_todo`、`all_final_output_paths_share_blocker_and_turn_limit_decision`。 | Goal completion 命令使用预置 durable Planning facts；缺真实 required child execution/result 触发具体 blocker 并经真实复核后完成。 |
| BG-05 | Goal completion test 证明 unrelated pending TODO 不机械阻塞，final output gate 共用 blocker/turn-limit 决策。 | 缺目标都达成、必须义务闭合且 child 无关探索项未勾的真实原执行收尾路径。 |
| BG-06 | tool-types `effective_diff_only_replans_for_intent_changes` 与 Timeline planning identity tests。 | 缺 actor 层断言 TODO 修改保留 Goal id/revision/预算，同时真正 Goal revision 变化隔离旧结果唤醒。 |
| BG-07 | Pager `plan_mode_keeps_current_behavior_and_all_phases_without_auto_approval` 通过，证明展示层不自动审批。 | 没有已审批 Plan 的合法细化与越界输入进入实际 Goal/Plan authority gate 的运行测试。 |
| BG-08 | ChatState exact parent-child ledger/result-ref tests，tool-types bounded summary tests。 | root→普通 Task child→grandchild 的逐级验收与权限限制同链尚未覆盖。 |
| BG-09 | Shell 有暂停/用户/权限等待组件测试，但 clean named set 未发现变化和结果到达时“持久保存、采样/批准计数为零”的联合入口。 | 需要一个被暂停/预算关闭/真实等待 gate 拦截的 provider/approval counter 场景。 |
| TA-01 | Shell `planning_result_requires_expected_owner_but_allows_unlinked_task`、`planning_wait_rejects_missing_and_foreign_dependency_identities`；Timeline identity validators。 | 这些测 canonical owner/missing/foreign；伪造 sibling 或历史 step 引用后的调用拒绝且零状态副作用未全覆盖。 |
| TA-02 | Shell `planning_result_requires_a_canonical_child_reference`；ChatState exact atomic consumption tests。 | 没有针对“未消费/其他 child 确认”及缺结果引用一并断言拒绝伪确认/伪验收的入口。 |
| TA-03 | Pager 有 permission UI 状态测试；现有 Workflow/permission gates 属于组件覆盖。 | 父项变化后 read-only child 尝试写文件/越权命令并验证走原 gate、没有新增授权，未测。 |
| TA-04 | Shell direct-parent replan 链；ChatState received-message ACK；Pager ordinary surfaces。 | 普通 Send 到终态/Workflow child 与内部变化到挂起 child 的两种准入/投递状态组合无统一入口。 |

### CS-01–CS-04 and CC-01–CC-04

| ID | 现有真实入口及证据 | 尚未覆盖的验收边界 |
| --- | --- | --- |
| CS-01 | Pager `views::turn_status::tests::parked_wait_preserves_live_control_feedback_until_settled`、`recorded_parked_wait_omits_input_and_execution_actions`、`parked_without_watchers_renders_waiting_cue`；headless `messages_tool_use_grouped_then_user_results`；`lifecycle_tracking_is_independent_of_wait_flag`。 | Pager waiting cue 和 headless 未完成工具调用有分层入口；同一仍等待 owner 的 live/headless/resume 三表面 lifecycle 一致性仍缺。 |
| CS-02 | Pager ACP tracker `task_output_wait_captures_task_ids_from_raw_input_update`、`task_output_waits_tracks_timeout_ms`；Pager `task_output_wait_resolves_subagent_by_subagent_id`；headless 未完成工具结果测试。 | 当前入口能保存 Grow task identity 并渲染/等待；ACP 缺等待枚举时 conservative `in_progress` 与身份原样保留还未由一条明确协议夹具验证。 |
| CS-03 | Pager `app::acp_handler::tests::session_routing::plan_update_for_inactive_agent_lands_in_its_todo`（对非活动 session 投递真实 ACP Plan 更新）和 `session_events::async_compaction_notification_preserves_latest_todos` 均在 Pager31 为 `ok`。 | 计划确实路由到指定 child/session 且异步压缩通知保留压缩期间最新 TODO；还没有一个 fixture 同时带 active child、terminal child、未应用 replan 和待验收结果的完整任务列表。 |
| CS-04 | Pager `export_cmd::tests::tree_export_keeps_parent_and_child_conversations_separate`、`replay_cmd::tree::tests::ordinary_task_surfaces_follow_delivery_and_open_children_without_execution_controls`。 | 现有测试分层覆盖树分离与 replay 无执行控制；未发现同名 step、嵌套 child、零 provider/tool/通知消费的同一输入端到端计数，但这不是整项空白。 |
| CC-01 | Shell `local_plan_reminder_preserves_open_obligation_references_without_copying_payloads`；新增 `context_recall_tests::compaction_reminder_uses_current_timeline_plan_references` 从 ChatState Timeline 实际读取当前 plan，再经 compaction reminder formatter 保留 replan/wait/link/child 引用并省略 payload。后者待 root 集中构建运行。 | 没有大完成历史+单必需等待的 compaction 后实际模型输入 token/引用计数验证。 |
| CC-02 | ChatState conversation fork/summary 基础测试与本 change bounded projection 测试分层覆盖。 | 没有 fork/summary 含父 TODO 时断言 child 当前投影和权限仍独立的 change-specific 入口。 |
| CC-03 | Pager31 `async_compaction_notification_preserves_latest_todos` 验证压缩期间新 Plan 更新胜过旧值；Shell 新增 compaction 当前 Timeline snapshot 到 reminder 的路径测试（待 root 集中构建运行）。 | ChatState `snapshot_combines_timeline_projection_and_runtime_metadata` 是一般快照投影；压缩生成已开始后父更新/结果并发到达，最终 replacement 仍保留新责任的 Timeline 发布竞争未测。 |
| CC-04 | ChatState `agent_message_consumption_is_exact_atomic_and_one_surface_coordinate`、`parent_receipt_history_retains_consumed_payload_without_model_input`。 | compaction 同时包含 Consumed 未 Handled 消息的恢复责任保持及不再次消费未在同一压缩恢复入口核验。 |

### 最小待补门禁

现有组件测试已经覆盖 LocalPlan diff/tombstone/replan immutable 与 coalesce、canonical result/ref、required-delivery/review、Any/All waiter 的大部分状态规则、Goal FIFO/budget/max-turn provider 请求，以及 Timeline 消息 exact 消费和部分 ACK retry。无需为这些规则再造一个全组合 fixture。剩余应收敛为以下定向门禁：

1. **planning responsibility 的真实持久化失败链**：在 `planning_dispatch_tests.rs` 现有 parent/child actor 链基础上，用真实 coordinator/Timeline persistence 注入父 source commit 前失败、commit 后 ACK 丢失、child Received/Consumed/Handled 各边界退出；断言重启后的 exact source、通知次数及 child owner 不变。对应 TP-05、TP-14/15、ST-01/02/05/14/15、TP-32 中尚缺的故障/竞争断言。通用 ChatState ACK tests 已证明存储机制可重试，不替代 Planning source 责任的故障注入。
2. **wait owner 竞态和 terminal 真实性**：从 `planning_dispatch_tests.rs` 的 waiter 扩展为实际 task coordinator/child owner 在 wait 注册前、中、后完成；只需命中一次失败/取消/unknown 和 unavailable artifact 分支并检查不冒报成功、owner 不消失。再用一次 child 等 grandchild 跨 yield/resume 核验 owner 连续性。对应 TP-20–24、TP-31。
3. **一个三层 direct-edge 交付实例**：复用 `subagent_parent_and_child_ledgers_close_through_exact_result_ref` 与 bounded summary 事实，运行 root→child→grandchild，完成 grandchild 结果后只由直接父处理，父/子验收局部闭合后 root 收到 canonical 摘要；补一个 child 提前结束/待验收分支即可。不需把 TP-01–19 所有输入塞进该实例。对应 TP-01/04/18、TP-26/29–30、ST-06/08/16、BG-08、TA-02。
4. **Goal authority 的一个相邻 gate 竞争**：已有 FIFO、budget、max-turn 和 review/complete gate 证据。只需注入 Stop 或 Paused/permission 与 waiter 完成同时到达，断言 provider/approval 为零，并对静默等待采一次 actor idle requeue 数。另以已审批 Plan 实际细化/越界请求各验证一次 gate。对应 TP-25、BG-01/03/06/07/09、TA-03。
5. **客户端跨表面生命周期和压缩发布竞争**：Pager waiting cue、headless 未完成工具结果、child Plan 路由及压缩期间最新 TODO 已有分别运行入口；新增 Timeline→compaction reminder 当前引用测试待 root 集中构建。仍需一个相同 owner 的 live/headless/resume lifecycle 对照，以及压缩 replacement 与较新 Planning/Consumed 未 Handled 责任并发的发布断言。对应 CS-01–04、CC-02–04、ST-03/04/09–11。

## Planning ACK cold-prefix regressions — added, not run

Added two real actor-handler tests in `crates/codegen/shell/src/session/actor/tests/planning_dispatch_tests.rs`:

- `received_replan_ack_loss_cold_retry_reuses_receipt_and_consumed_input` drives a parent source edit through `receive_plan_guidance`, commits the child `Received` Planning fact while dropping its ACK, reconstructs the child ChatState actor from that durable Timeline prefix, then retries the same delivery. It checks one source responsibility and receipt identity, consumes the message once, and confirms cold replay leaves it in model history without reopening the inbox.
- `handled_receipt_ack_loss_cold_retry_preserves_new_replan` builds an older consumed source and a newer pending source in the same parent plan, accepts the older child receipt through `receive_replan_handled`, commits Handled while dropping its ACK, reconstructs from the persisted prefix, and retries the same receipt. It checks the older fact is not duplicated and the newer pending responsibility remains.

The local `PlanningAckPersistence` seam stores the immutable event before dropping the selected ACK and cancelling the old ChatState writer. Both tests passed 1/1 in `/tmp/grow-v2-4-0-focus108-results.json`; they are runtime evidence for selected ACK-loss/cold-prefix boundaries, not an OS process kill or a full SessionActor/coordinator rebuild.

The three-layer boundary passed 1/1 as `nested_planning_result_and_summary_follow_direct_parent_edges` in `/tmp/grow-v2-4-0-focus109-results.json`, including canonical direct-parent results, Accepted review, and bounded summary. Broader freeze/recovery and ACK-failure combinations remain separate gaps.

本段历史清单描述当时尚未证明的边界；当前状态见文首 reconciliation。完整 acceptance ID 未整行标通过，但部分场景组件已有具名运行证据；剩余门禁限于明确列出的 race、owner、ACK 与产品路径边界。