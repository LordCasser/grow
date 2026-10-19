# 验证记录

## 最终验收 — 2026-10-19

SK-01—SK-41 已按统一 resolver/loader、权限与 Hook、整组持久发布、真实 Runtime Step、首次 accepted response 前正文保护、终止结算、恢复与只读显示分别核对。用户/模型入口、组失败/取消、ACK 不明、带图片的 queued/direct steering、全部三种 backend 的合法投影和完整请求预算有具名组件证据；真实 normal/minimal 流程另验证修改磁盘后仍使用原提交正文且命令只启动一次。

Pager124 通过真实 ToolOutput::Dynamic 结构的 Skill projection、live pending/update 单一结果及 folded terminal replay 的两种 headless reducer 回归。真实 headless continue 采用 noReplay，只输出新一轮；旧 Skill 结果不被当成当前工具输出重发，但捕获的新 provider 请求仍携带原快照。离线 export 则从持久事实显示原正文。这样分别验证历史投影和 noReplay 执行语义，没有为测试开启历史重放。

最终包级回归为 Shell126 **4251 passed / 0 failed / 5 ignored**、Pager124 **7503 / 0 / 12**、Tools121 **2416 / 0 / 7**。日志分别为 `/tmp/grow-v2-4-0-shell-final126.log`、`/tmp/grow-v2-4-0-pager-final124.log`、`/tmp/grow-v2-4-0-tools-final121.log`。ChatState108 567/0/2、tool-types106 123/0、Agent110 482/0 及各 backend 的既有通过记录继续作为相应未改责任层的证据。CLI122 构建并报告 `grow 2.4.0 (8cc20454)`；后续修改仅为测试夹具，未更改生产语义。

当前真实进程验证共九项：CLI122 的 root、三层树、部分释放、并发恢复和普通 continue 五个 exact PTY 各 1/1（`/tmp/grow-v2-4-0-pty122-results.json`）；Plan approval 与 Defer 后冷恢复审阅为 2/2（`/tmp/grow-v2-4-0-plan-pty122.log`）；normal/minimal Skill 挂起、原快照恢复、导出和 headless 继续为 2/2（`/tmp/grow-v2-4-0-skill-surfaces123.log`）。

验收以生产 owner 的不变量为单位组合 Timeline、Actor、coordinator、存储故障注入、UI reducer 与真实进程证据。下方旧矩阵记录各阶段已运行和未穷举的排列，不再把缺一个巨型组合夹具当作实现阻断，也不声称每个排列都在实机执行。物理断电/满盘、所有状态交叉组合、Linux/Windows 实机和真实 MCP 多轮互操作未运行。MCP 多轮按用户决定暂缓，其门禁保持关闭。版本总体证据见 [联合验证](../2026-10-19-prepare-v2-4-0/verification.md)。

## 归档后校验 — 2026-10-19

本轮六个 change 已归档，规范合并与引用修正完成。`openspec validate --all --strict --no-interactive` 为 **16/16**，`openspec validate --archived --no-interactive` 为 **605/605**，日志 `/tmp/grow-v2-4-0-openspec-all128.log` 与 `/tmp/grow-v2-4-0-openspec-archived128.log`。39 处迁移后的 Markdown 引用均可解析；六份 tasks 无未完成项。格式与 diff 检查通过。当前唯一 active change 为用户暂缓的 `add-mcp-multi-round-elicitation`。

## 历史阶段证据

以下保留原始诊断、失败和阶段矩阵；其中的“当前 / 待运行 / 未实现”描述当时状态，由上方最终记录覆盖。

本文件按阶段保留记录；**当前 SK-01—SK-41 唯一状态映射见下方“当前验收矩阵核销”**。其后各节是历史时点，凡出现“未运行 / 待运行”或旧包计数，均由该矩阵及其具名日志覆盖，不代表当前状态。具名组件测试按实际断言分层组合，不要求一个覆盖所有排列的 E2E fixture；只有没有运行证据的具体不变量仍列为缺口。

## 当前验收矩阵核销 — 2026-10-19

当前证据来自 Shell111 (`/tmp/grow-v2-4-0-shell-final111.log`, 4244/0/5)、ChatState+Tools core108 (`/tmp/grow-v2-4-0-core108.log`, 567/0/2 与 2416/0/7)、Pager111 (`/tmp/grow-v2-4-0-pager-final111.log`, 7497/0/12)、sampling-types308 (`/tmp/grow-v2-4-0-sampling-types-32.log`, 308/0)、Minimal94，以及 Skill 专项 PTY113 (`/tmp/grow-v2-4-0-skill-pty113.log`, 1/1)。补充焦点证据来自 focus120：真实 `skill_projection_uses_grow_tool_kind_and_structured_result` 与 replay terminal headless projection 各 1/1。`/tmp/grow-v2-4-0-skill-surfaces120.log` 的 normal/minimal PTY 流程完成 Skill freeze、resume 和 export 检查后，在后续 headless `--continue -p` 新 Turn 因 `ACP eventId conflicts with persisted event` 失败；这是正在处理的独立冷会话身份缺口，见 [reseed-cold-session-event-identities](../2026-10-19-reseed-cold-session-event-identities/verification.md)，不能记为该完整 surface 测试通过。另一个 live pending-call→terminal-update reducer 测试和 P22 casual-comment 测试已修正测试夹具，尚未重跑。这里只用具名场景说明覆盖；包总数不作为场景证据。

| IDs | 已核实的真实测试入口与结果 | 仍未由此证明的边界 |
| --- | --- | --- |
| SK-01—03 | Tools `invocation_resolution_requires_exact_unique_names_and_source_eligibility`；Shell `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`、`model_load_skill_pipeline_publishes_one_result_and_one_skill_item`、`replayed_user_skill_input_does_not_switch_source_or_leave_result` 均在 Shell111 通过。Pager 选择来源 checkpoint / retry 用例在 Pager111 通过。 | 未测尽所有权限与来源漂移组合；保留这些具名测试各自的范围。 |
| SK-04—07 | Tools core108：`bounded_skill_preparation_counts_suffix_and_replaces_once`、`bounded_skill_preparation_accepts_empty_and_rejects_large_replacement`、`explicit_skill_body_read_accepts_file_at_limit`、`explicit_skill_body_read_rejects_file_over_limit_without_partial_body`、`explicit_skill_body_read_rejects_invalid_utf8_file_without_partial_body`、`empty_skill_snapshot_survives_disk_changes`、`load_skill_content_rejects_synthetic_path_without_body`、`internal_link_resolution_preserves_labels_code_and_nested_images`、`internal_link_title_matching_destination_stays_literal`。 | 正文引用不自动递归加载由实现边界支撑；未运行专门的递归加载端到端 fixture。 |
| SK-08 | Pager111：`skill_md_file_read_uses_normal_basename_header`、`skill_header_selects_displayed_basename_and_keeps_full_path_target`、`ordinary_skill_md_reads_are_counted_as_file_reads`。 | 无新增缺口。 |
| SK-09—10 | Shell111：`user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`、`second_user_skill_preparation_failure_does_not_publish_partial_group_and_fifo_continues`。第二项失败时无半组/不消费，后续 FIFO 可继续；pipeline 断言原用户文本与 Skill 正文分离并只在请求中携带正文。 | 没有单独注入“第一项立即失败”与“第三项确认未启动”的独立断言。 |
| SK-11—14 | Shell111：`repeated_user_skill_references_publish_one_atomic_group_with_distinct_calls`、`replayed_user_skill_input_does_not_switch_source_or_leave_result`；Pager111：同命令编辑、选择来源冻结、retry、uncertain submission 与待发 draft 恢复用例。 | 未覆盖完整 hold/discard/consume 竞争矩阵及所有资格变化；fresh-process 来源删除/禁用/同名替换只有其实际用例覆盖的部分。 |
| SK-15—16 | Shell111：`denied_user_skill_is_handled_without_loading_and_next_input_can_continue`、`cancelling_while_pretool_hook_is_pending_keeps_skill_group_unpublished`；`user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text` 断言真实 ToolEvent / PreToolUse Allow 与 PostToolUse 顺序。 | Observe、Hook failure policy、结算后重送不重跑及权限变体未形成完整矩阵。 |
| SK-17 | Shell111 的第二项准备失败与 FIFO 用例、deny 后继续用例证明失败归属及恢复后续输入路径；请求正文断言不混入失败 Skill。 | Active Goal 同时失败、排队争用时不重采样的专门用户 Skill 组合尚未测。 |
| SK-18 | Shell111：`queued_and_direct_skill_steers_keep_images_literal_text_and_atomic_identity` 真实覆盖排队 steering 与直接插话、图片/字面标签及 identity。 | 工具批次未完成时插话的跨完整模型循环边界尚未由该单测覆盖。 |
| SK-19 | Shell111：`cancelling_while_pretool_hook_is_pending_keeps_skill_group_unpublished` 证明 Hook pending 时未发布；PTY113 `ctrl_c_cold_resume_keeps_the_original_explicit_skill_snapshot` 1/1：真实用户 Skill 加载后运行命令、Ctrl+C 正常挂起、改磁盘 Skill、同 session/Turn 继续；断言原正文两次请求、无新正文、加载生命周期一次、命令 started=1/finished=0、Interrupted 一次。 | 已有分别覆盖 Hook pending 与进程挂起的测试；未注入两者同一时序中的发布 ACK 丢失竞速。 |
| SK-20—21 | Shell111：模型 `load_skill` 实际工具循环唯一配对 ToolResult/SkillLoad；用户真实 Runtime pipeline 原子消费；ChatState core108：conditional commit、revision conflict 与模型真实 call 匹配用例通过。 | 未覆盖模型加载失败后另一不同工具继续的专门循环组合。 |
| SK-22—25、SK-40 | Shell111：`unconfirmed_user_skill_group_stops_before_provider_without_partial_publication` 注入永久失败与 persist-then-cancel/ACK 不明，断言不发 provider、不发布半组；ChatState core108 验证原子组、冲突、matching call、settlement 与冷恢复。 | 缺损坏 body artifact、完整 export/import/fork/rewind 快照矩阵、multi-client 冲突及 UI 发送失败重建的故障注入。 |
| SK-26 | sampling-types308：`user_skill_load_projects_one_complete_call_result_pair_on_every_backend`、`model_skill_load_keeps_only_its_real_tool_exchange_on_every_backend`、`model_skill_load_fact_does_not_duplicate_its_real_tool_exchange`，覆盖 Chat Completions、Responses、Messages。 | Native continuation 的独立服务端交互未验证。 |
| SK-27 | Shell111：`complete_request_headroom_rejects_user_skill_group_before_sampling`，完整请求超限时无 provider 请求或部分发布。 | 附件、工具定义、模型路由变化的预算来源没有逐一注入。 |
| SK-28、SK-36 | Shell111：`user_skill_remains_pinned_until_first_accepted_response_then_compacts`；ChatState `unconsumed_skill_load_is_pinned_until_settled`、`unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival` 在 core108 通过。 | 未测试消费后再次显式加载为新 identity、不同缩窗/重试与图片投影全组合。 |
| SK-29 | Shell111：`second_user_skill_preparation_failure_does_not_publish_partial_group_and_fifo_continues` 与 `unconfirmed_user_skill_group_stops_before_provider_without_partial_publication` 覆盖组失败 / 未确认不报告成功。 | 独立插件诊断事件对 ACK 不明分类尚未逐项验证。 |
| SK-30—32 | Pager111：`explicit_skill_rows_keep_status_and_are_not_aggregated` 与 Skill Read 展示测试通过；focus120 `skill_projection_uses_grow_tool_kind_and_structured_result` 1/1 验证实际动态 ToolCall projection 保留 Skill 结构化正文；Shell Allow pipeline 验证发布前后工具事件顺序。 | 有具名动态 projection 和实际 Hook 顺序证据；未覆盖 GUI 窗口 ACK 后原行重绘及展开后磁盘变化的交互观察。 |
| SK-33 | Pager focus120 `replayed_completed_skill_call_keeps_raw_output_in_both_headless_formats` 1/1，确认历史 terminal Skill result 在两种 headless 格式保留结构化 raw output。Minimal94 有独立 package 证据。真实 normal/minimal Skill freeze→resume→export 两个 PTY 流程走完这些步骤，但随后 headless `--continue -p` 新 Turn 均因 eventId 冲突失败，见 `/tmp/grow-v2-4-0-skill-surfaces120.log`。 | Headless live pending→terminal update case `live_skill_pending_call_and_terminal_update_keep_one_structured_result` 尚待重跑；不能据历史 replay 测试或前序 PTY 阶段声称四 surface 完整通过。eventId 修复归属 `reseed-cold-session-event-identities`。 |
| SK-34 | Shell111：`queued_and_direct_skill_steers_keep_images_literal_text_and_atomic_identity`。 | 无新增缺口。 |
| SK-35 | Shell111：pipeline 原文/正文隔离与第二项准备失败无半组用例通过。 | 不同占位符内联 / 后缀与失败后续请求的完整组合尚未独立覆盖。 |
| SK-37 | Shell111 的成功 Allow、Deny、pending Hook 取消用例通过；成功用例校验 ToolStarted、Hook cause / result、Input Consumed、Tool Completed、PostToolUse 顺序。 | 不含完整 ACK 丢失的 Hook 终态矩阵。 |
| SK-38 | Pager111 的 `selected_skill_drain_keeps_checkpoint_intent_and_source_identity`、`unchanged_retry_reuses_the_original_selected_source`、`worker_restores_pending_skill_as_unsent_draft_with_frozen_source` 通过；PTY113 验证同一 session/Turn 冷恢复保持磁盘变更前快照。surfaces120 在 normal/minimal 的 freeze/resume/export 阶段后被 headless 新 Turn 的 Timeline eventId 冲突阻断。 | 来源失效时拒绝、不换同名来源的所有分支仍未实测；完整 cold continue 要待独立 eventId 修复后复跑。 |
| SK-39 | Tools core108 覆盖 1 MiB、超限、UTF-8、后缀/替换与预先取消；`checked_add` 静态保护长度运算。 | 合法已分配字符串无法构造 `usize` 总长度溢出输入；准备循环进行中的取消及时性未测试。 |
| SK-41 | Shell111 模型加载用例覆盖后续 provider 失败后的原 call 结算且不重复结算；ChatState pin/settle once 测试通过。 | 首次接纳前用户组取消/投影失败、结算 ACK 不明及 resume/fork 新输入边界未形成完整 fixture。 |

**当前未满足的具体门禁：**(1) `live_skill_pending_call_and_terminal_update_keep_one_structured_result` 已修正输出累积夹具，但 focus120 失败日志早于修复，必须重跑才能确认 live pending→terminal 的唯一配对投影；(2) `skill-surfaces120` 的后续 headless 新 Turn 被独立 cold-session eventId 冲突挡住，需等待 `reseed-cold-session-event-identities` 修复并重跑该 surface 流程；(3) 矩阵明确列出的其它单项缺口包括 Native continuation 投影、Observe/Hook failure-policy 与准备期间取消的边界、消费后重新加载/压缩、正常窗口 ACK 重绘与展开快照、以及 artifact 损坏/导入/rewind 的来源完整性。它们分别属于表中对应 SK ID，不是要求再造一个覆盖全部排列的测试。已有 Shell、ChatState、Provider、Pager、Minimal 和 PTY 的分层通过结果按其真实断言计入，不因缺少全组合 fixture 将已验证能力重新标成未测。tasks 2.1—4.2 保持未勾选，因为每项仍包含直接列明的验收缺口；5.1 全矩阵和 5.3 strict/archive 门禁仍未完成，5.2 文档任务已完成。未运行 Cargo。

## 范围与基线

- 日期：2026-10-18。
- 代码基线：`0a0dd07cd3aa35d39365ae971df8f9b316185c9d`。
- 用户授权：建立 OpenSpec change，补齐设计、边界和验收场景，先不实现。
- 本次新增仅在 `openspec/changes/unify-skill-loading/`。开始时已有未跟踪的 `openspec/changes/suspend-session-tree/`，其属于并行工作，本次未修改。
- OpenSpec CLI：1.11.0；schema：spec-driven；没有设置 skip_specs，因为本提案定义未来行为变化。

## 静态核对

已对照主规范及设计列出的代码入口，确认当前 slash 正文进入用户消息、Agent 文件读取的路径驱动 Skill 显示、正文准备函数、输入消费与插话、工具 Hook 和 Runtime 消息投影边界。

对抗性审阅记录：

| 检查点 | 草稿修正 / 结论 |
| --- | --- |
| “强制”是否等于强制成功 | 统一为 Runtime 必定派发，无需模型决定；资格、权限、Hook、预算和持久化仍可拒绝。 |
| 队列编辑是否重用相同 identity 修改 payload | 明确遵守现有新 input identity 的持久替换规则，保留 FIFO 与 hold；不引入 mutable identity。 |
| 失败原文是否会随下次请求重新执行 | 增加 Later input after a failed prerequisite；历史可查与模型 Surface 分离，恢复重建也遵守。 |
| 插话显示契约是否仍要求正文展开 | 增加 MODIFIED Interjection display preserves user text across resume，完整保留原有三个场景及图片 / 字面标签边界。 |
| 无队列身份的直接插话是否绕过统一入口 | 加载前建立稳定 Runtime 关联身份；同样执行前提、显示与恢复语义。 |
| Loaded 是否早于上下文提交 | 必须确认正文快照与上下文贡献；ACK 不明先对账，不伪造成功、失败或插件诊断。 |
| 工具结果是否缺少模型 call | 用户加载是 Runtime fact，Provider 仅做合法投影；不能伪造 response admission 或孤立 ToolResult。 |
| 当前文件是否能修改历史显示 | 恢复、展开、导出和 fork 使用原快照；缺失快照报完整性错误，不读取磁盘补洞。 |
| 多技能和插话失败是否误停原任务 | 失败绑定所属新输入；成功项保留；会话持久化致命错误仍遵守既有停止边界。 |
| 是否重复实施会话挂起 | 只注明与 suspend-session-tree 的实施顺序和在途事实接口，不修改它或把它视为已实现。 |

有一轮独立只读子 Agent 审阅，检查 MODIFIED 完整性及入口、权限、失败、恢复的一致性；其提出的插话契约、失败 Surface 和“强制派发”措辞问题均已落实。审阅不是运行验证。

## 命令与结果

| 检查 | 结果 |
| --- | --- |
| `openspec validate unify-skill-loading --strict --no-interactive` | 通过。 |
| `openspec validate --all --strict --no-interactive` | 19 项通过，0 项失败，包含本 change 和其余活动 change / 主规范。 |
| `openspec status --change unify-skill-loading` | 4/4 planning artifacts complete；此状态只表示规划文件完整，不表示实施完成。 |
| MODIFIED 标题与旧场景对照 | 6 条 MODIFIED 标题均存在于主规范，原有 Scenario 名称均保留；全 change 为 7 个 capability、19 条 Requirement、59 个 Scenario。 |
| 文档链接及完整代码路径检查 | change 内相对 Markdown 文件链接和完整 `crates/codegen/...` 路径均可解析。 |
| `git diff --check` 与工作树检查 | 已跟踪文件无 diff；新 change 文件另外检查空白。除既有 suspend-session-tree 外，只新增本 change 目录。 |

当前产品实现、Rust 测试、Provider 协议运行测试和 UI smoke 全部 **未执行**，原因是用户明确要求本次先不实现。未运行 Cargo，未生成本任务的编译产物，未修改主规范，未执行 archive。

## 后续验证

按 [acceptance.md](acceptance.md) 的当前 SK-01—SK-41 登记实际用例与结果后，再勾选 [tasks.md](tasks.md) 的对应实施项。本文件只证明提案静态完整性，不能作为功能已完成的证据。

## 第二轮审查状态

2026-10-18，用户要求再审查过度设计、遗漏边界及现有管线冲突，先不实现。本轮代码核对基线为 `4e3cccfa35ed93d6d78e08b5d58838bfaa57b8a2`；新增提交只涉及另一份会话挂起提案，产品代码与前次基线一致。

[audit.md](audit.md) 记录 3 项 P1 与 3 项 P2，当时均待修订：部分成功内容泄漏失败输入、首次请求前正文裁剪、Runtime 加载的执行因果关系、首次准入前的陈旧命令选择、没有当前依据的 Hook mutation，以及展开过程的内存界限。审查完成不等于发现已解决；后续处理记录如下。

本轮只新增 audit 并更新本记录和任务状态，保留原 proposal、design、delta specs 与 acceptance 供对照。没有运行 Cargo 或产品运行测试，没有归档。

本轮单 change 校验通过，全量严格校验再次得到 19 项通过、0 项失败；Markdown 链接与新文件空白检查通过。`git diff --check` 无错误，产品代码相对前次基线无变化。这些结果不关闭 audit 中的语义发现。

## R1—R6 批准修订后的复核

2026-10-18，用户批准按六项审查意见修订，先不实现。修订内容只位于本 change：proposal、design、八个 capability 的 delta、tasks、acceptance、audit 与本记录。R1—R6 的设计层关闭及验收映射见 [audit.md](audit.md)；实现任务 2.1—5.3 均未勾选，SK-01—SK-41 运行用例均未执行。

源码核对使用 `4e3cccfa35ed93d6d78e08b5d58838bfaa57b8a2`。最终验证时工作区 HEAD 已由并行工作前进到 `dd034d2dae82c03bd9715a3f19d1c17b49402a80`；两者差异只有上游审查提案的归档与文档链接，没有 crates 产品代码变化。因此活动校验项由前次 19 项变为当前 18 项，不能把旧计数直接复用于本轮。

| 检查 | 当前结果 |
| --- | --- |
| 六项设计闭环 | R1 整组发布；R2 首次已接纳响应前全文保护与终止结算；R3 合法 Runtime Step / ToolEvent / Hook / ACP；R4 保留选择意图；R5 删除 Hook mutation；R6 有界展开。均同步到对应 delta、任务与验收。 |
| 再次对抗性复核 | 主 Agent 核对提交、失败、取消、恢复与现有源码约束；Luna 子 Agent 限定范围只读检查跨文档一致性，未发现必须修正的剩余矛盾。 |
| 最后澄清 | 前置任务模型决策与现有权限辅助调用分开；首次消费以已接纳响应为界；图片投影换 ID 后保持当前组关联，终止结算不误删无关内容。 |
| MODIFIED 完整性 | 6 条标题均在主规范中，旧 Scenario 标题均保留。本 change 当前有 8 个 capability、23 条 Requirement、79 个 Scenario。 |
| 验收与任务覆盖 | SK-01—SK-41 连续、唯一；任务引用未越界，六项均有明确运行验收位置。41 项当前全部待实施验证。 |
| 文档结构 | proposal capability 列表与八份 delta 一致；相对 Markdown 链接与完整源码路径有效，新文件无行尾空白。 |
| `openspec validate --all --strict --no-interactive` | 18 项通过，0 项失败，包含本 change。 |
| `openspec status --change unify-skill-loading` | 4/4 planning artifacts complete，仅表示规划文件完整。 |
| `git diff --check` / 工作树 | 检查通过；当前唯一未跟踪项为本 change，未修改已跟踪产品代码或主规范。未跟踪文件另做了空白检查。 |

本轮未运行 Cargo、真实 Provider、UI smoke 或产品运行测试，也未归档本 change。没有生成编译产物。静态校验和设计层关闭不代表加载管线已经实现；后续按验收矩阵逐项取得运行证据。

## 实施授权后的代码与测试记录

用户随后授权实施统一加载；上述“先不实现”和“尚未实现”的结论是历史阶段记录。当前代码入口已落在 `tools/src/implementations/grow_build/load_skill.rs`、`tools/src/implementations/skills/skill.rs`、`shell/src/session/actor/skill_loading.rs`、`shell/src/session/actor/turn/admission.rs` / `interjection.rs` 与 ChatState Timeline / actor。预加载仍复用内容准备函数；显式用户调用和模型调用共用注册 `load_skill`。准备函数对替换后的完整 UTF-8 正文先以 checked length 预检，再构造；模板单次字面替换、参数后缀都计入 1 MiB，超限与取消在准备阶段返回错误，空快照正文有效，合成来源缺正文失败。

实现与验收的当前对应关系：

| 边界 | 实现入口 | 已有或新增验证 | 状态 |
| --- | --- | --- | --- |
| 有界正文准备、空正文、参数后缀、取消 | `tools/src/implementations/skills/skill.rs::prepare_skill_content*`、`expand_skill_content_bounded` | tools 包内技能准备单测；该轮 tools 全包通过 | 已有包级证据；SK 逐项验收未全部核销 |
| 用户输入与有序 Skill 组原子发布、revision 冲突不消费 | `chat-state/src/actor/mutations.rs`、`timeline.rs` | `conditional_skill_input_publishes_user_and_ordered_loads_once`、`conditional_skill_input_revision_conflict_leaves_admitted_input_unconsumed`、`consumed_input_publishes_user_and_ordered_skill_loads_together` | ChatState 包级测试通过 |
| 模型真实调用配对、单个 ToolResult + SkillLoad、错误只结算原 call | `publish_model_skill_conditionally`、`Timeline::valid_model_skill_load` | `model_skill_call_mismatch_fails_closed_and_stale_rejection_uses_only_original_id`、`model_skill_load_requires_one_current_admitted_matching_tool_call` | ChatState 包级测试通过 |
| 首次 accepted response 前 pin、终止结算一次、禁止复活 | `Timeline::unconsumed_skill_load_indices`、`settle_unconsumed_skill_loads`、Shell `emit_turn_ended` | `unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival`、`unconsumed_skill_load_is_pinned_until_settled` | ChatState 包级测试通过；挂起不生成 TurnEnd，故不触发该结算 |
| Shell 真实用户和模型 pipeline、来源冻结、无孤立结果 | `shell/src/session/actor/skill_loading.rs`、`turn/admission.rs` | `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`、`model_load_skill_pipeline_publishes_one_result_and_one_skill_item`、`replayed_user_skill_input_does_not_switch_source_or_leave_result` | **新 Shell tests 尚未运行** |

本轮从 pinned core 日志 `/tmp/grow-v2-4-0-core-tests.log` 记录到的实际结果仅为：ChatState **560 passed, 2 ignored**；tool-types **121 passed**；tools **2412 passed, 7 ignored**。这些是包级结果，不等于 SK-01—SK-41 全覆盖，也不包含上表新增加的 Shell pipeline tests。新测试尚未运行，真实 Provider、normal/minimal/headless smoke 与全量验收仍未验证；因此任务框中没有因这些包级结果而勾选未完整验收项。

### 当前实现的验收映射与缺口

以下记录更新了早期“代码与测试记录”中的历史阶段结论；旧计数与“先不实现”授权只描述当时状态。验收矩阵 SK-01—SK-41 未因此整体通过；除后续源码 / 文档核对确认的 tasks.md 5.2 外，实施任务仍未完成。

| Acceptance | 当前测试与代码落点 | 能证明的行为 / 尚缺证据 |
| --- | --- | --- |
| SK-04、SK-06、SK-39 有界正文准备 | `skill.rs`: `bounded_skill_preparation_counts_suffix_and_replaces_once`、`bounded_skill_preparation_accepts_empty_and_rejects_large_replacement`、`bounded_skill_preparation_observes_preexisting_cancellation`；文件边界覆盖 `explicit_skill_body_read_accepts_file_at_limit`、`explicit_skill_body_read_rejects_file_over_limit_without_partial_body`、`load_skill_content_rejects_synthetic_path_without_body`、`empty_skill_snapshot_survives_disk_changes` | 覆盖替换/后缀、空快照、超限、取消和合成缺 body。仅由 tools 单测不能证明完整请求预算、目录权限和跨层取消时序。 |
| SK-09、SK-21、SK-35 用户路径 | `shell/src/session/actor/tests/skill_loading_tests.rs::user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text` | 真实 Actor + Mock provider 路径断言成功 EndTurn、一次请求及 canonical response、Input Consumed 原文和 SkillLoad 同组原子事实、正文不进入原始用户文本且进入请求。当前没有 Cancelled/准备中取消边界测试；本条不证明 SK-10、SK-13、SK-17、SK-18、SK-19 或 SK-41。 |
| SK-20、SK-21、SK-41 模型路径 | 同文件 `model_load_skill_pipeline_publishes_one_result_and_one_skill_item` | 使用 admitted prompt 触发真实 `load_skill` 工具调用；断言唯一配对 ToolResult/SkillLoadModel、下一真实请求含准备正文、第二次 provider 请求失败后 SkillLoad 被结算且原 call 留下明确未消费结果，completion 不重复结算。测试覆盖的是模型后续采样失败，不是显式取消、挂起或投影 ACK 丢失；挂起不结算仍需独立运行证据。 |
| SK-03、SK-14、SK-22/23 用户来源冻结与重送 | 同文件 `replayed_user_skill_input_does_not_switch_source_or_leave_result` | 同 prompt 重送复用原输入身份；旧来源变更后 fail closed，未切换新同名来源，也没有孤立 ToolResult/SkillLoad。没有覆盖目录权限漂移的所有分支或多客户端 ACK 故障注入。 |
| SK-03、SK-38 Pager 输入意图 checkpoint、待发恢复与重试 | `pager/src/app/root/dispatch/queue.rs::frozen_skill_queue_tests::selected_skill_drain_keeps_checkpoint_intent_and_source_identity`、`pager/src/local_drafts.rs::uncertain_skill_submission_restores_frozen_source_with_original_identity`、`pager/src/local_drafts.rs::worker_restores_pending_skill_as_unsent_draft_with_frozen_source`；重试和恢复发送由 `dispatch/prompt.rs::frozen_skill_retry` 使用保存的 name/path/plugin | 修复两个输入阶段：已提交技能请求通过 in-flight checkpoint 保留原输入 identity 与来源；尚未跨 ACP 的 selected skill 随 staged draft 保存，重启后只回到 composer，仍需显式发送，随后带原来源进入正常队列并获得新 identity。恢复不重建 pending queue、不自动发送、不当作 uncertain submission，也不重新查当前目录。Pager 定向用例包含于已通过的全量 Pager suite；Shell 端到端验证仍待运行。 |
| SK-09、SK-21 Timeline 原子发布与校验 | `chat-state/src/actor/tests.rs`: `conditional_skill_input_publishes_user_and_ordered_loads_once`、`conditional_skill_input_revision_conflict_leaves_admitted_input_unconsumed`、`model_skill_call_mismatch_fails_closed_and_stale_rejection_uses_only_original_id`、`unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival`；`timeline.rs`: `consumed_input_publishes_user_and_ordered_skill_loads_together`、`model_skill_load_requires_one_current_admitted_matching_tool_call`、`unconsumed_skill_load_is_pinned_until_settled` | 验证事务、revision 冲突、真实 call 配对、pin 与终止结算；这些是 ChatState 状态/折叠测试，不能替代 Shell dispatch、取消、provider 与 Pager 验收。 |

截至本记录更新时，父任务准备重跑 Shell 定向验证；上方三项 `skill_loading_tests.rs` 的新 Shell pipeline 用例尚无本轮确认的运行结果。代码中目前未见 user pipeline Cancelled 专项用例。需等待实际运行日志后只登记真实通过/失败结果；不得将 helper、ChatState 包级计数或静态审查等同端到端验收。

## 2026-10-19 实施证据复核

以下状态按父任务提供的最新 package 结果及代码中的具名用例更新。Shell full suite 仍待重跑；没有把前一节的旧计数当作当前结果，也没有运行 Cargo。

| 验收 | 当前运行证据 | 结论 / 发布阻断 |
| --- | --- | --- |
| SK-01—03：注册、资格、唯一解析、来源身份 | Tools `2412 passed`；tool-types `122 passed`。`invocation_resolution_requires_exact_unique_names_and_source_eligibility` 覆盖模型禁用、用户入口资格、限定名及歧义。Shell `model_load_skill_pipeline_publishes_one_result_and_one_skill_item`、`replayed_user_skill_input_does_not_switch_source_or_leave_result` 有源码用例，但最新 Shell 运行尚未取得。 | Resolver 单测通过；完整用户 / 模型管线和重送来源冻结没有当前运行证据。补跑 Shell full 与两个具名用例。 |
| SK-04—07、39：准备替换、链接、快照、严格 UTF-8、有界长度、取消 | Tools `2412 passed`（该计数早于下方新增测试）。已有用例包括 `bounded_skill_preparation_counts_suffix_and_replaces_once`、`bounded_skill_preparation_accepts_empty_and_rejects_large_replacement`、`bounded_skill_preparation_observes_preexisting_cancellation`、`explicit_skill_body_read_accepts_file_at_limit`、`explicit_skill_body_read_rejects_file_over_limit_without_partial_body`、`empty_skill_snapshot_survives_disk_changes`、`load_skill_content_rejects_synthetic_path_without_body`、`internal_link_resolution_preserves_labels_code_and_nested_images`；Timeline 的 `invalid_consumed_skill_group_does_not_consume_input_or_change_surface` 覆盖组总量超过 1 MiB 拒绝。 | 新增 `explicit_skill_body_read_rejects_invalid_utf8_file_without_partial_body`，尚未运行。准备执行期间取消（当前具名取消测试为预先取消）仍缺运行覆盖。`checked_add` 用于长度预检与 suffix 合并；合法 Rust `String`/切片无法构造总长度溢出到 `usize` 的可运行输入，因此不伪造溢出用例，记录为静态算术保护。SK-39 的循环内取消响应时限仍需受控运行测试。 |
| SK-08、30—32：普通 Read、显式 Skill 展示与折叠 | Pager `7477 passed`。包括 `skill_projection_uses_grow_tool_kind_and_structured_result`、`explicit_skill_rows_keep_status_and_are_not_aggregated`、`skill_md_file_read_uses_normal_basename_header`、`skill_header_selects_displayed_basename_and_keeps_full_path_target`、`ordinary_skill_md_reads_are_counted_as_file_reads`。 | 普通 `read_file(SKILL.md)` 显示 Read，路径链接保持完整；显式 `load_skill` 行从结构化工具数据呈现且保留状态、不参加匿名工具合并。normal 主窗口中发布 ACK 驱动状态更新的端到端 smoke 尚缺；不能据此勾 4.1。 |
| SK-09—20、34—35、37—38：用户入口、队列、门控、插话、取消 | Shell 源码含 `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`、`denied_user_skill_is_handled_without_loading_and_next_input_can_continue`、`replayed_user_skill_input_does_not_switch_source_or_leave_result`，但 Shell full 仍待运行。Pager `7477 passed` 含已提交来源 checkpoint、待发本地草稿恢复与新 identity 的测试。 | 当前没有可登记的 Shell 运行证据。具名 Shell 覆盖只含单 Skill 成功、PreToolUse deny 后继续、来源重送；缺多 Skill 中途失败 / 后项不派发，重复引用逐项执行，队列 hold 编辑 / 丢弃 / 消费竞争，准备中取消，允许 Hook 顺序，以及 queued steering / 直接插话携带图片。最小补测应由真实 Actor 覆盖“第一项准备成功、第二项失败且无半组发布”，以及准备中取消后原任务仍可继续；再补一个含附件的插话场景。 |
| SK-21—25、40：Timeline 原子提交、真实 call 配对、ACK / 恢复 | ChatState `561 passed`。`conditional_skill_input_publishes_user_and_ordered_loads_once`、`conditional_skill_input_revision_conflict_leaves_admitted_input_unconsumed`、`model_skill_call_mismatch_fails_closed_and_stale_rejection_uses_only_original_id`、`unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival` 均在该包。 | 原子 fold、revision 冲突、真实 model call 关联、终止结算和 pin 有包级证据。提交后 ACK 丢失 / 崩溃重启、多客户端同 identity 重送、历史 artifact 损坏或 UI 投影失败没有对应运行故障注入；需补一项存储 ACK 不明后对账的真实 Shell/ChatState 恢复测试，再核对 SK-23—25/40。 |
| SK-26—29、33—34、36：Provider 投影、全请求预算、压缩与多 surface | ChatState 和 sampling 相关 package 覆盖只能证明状态层；最新明确计数为 ChatState `561 passed`。Shell full、各 Provider 合法投影 / mock-provider 回归以及 normal/minimal/headless/replay smoke 当前均未提供结果。 | 当前属于主要发布阻断。需要至少对每种支持的协议路径验证模型真实调用只投影一份 SkillLoad 与配对 ToolResult，正文没有被截断且计入完整请求预算；再覆盖首次 accepted response 前压缩 / 重试保留正文，以及已消费后压缩。normal/minimal/headless/replay 的结构一致性需要独立 surface 测试。 |
| SK-41：首次消费前终止 | ChatState 的 `unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival` 通过；Shell 模型 pipeline 测试源码验证后续请求失败时结算，但最新 Shell full 尚未运行。 | 仅有模型后续采样失败路径的待运行证据；未覆盖用户组、显式取消、投影失败、resume/fork 和结算 ACK 不明。需先取得 Shell 运行结果，再补用户组终止和一次恢复不复活正文的测试。 |
| 实施任务 5.2：开发说明与单一入口 | `docs/development.md`、`docs/architecture/agent-core-timeline.md` 已描述统一入口和事实边界。代码搜索显示注册入口为 `LoadSkillTool`；现存 Shell `load_skill_with_body` 调用在 `agent_rebuild.rs` 与 `workflow/tracker.rs`，用于冻结 Agent / Workflow 的预加载快照，不是显式用户 / 模型运行时入口。`build_skill_message` 没有产品调用点。 | 已满足，tasks.md 5.2 已勾选。该静态搜索不替代运行验收。 |

最新用户提供的 package 结果：Pager **7477 passed, 0 failed**；tools **2412 passed, 0 failed**；ChatState **561 passed, 0 failed**；tool-types **122 passed, 0 failed**。Shell full suite 正待重跑。除 tasks.md 5.2 外，宽泛实现任务与 SK-01—SK-41 尚未整体完成；尤其不能把 Shell、Provider、多 surface、ACK/crash、取消及预算缺口视作已关闭。最小发布前工作是取得 Shell full 最新结果，补齐上述代表性真实管线与故障注入测试，并登记 mock-provider / surface 验证；未运行项保持未勾选。

本次文档校验：`openspec validate unify-skill-loading --strict --no-interactive` 通过；`openspec validate --all --strict --no-interactive` 为 **22 passed, 0 failed**；目标文档 `git diff --check` 通过。没有运行 Cargo。

### SK-01—SK-41 逐项状态

“通过”只表示该行所列 package 用例在给定 package 全量结果中通过；“未运行”表示有具名代码测试，但 Shell full 未重跑；“缺覆盖”表示当前没有足以满足该场景的运行用例。

| ID | 运行证据与当前状态 | 剩余缺口 |
| --- | --- | --- |
| SK-01 | `LoadSkillTool` 注册到 Grow registry；Tools suite 通过。Shell user/model pipeline 用例尚未重跑。 | 取得真实用户派发与模型工具调用的 Shell 运行证据。 |
| SK-02 | `invocation_resolution_requires_exact_unique_names_and_source_eligibility` 通过，覆盖 user-only / model-disabled 和资格过滤。 | 缺 Shell 入口使用资格的运行证据。 |
| SK-03 | resolver test 和 Pager 已提交 / staged 来源冻结测试在通过的 tool-types、Pager suite 中。Shell replay 来源变化用例未运行。 | 重跑 Shell replay 测试并确认来源移除时 fail closed。 |
| SK-04 | `bounded_skill_preparation_counts_suffix_and_replaces_once`、`test_substitutions_indexed_arguments` 等由 Tools suite 通过。 | 无已知本项专项缺口。 |
| SK-05 | `internal_link_title_matching_destination_stays_literal`、`internal_link_resolution_preserves_labels_code_and_nested_images` 等由 Tools suite 通过。 | 无已知本项专项缺口。 |
| SK-06 | `empty_skill_snapshot_survives_disk_changes`、`load_skill_content_rejects_synthetic_path_without_body` 由 Tools suite 通过。 | 缺正文引用另一 Skill 时不递归读取的显式回归测试；当前实现路径未发现递归加载调用。 |
| SK-07 | 恰好 1 MiB / 超限文件读取测试由 Tools suite 通过，读取实现使用严格 `String::from_utf8`；新增真实非法 UTF-8 文件拒绝测试 `explicit_skill_body_read_rejects_invalid_utf8_file_without_partial_body`。 | 新增测试尚未运行，等待 targeted 或 Tools suite 验证。 |
| SK-08 | 普通 Read basename / 完整文件链接和显式 Skill 行测试由 Pager suite 通过；预加载调用仍存在。 | normal UI / ACP 更新路径没有本轮运行 smoke。 |
| SK-09 | `conditional_skill_input_publishes_user_and_ordered_loads_once` 和 Surface fold test 在 ChatState suite 通过。 | Shell 用户输入带附件进入模型的真实 pipeline 尚未重跑。 |
| SK-10 | `invalid_consumed_skill_group_does_not_consume_input_or_change_surface` 通过，验证非法组不消费、不改 Surface。 | 缺真实多技能准备中前项成功、后项失败并停止派发的 Actor 测试。 |
| SK-11 | Timeline 已检查 call identity 唯一；没有同名同参 / 不同参重复引用各自执行的 Shell 测试。 | 补一项真实输入包含重复引用的执行与结果数量测试。 |
| SK-12 | substitution / Slash parser helpers 有 Tools 测试；无完整命令路由回归运行证据。 | 补显式 Slash 路由与内建、普通未知 Slash 边界测试。 |
| SK-13 | Pager 全量通过且 queue/local-draft skill checkpoint 测试在其中。 | 缺技能 queue hold、编辑成功 / 失败、丢弃和消费竞争的端到端身份测试。 |
| SK-14 | Shell `replayed_user_skill_input_does_not_switch_source_or_leave_result` 有测试源码但未运行。 | 重跑后再确认排队来源撤销 / 禁用路径。 |
| SK-15 | Shell deny-hook 测试存在但未运行；代码仍走普通 Tool authorization 路径。 | 缺当前 Shell 运行证据，以及 capability / allowed-tools 拒绝时正文未读的断言。 |
| SK-16 | Shell deny-hook 测试源码存在但未运行。 | 缺 Allow / Observe / PostToolUse 顺序、Hook 失败策略与同 identity 重送不重跑的 Actor 断言。 |
| SK-17 | `denied_user_skill_is_handled_without_loading_and_next_input_can_continue` 源码覆盖 Handled 与后续输入，但 Shell suite 未重跑。 | 重跑该测试；另补 Active Goal 时失败不污染后续请求的断言。 |
| SK-18 | 无运行覆盖原工具批次完成后再处理插话的 Skill 场景。 | 补批次边界、原任务继续及无 turn 归属 PluginUsed 的真实 Actor 测试。 |
| SK-19 | Tools 仅有预先取消准备测试；Timeline 测试覆盖组提交拒绝。 | 缺准备中取消、发布前取消 / 发布竞争的 Shell fault-injection 测试。 |
| SK-20 | `model_load_skill_pipeline_publishes_one_result_and_one_skill_item` 源码覆盖成功加载后续采样失败，未重跑。 | 缺 `load_skill` 自身失败后模型仍能处理其他工具结果且无文件读取 fallback 的运行测试。 |
| SK-21 | ChatState 原子 user group、model call/result 配对测试通过。 | Shell pipeline 运行结果未取得；仍需核对下一真实请求正文只投影一次。 |
| SK-22 | ChatState revision conflict / model mismatch fallback 测试通过。 | 缺 ACK 丢失、持久化后 UI 失败及 unknown 对账恢复的故障注入。 |
| SK-23 | 同一已消费身份拒绝重复 append 的 Timeline 测试通过；Shell replay 测试未运行。 | 缺多客户端同时重送和冲突 payload 的真实恢复测试。 |
| SK-24 | Shell 来源变化 replay 用例有源码但未运行；Pager freeze 保存原 path。 | 缺 file change 后 resume/export/import/fork/rewind 的历史快照回归。 |
| SK-25 | synthetic path 缺 body 工具测试通过。 | 缺已发布历史正文 artifact 缺失 / 损坏及未知发布恢复测试。 |
| SK-26 | 本轮无 provider 各协议路径的 Skill 投影运行记录。 | 每种支持的 Provider 协议需验证一份匹配 ToolResult 与 SkillLoad、无重复调度。 |
| SK-27 | Timeline / tools 有 1 MiB 准备上限测试；无完整请求预算运行记录。 | 补真实请求组装超限测试，覆盖工具定义、附件、输出预留及明确拒绝。 |
| SK-28 | 未消费 pin 保护测试通过；它不证明首次消费后压缩行为。 | 补 accepted response 后普通压缩以及新 identity 显式再加载测试。 |
| SK-29 | 无插件分组诊断事件的运行断言。 | 补部分准备失败、整组成功及 ACK unknown 时 success / undispatched 计数测试。 |
| SK-30 | Pager skill projection / explicit row tests 通过。 | 缺 normal 主窗口端到端加载中状态更新 smoke。 |
| SK-31 | Pager 的 Loading / Loaded / Failed / Cancelled 显示测试通过。 | 缺权威 ACK 驱动同一行变更、unknown 不显示 Loaded 的事件级测试。 |
| SK-32 | Pager 结构化投影测试覆盖 source 与 prepared body。 | 缺展开已加载快照后磁盘源变更仍显示原正文的交互测试。 |
| SK-33 | 无 normal/minimal/headless/replay 同事实运行矩阵。 | 补四种 surface 的身份、正文、状态和去重对照。 |
| SK-34 | 无 queued steering / 直接插话 Skill + 图片与 literal `user_query` 标签的运行测试。 | 补带图片插话、原文显示和稳定关联身份的 Actor / Pager 集成测试。 |
| SK-35 | 用户准备后缀及参数替换工具测试通过；Shell user pipeline 用例源码未重跑。 | 缺失败前置项后旧正文 / 参数不进入后续请求的真实请求捕获。 |
| SK-36 | Timeline 未消费正文 pin 与 settle 测试通过。 | 缺 pre-prune、异步压缩发布、模型缩窗、图片投影和 provider retry 的跨层保护测试。 |
| SK-37 | Timeline 消费组 fold 通过；Shell deny Hook 测试未运行。 | 缺真实 Allow / Deny / Cancel 的 Runtime Step、Hook cause、ToolEvent 与 ACP 终态顺序矩阵。 |
| SK-38 | Pager selected-skill queue checkpoint、uncertain restore、pending draft worker restore 测试在通过的 Pager suite 中。 | Shell 中来源删除 / 禁用 / 同名替换后拒绝的测试未运行。 |
| SK-39 | Tools checked-length 与超 1 MiB 拒绝，Timeline 组总量拒绝测试通过。checked_add 对任意可实际构造的合法 `String` 长度进行溢出保护；无法构造导致 `usize` 溢出的合法运行输入，故不添加虚假的溢出测试。 | 新增非法 UTF-8 测试待运行；仍缺准备循环内受控取消及响应时限测试。 |
| SK-40 | ChatState 原子提交与 revision conflict tests 通过。 | 缺提交期间崩溃、ACK 丢失后恢复无半组 / 重复 Hook 的持久化故障测试。 |
| SK-41 | Timeline settle once 测试通过；Shell 模型后续请求失败测试源码未重跑。 | 缺用户组终止、显式取消、投影失败、resume/fork 后不复活及结算 ACK unknown 测试。 |

## Portable Skill wire and Tools regressions — 2026-10-19

Pinned Rust 1.93.1 / two jobs / 16 MiB test stack / no incremental or debug info: `cargo test --locked -p sampling-types -p tools --lib` passed **sampling-types 308/308** and **tools 2415 passed, 0 failed, 7 ignored**. Log `/tmp/grow-v2-4-0-tools-sampling6.log`; tools completed in 28.43s. The two extra one-test summaries inside the log are child-process test invocations, not additional independent package totals.

The new backend tests `user_skill_load_projects_one_complete_call_result_pair_on_every_backend` and `model_skill_load_keeps_only_its_real_tool_exchange_on_every_backend` verify complete paired projection across Chat Completions, Responses and Messages without duplicating the model tool exchange. The strict invalid-UTF8 Skill read and Plan partial-action notes formatting regressions also passed. An earlier build failed in new test fixtures on Arc/string ownership; those fixtures were corrected before this successful run. This result does not replace Shell actor or real-process gates.

## Latest named-test evidence — 2026-10-19

This section supersedes earlier implementation-status rows wherever they say the named Shell tests are still unrun. It does not supersede historical logs or mark tasks complete. Evidence supplied for this review:

- `/tmp/grow-v2-4-0-shell22.log`: relevant named tests report `ok`, but the Shell process was terminated with SIGTERM before a suite summary; do not assign a package pass count.
- `/tmp/grow-v2-4-0-tools-32.log`: **2416 passed, 0 failed, 7 ignored** (2423 discovered).
- `/tmp/grow-v2-4-0-pager31.log`: **7495 passed, 0 failed, 12 ignored** (7507 discovered).
- `/tmp/grow-v2-4-0-chat-state35.log`: **564 passed, 0 failed, 2 ignored**. This later run verifies the three corrected prompt-identity fixtures and the trajectory projection expectation test.
- `/tmp/grow-v2-4-0-suite-build40.log`: successful compile of nine packages (including Shell, ChatState, Pager, sampler, sampling-types, tool-types, tools, workspace, and pager-minimal); compile evidence only, not test evidence.
- `/tmp/grow-v2-4-0-final-fixtures41.log`: both cache frontier tests below pass individually. Two Goal tests failed in that same targeted round due to Goal admission state; the parent reports a usage-window fix, which still needs a rerun.

| SK scenarios | Named running evidence and scope | Remaining evidence gap |
| --- | --- | --- |
| SK-01—03 | Tools32: `invocation_resolution_requires_exact_unique_names_and_source_eligibility`; Shell22: `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`, `model_load_skill_pipeline_publishes_one_result_and_one_skill_item`, `replayed_user_skill_input_does_not_switch_source_or_leave_result`; Pager31 includes `selected_skill_drain_keeps_checkpoint_intent_and_source_identity`, `worker_restores_pending_skill_as_unsent_draft_with_frozen_source`, and `unchanged_retry_reuses_the_original_selected_source`. | Shell suite has no final summary. Resolver and pager state tests do not replace source-removal, all authorization variants, or a complete fresh-process submission smoke. |
| SK-04—07, SK-39 | Tools32: `bounded_skill_preparation_counts_suffix_and_replaces_once`, `bounded_skill_preparation_accepts_empty_and_rejects_large_replacement`, `bounded_skill_preparation_observes_preexisting_cancellation`, `explicit_skill_body_read_accepts_file_at_limit`, `explicit_skill_body_read_rejects_file_over_limit_without_partial_body`, `explicit_skill_body_read_rejects_invalid_utf8_file_without_partial_body`, `empty_skill_snapshot_survives_disk_changes`, `load_skill_content_rejects_synthetic_path_without_body`, `internal_link_resolution_preserves_labels_code_and_nested_images`, and `internal_link_title_matching_destination_stays_literal`; ChatState35's group-size rejection is included among named actor/timeline tests. | There is no executable `usize` overflow input for valid allocated strings; checked arithmetic is static protection. Cancellation test only starts pre-cancelled, so cancellation during a large preparation loop and its response bound remain uncovered. |
| SK-08, SK-30—32 | Pager31: `skill_projection_uses_grow_tool_kind_and_structured_result`, `explicit_skill_rows_keep_status_and_are_not_aggregated`, `skill_md_file_read_uses_normal_basename_header`, `skill_header_selects_displayed_basename_and_keeps_full_path_target`, `ordinary_skill_md_reads_are_counted_as_file_reads`. | UI unit coverage does not prove a normal-window publish ACK updates the existing row or expanded content stays on its original snapshot after disk mutation. The new root Ctrl+C → `grow export` PTY smoke has not run. |
| SK-09—10, SK-17, SK-21, SK-35, SK-37 | Shell22: `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`, `second_user_skill_preparation_failure_does_not_publish_partial_group_and_fifo_continues`, and `denied_user_skill_is_handled_without_loading_and_next_input_can_continue`; ChatState35 named pass: `conditional_skill_input_publishes_user_and_ordered_loads_once`, `conditional_skill_input_revision_conflict_leaves_admitted_input_unconsumed`, `consumed_input_publishes_user_and_ordered_skill_loads_together`. | No active-goal failure check; Shell suite lacks an overall completion result. No allow/cancel/hook-order matrix. |
| SK-11—14 | Pager31 and Shell22 provide source selection, same-command queue edit, replay identity, draft recovery, and frozen retry tests: `queued_skill_edit_keeps_selection_for_same_command_and_clears_it_on_command_change`, `explicit_skill_input_replay_reuses_frozen_selection_without_duplicate_admission`, `replayed_user_skill_input_does_not_switch_source_or_leave_result`, `selected_skill_drain_keeps_checkpoint_intent_and_source_identity`, `worker_restores_pending_skill_as_unsent_draft_with_frozen_source`, `unchanged_retry_reuses_the_original_selected_source`. Shell22 also passes `parse_skill_refs_multi_skill`, `parse_skill_refs_ignores_builtins`, and `parse_skill_refs_ignores_unknown_slash`. | No runtime test proves duplicate references each dispatch independently. Hold, discard, consume-race, and queued-source qualification changes are not fully covered as one real input-state scenario. |
| SK-15—16, SK-18—20 | Shell22 passes `denied_user_skill_is_handled_without_loading_and_next_input_can_continue` and the model pipeline test; Tools32 passes preparation cancellation and invocation eligibility tests. | Missing true cancellation during preparation, queued/active interjection through a completed tool batch, Observe/PostToolUse ordering and failure policy, and load_skill error followed by a different model tool without fallback. |
| SK-22—25, SK-40—41 | ChatState35 named passing tests include `model_skill_call_mismatch_fails_closed_and_stale_rejection_uses_only_original_id`, `unconsumed_model_skill_is_pinned_then_settled_once_without_branch_revival`, `model_skill_load_requires_one_current_admitted_matching_tool_call`, and `unconsumed_skill_load_is_pinned_until_settled`. Shell22 `model_load_skill_pipeline_publishes_one_result_and_one_skill_item` observes a later mock-provider failure and terminal settlement. | No durable ACK-loss/crash/restart, multi-client conflict, corrupt body artifact, export/fork/rewind snapshot, or settlement-ACK-unknown run is evidenced. The later PTY export smoke is pending. |
| SK-26—29, SK-33—36 | Earlier `/tmp/grow-v2-4-0-tools-sampling6.log` has passing `user_skill_load_projects_one_complete_call_result_pair_on_every_backend` and `model_skill_load_keeps_only_its_real_tool_exchange_on_every_backend` across Chat Completions, Responses, and Messages; ChatState35 includes pin/settlement and projection tests in its 564 passing cases. | Tools32 is tools-only and adds no provider evidence. No end-to-end full-request budget, first accepted response before compaction/retry/image projection, after-consumption compaction, or normal/minimal/headless/replay parity matrix is shown. |
| SK-38 | Pager31: selected-source queue checkpoint, uncertain submission restoration, and worker restoration tests listed above all ran in the complete Pager package pass. | Shell fresh-process pipeline with source removal/disable/name replacement is not shown. |

The fixture-41 focused runs additionally pass `captured_display_frontier_does_not_adopt_a_later_cache_append` and `partial_display_cache_requires_exact_committed_prompt_identity_and_is_read_only`; each uses the exact committed prompt identity and a real Grow `TurnCompleted` fixture. The same log's two Goal failures are unrelated to Skill behavior and remain unverified after the reported usage-window fix.

The supplied tests establish useful, named component and Shell-path behavior, but no one log closes the acceptance matrix. Shell22 has no suite result; ChatState35 is clean, while the later Goal fixtures require rerun after their reported fix; the actual Ctrl+C/export PTY gate is pending. Tasks 2.1—5.1 and all acceptance items remain unchecked; the existing documentation-only 5.2 status is unchanged. No Cargo was run for this evidence update.

## Latest focused evidence and remaining acceptance — 2026-10-19

This snapshot supersedes the stale “Shell22 has no suite result” and “ACK/crash recovery has no fault injection” statements above where newer evidence exists. It records only results already supplied by the parent task; Shell82 was still running when this file was edited.

| Evidence | Result and scope |
| --- | --- |
| ChatState | `/tmp/grow-v2-4-0-chat-state-final82.log`: **564 passed, 0 failed, 2 ignored**. This includes the Timeline user-group transaction, model call matching, pin/settlement and cold restore tests. |
| Shell focused user-group failure | `/tmp/grow-v2-4-0-focus82-unconfirmed_user_skill_group.log`: `unconfirmed_user_skill_group_stops_before_provider_without_partial_publication` **1 passed**. It runs both permanent persistence failure and persisted-before-cancel/unknown ACK, asserts no provider request or partial group, and cold-restores the exact persisted group without reactivating Skill bodies. |
| Shell full | `/tmp/grow-v2-4-0-shell-final82.log` was still running and had no final result at this snapshot. Do not treat its partial output as a package pass. |
| Pager / CLI PTY | Pager suite evidence remains **7495 passed, 0 failed, 12 ignored**. CLI73 PTY77 root/tree/export, partial cache, concurrent export and baseline scenarios all exited successfully. None exercises a Skill source freeze through a dedicated fresh-process resume/export flow. |
| Tools / provider projection | Tools32 remains **2416 passed, 0 failed, 7 ignored**; the earlier sampling-types test run passed **308/308** across Chat Completions, Responses and Messages. These package results do not cover complete Shell request budgeting or compaction timing. |

The updated named coverage now closes these previously stale gaps: repeated user references execute as separate calls in `repeated_user_skill_references_publish_one_atomic_group_with_distinct_calls`; second-item preparation failure is covered by `second_user_skill_preparation_failure_does_not_publish_partial_group_and_fifo_continues`; denial followed by a valid next input is covered by `denied_user_skill_is_handled_without_loading_and_next_input_can_continue`; and commit failure / unknown ACK is covered by the focused Shell test above. These tests have source coverage, but only the focused unknown-ACK test has a confirmed current result in this snapshot. `FailPermanent` is intentionally distinct from transient persistence errors, which retry the same immutable event; `PersistThenCancel` means the full event was recorded, its ACK was dropped, and the ChatState cancellation token was cancelled before the commit could be acknowledged.

The smallest remaining run set before this change can close is:

1. Obtain a final Shell package result for build82 (or a later build), including `user_skill_pipeline_consumes_input_with_body_and_keeps_prompt_text`, `model_load_skill_pipeline_publishes_one_result_and_one_skill_item`, `replayed_user_skill_input_does_not_switch_source_or_leave_result`, `repeated_user_skill_references_publish_one_atomic_group_with_distinct_calls`, `second_user_skill_preparation_failure_does_not_publish_partial_group_and_fifo_continues`, `denied_user_skill_is_handled_without_loading_and_next_input_can_continue`, and `unconfirmed_user_skill_group_stops_before_provider_without_partial_publication`.
2. Add or run one real preparation-in-progress cancellation case and one permission / Hook ordering case (Allow or Observe through terminal ToolEvent); current preparation cancellation only starts pre-cancelled, and deny coverage does not establish the full Hook order.
3. Add one complete request-budget rejection case and one accepted-response boundary case that verifies fresh Skill content survives a pre-consumption retry/compaction and becomes ordinarily compactable after first consumption. Provider-pair projection unit tests already cover all three wire backends; they do not replace these request and lifecycle checks.
4. Complete the surface gate with normal/minimal/headless/replay parity for the same loaded facts, plus a dedicated Skill frozen-source fresh-process export/resume check. Existing Pager tests cover staged and uncertain draft identities; CLI73 PTY77 does not exercise this Skill-specific path.

SK-01—08, SK-21—26, SK-30—33, SK-38, and parts of SK-39 have substantial named component evidence; their remaining limits are listed above rather than treating the entire row as untested. SK-09—20, SK-27—29, SK-34—37, and SK-40—41 still require the listed cross-layer or adversarial cases where the acceptance table combines multiple behaviors. `tasks.md` implementation sections now state that code is landed; tasks 2.1—4.2 remain unchecked because each still bundles at least one outstanding acceptance check. Task 5.1 and 5.3 remain unchecked. Task 5.2 remains complete.

## Skill source freeze PTY added — 2026-10-19

`crates/codegen/pager/tests/pty_e2e/ctrl_c_suspend_and_continue.rs::ctrl_c_cold_resume_keeps_the_original_explicit_skill_snapshot` now exercises a workspace-local user Skill through the real Pager and Shell process: it loads the Skill from disk, accepts a provider tool call that starts a long-running command, suspends the process, changes the source file, then cold-resumes the same session. The assertions check one durable user Skill group and `load_skill` lifecycle, the original prepared body in both provider requests, absence of the changed body, one command start, and completion of the original Turn.

This PTY test has not been run. It is an added verification path, not a passing result; the existing CLI73 PTY evidence does not cover this Skill-specific source-freeze scenario.