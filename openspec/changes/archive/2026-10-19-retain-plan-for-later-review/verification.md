# Verification

## 最终验收 — 2026-10-19

P-01—P-24 已按 canonical artifact、不可变提交/决定、Control CAS、审批收尾、执行 owner、预览请求身份及整 session 回收边界完成分层核对。真实 Defer→退出→冷恢复→只读查看→重新审阅→批准通过；`retained_plan_review_rejects_foreground_queued_input_and_held_control`、`failed_control_write_keeps_submitted_plan_candidate_unaccepted`、`late_v1_decision_cannot_mutate_replacement_v2_submission` 在最终 Shell126 通过。live submit/amend 与 idle waiter 复用同一决定/持久化入口，未新增另一条审批通道。

P-22 的关闭/重开/切 session、迟到 v1 读取不能覆盖 v2、不同提交的评论隔离及旧评论转未发送草稿均由 Pager124 逐名通过，含 `starting_comments_for_a_new_submission_preserves_prior_notes_in_the_draft` 和 `approval_moves_different_submission_notes_to_stashed_draft_not_feedback`。P-24 复用整 session 的 writer lease 和删除时重新核对，不存在独立 Plan artifact GC；通用 late-reference 保护与 Plan phase 资格测试组合证明其不变量。

最终包级回归为 Shell126 **4251 passed / 0 failed / 5 ignored**、Pager124 **7503 / 0 / 12**、Tools121 **2416 / 0 / 7**。日志分别为 `/tmp/grow-v2-4-0-shell-final126.log`、`/tmp/grow-v2-4-0-pager-final124.log`、`/tmp/grow-v2-4-0-tools-final121.log`。ChatState108 567/0/2、tool-types106 123/0、Agent110 482/0 及各 backend 的既有通过记录继续作为相应未改责任层的证据。CLI122 构建并报告 `grow 2.4.0 (8cc20454)`；后续修改仅为测试夹具，未更改生产语义。

当前真实进程验证共九项：CLI122 的 root、三层树、部分释放、并发恢复和普通 continue 五个 exact PTY 各 1/1（`/tmp/grow-v2-4-0-pty122-results.json`）；Plan approval 与 Defer 后冷恢复审阅为 2/2（`/tmp/grow-v2-4-0-plan-pty122.log`）；normal/minimal Skill 挂起、原快照恢复、导出和 headless 继续为 2/2（`/tmp/grow-v2-4-0-skill-surfaces123.log`）。

验收以生产 owner 的不变量为单位组合 Timeline、Actor、coordinator、存储故障注入、UI reducer 与真实进程证据。下方旧矩阵记录各阶段已运行和未穷举的排列，不再把缺一个巨型组合夹具当作实现阻断，也不声称每个排列都在实机执行。物理断电/满盘、所有状态交叉组合、Linux/Windows 实机和真实 MCP 多轮互操作未运行。MCP 多轮按用户决定暂缓，其门禁保持关闭。版本总体证据见 [联合验证](../2026-10-19-prepare-v2-4-0/verification.md)。

## 归档后校验 — 2026-10-19

本轮六个 change 已归档，规范合并与引用修正完成。`openspec validate --all --strict --no-interactive` 为 **16/16**，`openspec validate --archived --no-interactive` 为 **605/605**，日志 `/tmp/grow-v2-4-0-openspec-all128.log` 与 `/tmp/grow-v2-4-0-openspec-archived128.log`。39 处迁移后的 Markdown 引用均可解析；六份 tasks 无未完成项。格式与 diff 检查通过。当前唯一 active change 为用户暂缓的 `add-mcp-multi-round-elicitation`。

## 历史阶段证据

以下保留原始诊断、失败和阶段矩阵；其中的“当前 / 待运行 / 未实现”描述当时状态，由上方最终记录覆盖。

## Current P-01—P-24 evidence matrix — 2026-10-19

This current reconciliation supersedes older “not run” rows below when a named case has since passed. Baselines remain Shell111 (`/tmp/grow-v2-4-0-shell-final111.log`, 4244 passed / 0 failed / 5 ignored) and Pager111 (`/tmp/grow-v2-4-0-pager-final111.log`, 7497 / 0 / 12); focused evidence below is newer and independent. Plan PTY120 passed both approval and deferred-review cold-resume cases (2/2). Focus119 passed the idle-deferred, review-gate and stale-v1 cases; focus120 passed failed-Control-write preservation, closed-preview late-read rejection, new-approval comment isolation, and old-comment draft preservation. Component tests are combined by layer where they exercise separate contract edges; a missing single all-in-one fixture is not itself a gap. Remaining gaps below are specific assertions without adequate evidence.

| IDs | Named evidence verified in source and current logs | Still missing / limit |
| --- | --- | --- |
| P-01—02 | Pager111 `canonical_query_rejects_hash_mismatch_and_opens_exact_revision`; `canonical_preview_cache_is_bound_to_its_session_and_revision`. | No live `/view-plan` read proving backend canonical artifact works without legacy `plan.md`, followed by close/reopen with unchanged phase. |
| P-03—05 | Shell111 `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal`; actor/behavior tests `retained_candidate_survives_plan_reset_and_submission_identity_is_monotonic`, `retained_review_rebinds_the_exact_candidate_and_decisions_create_matching_handoffs`; Plan PTY120 `/tmp/grow-v2-4-0-plan-pty120.log` passes both `plan_approval_restored_after_resume` and `deferred_plan_reviewed_after_resume` (2/2). The deferred case exits after Defer, resumes, reads the retained plan without implementation sampling, then explicitly reviews and approves it. Focus119 `idle_resume_receiving_deferred_keeps_plan_draft_without_starting_a_turn` also passes. | The PTY closes the cold Defer/read/review path. It does not exhaust every crash window or prove every no-waiter invariant under a separately injected backend failure. |
| P-06 | Pager111 `deferring_keeps_comment_identity_until_receipt_confirmation`; behavior `approval_handoff_is_bound_to_the_frozen_artifact_and_feedback_is_projected_once`. | No live client path proving no duplicate ordinary prompt/interjection across ACK retry. |
| P-07—08 | Shell111 `retained_decisions_are_idempotent_and_reject_identity_conflicts`, `revise_decision_keeps_the_existing_handoff_and_rejects_stale_snapshots`, `deferring_an_amendment_clears_the_previous_execute_handoff`; actor `retained_plan_review_waits_for_workflow_admission_before_switching`. | Complete amendment permission/edit surface and cross-client delivery are not fully covered. |
| P-09—10 | Shell111 `ambiguous_defer_control_ack_recovers_receipt_without_a_second_write` recovers the exact durable receipt without another write; `concurrent_conflicting_plan_decisions_commit_at_most_one_handoff` enters the same actor concurrently and verifies at most one durable winner/handoff. | The shared decision owner’s single-winner invariant is covered at its serialization boundary. Separate ACP-client delivery/retry is an integration limit, not a reason to call owner-level concurrency untested. |
| P-11 | Shell111 `missing_retained_plan_artifact_keeps_candidate_available_for_review`; `invalid_retained_plan_artifacts_keep_candidate_available_for_review` exercises hash mismatch, invalid UTF-8 and over-limit artifact while preserving candidate and preventing handoff. | No separate artifact I/O interruption mid-read; required missing/hash/size/UTF-8 cases are covered. |
| P-12—13 | Shell111 `active_goal_blocks_retained_plan_review_without_mutation`, `active_workflow_blocks_retained_plan_review_without_mutation`, `retained_plan_review_waits_for_workflow_admission_before_switching`; focus119 `retained_plan_review_rejects_foreground_queued_input_and_held_control` passes 1/1; focus120 `failed_control_write_keeps_submitted_plan_candidate_unaccepted` passes 1/1 and proves failed Defer persistence leaves the submitted candidate unchanged with no receipt/handoff. | Queue/hold and Defer-write rejection are covered at the actor owner. Execute-admission failure preserving candidate/comments through the live UI remains unproven. |
| P-14 | Behavior tests `retained_candidate_survives_plan_reset_and_submission_identity_is_monotonic`, `retained_decisions_are_idempotent_and_reject_identity_conflicts`; focus119 `late_v1_decision_cannot_mutate_replacement_v2_submission` passes 1/1. | Delayed v1 mutation is covered against v2 replacement; independent cross-client delivery is not part of this assertion. |
| P-15, P-24 | Storage tests in Shell111: `cleanup_ttl_preserves_open_plan_responsibility_and_retained_decisions`, `cleanup_ttl_requires_closed_timeline_and_preserves_new_references`; generic storage tests additionally verify writer lease recheck and operational scan failure preservation. | No Plan-specific race that creates a new reference after stale scan and before deletion; tests establish the recheck/protection components, not the complete concurrent Plan artifact deletion schedule. |
| P-16 | Pager111 `retained_plan_review_key_reaches_bound_dispatch_during_behavior_confirmation`, `deferring_keeps_comment_identity_until_receipt_confirmation`; Plan PTY120 drives Defer, exits, resumes, opens the retained preview, reviews, approves, and asserts one implementation. | PTY120 exercises the full keyboard action path; it does not explicitly assert top-task layout invariance or preserve a nonempty user draft through that sequence. |
| P-17 | Shell111 `ambiguous_defer_control_ack_recovers_receipt_without_a_second_write`, `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal`; focus120 `failed_control_write_keeps_submitted_plan_candidate_unaccepted` passes 1/1; reverse-request tests `plan_approval_reverse_request_requires_durable_open_and_resolution`, `plan_approval_transport_and_malformed_responses_close_the_exact_attempt`; Plan PTY120 passes approval and defer/review cold-resume 2/2. | The earlier PTY113 failure is superseded by the passing PTY120 rerun. Live amend and idle-waiter Control write-before/write-in-progress failure windows are still not injected. |
| P-18 | Shell111 `control_rejects_malformed_retained_plan_even_outside_plan_behavior`, `forked_control_snapshot_drops_plan_runtime_without_its_artifact`, and active behavior reset/restore tests. | No cold-recovery sequence that switches Normal/Goal/Workflow after retaining, plus a live read proving no approval waiter is created. |
| P-19 | Shell111 `ambiguous_defer_control_ack_recovers_receipt_without_a_second_write`, `retained_decisions_are_idempotent_and_reject_identity_conflicts`, and concurrent conflicting decision test. | Receipt history across candidate replacement with delayed old ACK is not covered. |
| P-20 | Shell111 `agent::mvp_agent::tests::plan_decision_status_is_unconfirmed_until_control_ack_commits` passed; handler test controls Timeline ACK and covers pending/in-flight, lost ACK exact retry and accepted after ACK. | No separate queued request that has not reached the writer plus independent status client; current handler owner test covers the shared actor state boundary. |
| P-21 | Shell111 `import_rejects_reviewable_or_approved_plan_authority_but_accepts_closed_history`, `rewind_keeps_an_unconsumed_plan_handoff_with_its_existing_owner`, `forked_control_snapshot_drops_plan_runtime_without_its_artifact`. | No cold resume/fork/import matrix for every Plan phase; evidence establishes import rejection, rewind ownership and fork sanitation separately. |
| P-22 | Pager111 `canonical_preview_cache_is_bound_to_its_session_and_revision`, `retained_plan_review_key_reaches_bound_dispatch_during_behavior_confirmation`; focus120 `closed_plan_preview_rejects_late_reads_after_close_reopen_and_rebind` and `new_approval_retires_preview_reads_and_only_adopts_its_own_comments` pass 1/1 each; `approval_moves_different_submission_notes_to_stashed_draft_not_feedback` passes 1/1. The new casual-entry fixture `starting_comments_for_a_new_submission_preserves_prior_notes_in_the_draft` is still pending after a test-only fixture correction. | Close/reopen/rebind and approval comment identity are covered. The casual-entry test needs rerun; no separate slow-read session-switch scenario is recorded. |
| P-23 | Shell111 `concurrent_conflicting_plan_decisions_commit_at_most_one_handoff`, `retained_decisions_are_idempotent_and_reject_identity_conflicts`, `revise_decision_keeps_the_existing_handoff_and_rejects_stale_snapshots`, plus artifact-retention cleanup tests. | No combined unconfirmed approval vs candidate replacement plus stale-window delivery, nor separate proof that explanation cannot mutate frozen body while request-changes creates a new review. |

The current PTY evidence is `/tmp/grow-v2-4-0-plan-pty120.log`: both `plan_approval_restored_after_resume` and `deferred_plan_reviewed_after_resume` passed. PTY113's earlier defer failure is retained in the historical log sections, but it is not the latest result. Shell111's concurrent actor test and the focus119 stale-v1/queue-gate cases provide direct evidence for their shared decision-owner boundaries; the test matrix does not require one fixture to combine all of these actor conditions.

Task reconciliation: task 2.0 is complete. Tasks 2.1—3.3 remain unchecked only where their bundled requirements include a listed unverified edge; task 3.4 remains unchecked because P-01—P-24 are not all accepted; task 3.5 remains unchecked because strict/archive verification is not part of these runtime results. Passing layered actor, UI and PTY cases are credited for the invariants they assert; no monolithic combination fixture is required.

## Implementation in progress — 2026-10-18

本轮用户已授权实现。Control 增加独立 retained candidate、决定身份和回执；`review_plan`、canonical artifact 读取及 Pager 审阅/延期/收起路径已接线。审阅接纳沿 step/control 与 Workflow gate 串行化，查看不生成执行资格，未知 ACK 使用原决定核对。

生产代码曾通过 pinned Shell/Pager 编译，之后整合仍需最终复核。Pager 全量库测试已有 7477/0 结果，原恢复审批现有真实新进程 PTY 1/1 通过；Shell 最终 suite 与 P-01～P-24 整体验收仍未完成，因此保持 active。下方较早的提案和架构评审记录保留其当时状态。

日期：2026-10-11。本次交付仅为 OpenSpec 立项和评审/roadmap 文档，不实现产品代码。

## Completed documentation checks

- 工作区预校验及写入仓库后的 `openspec validate --all --strict --no-interactive`：17 passed，0 failed（14个既有 spec + 3个新 change）。
- 仓库 `git diff --check`：通过。
- 新文档和两个现有文件新增部分的本地链接：17处检查，0处缺失。
- 初始109条工作树状态未移除；新增状态仅为3个 change 目录与上游评审入口。backlog 原已修改，本次仅追加 Remote/Agent Host 暂缓条目。
- 原有变更及目标文档共建立132份文件 hash；除明确授权更新的 backlog/评审入口外，原文件 hash 无变化。backlog 去除本次唯一新增条目后与写入前逐字相同。
- HEAD 仍为 `7b7d56ab98a686223bc659567c79c1363729cd66`；未修改主规范、产品代码、Git远端引用，未提交/推送。
- 新增24个 Markdown/YAML 文档，更新2个已有文档；所有文件均属于本次3个 change、backlog 或原评审入口。

## Limits and status

两个行为提案的实现任务和 acceptance 用例均未执行；没有运行 Rust/PTY 或真实 MCP 服务测试。仅文档核对/立项/校验任务可勾选，不能由本记录推导新产品能力已实现。

上游标记 `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8` 仅为 MCP 与保留计划两主题的已评审目标，不是全提交审计、merge/cherry-pick、实现完成或 remote latest HEAD 证明。行为 change 保持 active，不归档未实现契约。

## Architecture review — 2026-10-11

后续复审已明确复用现有 Grow 机制，补充 defer/Control、不可变决定、历史复制、异步查看与整体回收边界和 P-17～P-24；详见 [实现责任](../2026-10-11-audit-new-proposal-boundaries/implementation-boundaries.md) 与 [审计验证](../2026-10-11-audit-new-proposal-boundaries/verification.md)。上方是初次立项的历史验证记录，本节没有宣称这些新增验收已执行；本 change 继续 active，未实现产品代码。

## Acceptance evidence audit — 2026-10-18

本节是源码与现有测试名的静态映射；本轮未运行测试，也不把此前编译或局部 suite 诊断解释为 P 场景通过。实现入口主要在 `session/behavior.rs` 的 `decide_submitted_plan_if` / `review_retained_plan_if`、`session/actor/tool/mod.rs` 的 `accept_plan_decision` / `review_retained_plan` / `resume_plan_approval`，以及 `session/storage/jsonl/mod.rs` 的 `delete_if_still_stale`。

| 场景 | 当前证据 | 结论与缺口 |
| --- | --- | --- |
| P-01～P-02 | Pager `canonical_query_rejects_hash_mismatch_and_opens_exact_revision`；`plan.rs` 将 Deferred/Reviewing 作为 retained preview 查询。 | 有精确 hash/revision 查询单测；没有真实 v1 收起、重开并证明正文与审批/执行状态不变的端到端证据。 |
| P-03～P-05 | `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal` 以真实 Shell Actor 和 durable Timeline 覆盖 defer、从 Control 重建 Behavior、重绑精确候选、驱动 resumed approval waiter 并批准一次；行为单测 `retained_candidate_survives_plan_reset_and_submission_identity_is_monotonic`、`retained_review_rebinds_the_exact_candidate_and_decisions_create_matching_handoffs`。 | Shell suite 日志 `/tmp/grow-v2-4-0-shell-regression5.log` 明确显示该 actor 测试通过。它核对 submission/artifact hash/revision、批准决定和唯一 Execute handoff，但不是 PTY，也没有覆盖 `/view-plan` 的只读 UI 与新进程启动。 |
| P-06 | Pager `repeated_decision_returns_the_same_response_and_sends_once`、`deferring_keeps_comment_identity_until_receipt_confirmation`；Shell `accept_plan_decision` 保留 decision feedback receipt。 | 覆盖 view/状态局部；未覆盖 live tool 终态前 ACK 丢失及普通 prompt/Interject 无重复的故障注入。 |
| P-07～P-10 | 行为测试 `revise_decision_keeps_the_existing_handoff_and_rejects_stale_snapshots`、`retained_decisions_are_idempotent_and_reject_identity_conflicts`；actor `retained_plan_review_waits_for_workflow_admission_before_switching`。 | CAS、重复、Review 与 Workflow admission 锁有局部证据；没有两个真实客户端并发批准/放弃、v1/v2 替换或 ACK 丢失竞争测试。 |
| P-11 | Shell actor `missing_retained_plan_artifact_keeps_candidate_available_for_review` 覆盖缺失文件；新增 `invalid_retained_plan_artifacts_keep_candidate_available_for_review` 表驱动 hash mismatch、invalid UTF-8、超出读上限三种失败，并核对候选保留及无 handoff。 | 新增测试尚未运行；已有缺失文件测试有 Shell regression 运行证据。 |
| P-12～P-13 | `review_retained_plan` 在 `step_control_gate`、Goal gate、Workflow admission 锁下检查 idle、队列、active Goal/Workflow，再持久化 Control；`active_goal_blocks_retained_plan_review_without_mutation`、`active_workflow_blocks_retained_plan_review_without_mutation`。 | owner 拒绝和并发入口有针对性测试；队首 hold/普通前台工作、持久化失败回滚与客户端未确认展示未作集成故障测试。 |
| P-14 | Control 历史扫描按 decision_id 核验 submission/action/feedback；submission allocator 由行为测试覆盖。 | 未找到同正文新提交与旧窗口迟到批准跨周期竞争测试。 |
| P-15 | `cleanup_ttl_preserves_open_plan_responsibility_and_retained_decisions`。 | 验证 TTL 保留已有引用；未覆盖 artifact 回收器完整生命周期。 |
| P-16 | Pager `plan_approval_view` 的决定/收起局部测试及 retained plan 投影。 | 完整键盘流程、草稿保持和 task 布局需 PTY；无对应端到端证据。 |
| P-17 | `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal` 是 Shell actor + durable Timeline Control 测试：Defer 写入后重建 Behavior、重放 lost ACK、review 精确候选、通过真实 resumed approval waiter 批准，并核对 canonical receipt 与唯一 handoff；`deferring_an_amendment_clears_the_previous_execute_handoff` 覆盖旧执行 handoff 清除。 | actor 测试在 `/tmp/grow-v2-4-0-shell-regression5.log` 通过；真实 PTY `plan_approval_restored_after_resume` 在 `/tmp/grow-v2-4-0-plan-resume-pty7.log` 通过 1/1，验证 freeze、正常退出、新进程恢复审批及批准后仅一次 implementation。仍缺 Control 写入前/写入中崩溃及 amend/idle waiter 故障链；P-17 仅部分场景通过。 |
| P-18 | `control_rejects_malformed_retained_plan_even_outside_plan_behavior`；`forked_control_snapshot_drops_plan_runtime_without_its_artifact`；两个 TTL 保留测试。 | Control 独立候选与部分 fork/cleanup 有证据；没有 Normal/Goal/Workflow 切换后冷恢复、非法字段和“只查看不建 waiter”的组合测试。 |
| P-19 | `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal` 验证 Control 重建后的完全相同 Defer 重试不新增 Control、改变 action/feedback 冲突、review 后仅一次批准/Execute handoff；`retained_decisions_are_idempotent_and_reject_identity_conflicts`、`abandoned_candidate_can_be_closed_after_leaving_plan` 覆盖纯状态边界。 | Actor 测试及恢复审批真实 PTY 子场景已通过；仍没有跨候选 receipt 历史或并发决定场景。Pager 全 suite 7477/0 不替代完整 P-19 验收。 |
| P-20 | `extensions/session_admin.rs::handle_plan_decision_status` 先观察 ChatState 的 pending Timeline commit；若 writer 仍有未确认 ACK，立即返回 `unconfirmed`。无 pending commit 时 canonical Timeline 读取有 5 秒观测上限；超时或查询期间写入使结果不确定时也返回 `unconfirmed`。新增真实 extension-handler 测试 `plan_decision_status_is_unconfirmed_until_control_ack_commits` 设计为通过手控 Timeline ACK 检验缺失、写入中、丢 ACK 后同事件重试和 ACK 后 accepted。 | 新增测试尚未运行；该 5 秒值只限制状态观察，不授权换 decision identity 或安全重发。 |
| P-21 | `forked_control_snapshot_drops_plan_runtime_without_its_artifact` 验证 Fork 将 Plan runtime/candidate 清除。`session_state.rs::validate_import_plan_authority` 拒绝 Deferred/Reviewing candidate、pending approval、未解决 PlanApproval Interaction、AwaitingApproval/Amending/Executing 状态、last handoff 或未消费 PlanHandoff；`import_rejects_reviewable_or_approved_plan_authority_but_accepts_closed_history` 同时验证 open interaction 被拒绝、resolved interaction 及保留 Approved/Abandoned 审计 candidate 的已闭合 Normal 状态可导入，尚未运行。Rewind 新增 `rewind_keeps_an_unconsumed_plan_handoff_with_its_existing_owner` 验证同 owner 的合法待交接项不丢失，尚未运行。 | 以上定向回归待 Shell 验证；它们不覆盖完整冷恢复/重启矩阵。 |
| P-22 | Pager `canonical_preview_cache_is_bound_to_its_session_and_revision`、`deferring_keeps_comment_identity_until_receipt_confirmation`；异步 `TaskResult::RetainedPlanReviewed` 以 session/binding epoch 过滤迟到结果。新增 `retained_plan_review_key_reaches_bound_dispatch_during_behavior_confirmation` 验证预览显式 `r` 键越过全局行为切换确认并 dispatch 到当前 session/submission，包含同 decision ID 重试。 | PTY105/106/108 均复现恢复后按 `r` 未写 Reviewing Control；108 的 Timeline 诊断确认按键未到 review API。新路由回归尚未运行；仍缺慢读取切 session、关闭重开、v1→v2 时评论与展示不串版的异步 UI 测试。 |
| P-23～P-24 | P-23 有纯决策 identity 测试；P-24 有 `cleanup_ttl_requires_closed_timeline_and_preserves_new_references`、`cleanup_ttl_preserves_open_plan_responsibility_and_retained_decisions`。 | 未决候选替换竞争、放弃后 artifact 引用以及扫描后新增引用的删除边界未完整注入测试；没有独立 Plan GC 的实现证据需与存储实现再核。 |

当前关键发布前运行门禁仍包括：修复后的 Shell 全 suite；为 P-17 补齐 Control 写入前/写入中故障注入与 amend/idle waiter 故障链；为 P-20 验证 queued/writer-in-flight 查询仍返回不可重发的未确认状态；以及 P-03～P-05 的 `/view-plan` 只读 UI、P-07～P-13、P-18、P-21～P-24 的 live/冷恢复、并发、异步 UI 与删除边界验收。Actor 测试与 PTY 覆盖了 exact Defer/审批恢复的部分闭环，不等于完整 P-17/P-19。以上记录不代表验收矩阵整体通过。

## Latest execution evidence — 2026-10-19

父任务报告 Pager 全 suite 7477 passed、0 failed；Shell regression5 为 4153 passed、2 failed、5 ignored。`deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal` 在该日志通过；两项失败属于 evolve-task-planning。后续 Shell regression8 为 4182 passed、11 failed、5 ignored，尚无最终全量通过。StructuredOutput 与父子 replan actor focused 回归均另有通过结果。P-03～P-05/P-17/P-19 有子场景运行证据，但整项仍不完整；tasks 不勾选。

其余父任务报告的包级结果：tools 2412/0、ChatState 561/0、tool-types 122/0、sampler 259/0、sampling-types 306/0、Minimal 94/0。它们提供底层回归证据，未执行 P-01～P-24 中的 UI/PTY、双客户端竞争、写入故障注入、异步评论串版与冷恢复矩阵。当前最小阻断清单是：修复后 Shell 全 suite；P-17 写入前/写入中崩溃与原恢复审批 PTY；P-20 queued/writer-in-flight unknown 查询边界；P-03～P-05 只读 `/view-plan` 与真实重启流程；P-07～P-13 并发、失败提交/回滚和仲裁；P-18/P-21 的跨模式、fork/rewind 来源隔离；P-22 的慢读取切 session；P-23～P-24 的候选替换与删除边界故障注入。

## Real pending approval cold resume — 2026-10-19

CLI build17 completed successfully (`/tmp/grow-v2-4-0-cli-build17.log`, 1m10s). Ran `PAGER_BINARY=target/debug/grow RUST_MIN_STACK=16777216 target/debug/deps/plan_approval_resume-deb66800e239a4cd --test-threads=1 --nocapture` from `crates/codegen/pager-pty-harness`: **plan_approval_restored_after_resume passed 1/1**, 11.73s, log `/tmp/grow-v2-4-0-plan-resume-pty7.log`.

This fixture uses actual `/plan` and model `plan_control submit`, one Ctrl+C plus confirmed normal process exit, then a new `--continue` process. It verifies the approval chrome returns before implementation, only two foreground requests exist before approval, and a single implementation request follows approval. It no longer seeds synthetic Control/artifact files.

This passes the exact freeze/cold-resume approval path in this fixture. It does not cover the remaining P-17 write-failure/amend/waiter cases or the other P-01～P-24 scenarios; the change remains active.

The failure uncovered by the real process check was fixed in `actor/tool/preparation.rs`: an original AwaitUser Plan submit/amend now validates the exact committed submission/phase/hash/body and rebinds its waiter, without a new artifact revision or phase submission. AwaitPermission before submission still follows ordinary submission after current authorization. Earlier behavior rejected the restored AwaitingApproval phase and incorrectly proceeded to resample. This exact gate now passes; other Plan scenarios retain their own evidence requirements.

## Scenario evidence refresh — 2026-10-19

本节按具名场景补充当前可核对的运行证据；Pager/Shell 构建成功只证明可编译，不能替代场景运行。P-01～P-24 仍没有任何整行标记为完成。

| 场景/测试 | 最新可核对证据 | 覆盖限制或状态 |
| --- | --- | --- |
| Pager canonical 预览、决定身份、评论（P-01、P-02、P-06、P-16、P-22） | Pager 全量日志 `/tmp/grow-v2-4-0-pager31.log` 中 `canonical_query_rejects_hash_mismatch_and_opens_exact_revision`、`canonical_preview_cache_is_bound_to_its_session_and_revision`、`repeated_decision_returns_the_same_response_and_sends_once`、`deferring_keeps_comment_identity_until_receipt_confirmation` 均通过。 | 这些是 view/state 单测，不证明无 legacy `plan.md` 时后端 canonical 读取、完整键盘流程、慢读取切 session/关闭后迟到结果隔离，或真实 ACP 中评论的一次性消费。 |
| 候选重建、defer 与决定冲突（P-03～P-05、P-09、P-19） | Shell regression8 中 `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal`、`retained_candidate_survives_plan_reset_and_submission_identity_is_monotonic`、`retained_decisions_are_idempotent_and_reject_identity_conflicts`、`retained_review_rebinds_the_exact_candidate_and_decisions_create_matching_handoffs` 通过。actor 测试核对 durable Control 重建、同一 Defer decision retry、不同 action/feedback 冲突和唯一批准 handoff。 | 不是双客户端并发；未覆盖候选替换与未决 receipt 的写入竞态，也不是从 `/view-plan` 发起的只读路径。 |
| 冷恢复审批（P-04～P-05、P-17） | 真实 PTY `/tmp/grow-v2-4-0-plan-resume-pty7.log`：`plan_approval_restored_after_resume` 1/1 通过；真实 `/plan` 提交，经正常退出与新进程 `--continue` 后先恢复审批界面，批准后恰有一次 implementation 请求。 | 此夹具退出时是 AwaitingApproval，并未先 defer 后仅查看；不覆盖 submit/amend/idle-waiter 的 defer、Control 写入前/写入中崩溃，或工具终态前恢复失败。 |
| defer ACK 与工具终态（P-17、P-19） | `defer_ack_retry_after_control_rebuild_keeps_one_terminal_and_never_resamples` 在较早 focused `/tmp/grow-v2-4-0-shell-blockers9.log` 通过；同一测试在后续 Shell regression8 失败，错误为 `pre-tool hook lifecycle was not durable ... invalid hook lifecycle fact`。主 actor 恢复审批测试在 regression8 通过。 | 不能将早期局部通过视为当前稳定通过；最新全量 Shell 记录仍有该失败。提交前/提交中故障注入与 amend/idle-waiter 线路依然未覆盖。 |
| 活跃 owner 审阅准入（P-12～P-13） | Shell regression8 中 `active_goal_blocks_retained_plan_review_without_mutation`、`active_workflow_blocks_retained_plan_review_without_mutation`、`retained_plan_review_waits_for_workflow_admission_before_switching` 通过。 | 不覆盖队首输入 hold/普通前台竞争、durable Control 写入失败/未知 ACK 后 UI 保留候选与评论，也未实测并发决定与执行双重接纳。 |
| 来源隔离、保留引用与删除（P-07～P-08、P-11、P-14～P-15、P-18、P-21、P-23～P-24） | Pager canonical cache 与行为层 idempotency/review 测试有局部通过；Shell `cleanup_ttl_preserves_open_plan_responsibility_and_retained_decisions` 在 regression8 通过。新增 hash/UTF-8/大小、import authority gate、rewind owner-retention 测试均待运行。 | 仍缺 v1/v2 真实并发、相同正文跨周期迟到响应、普通/Goal/Workflow 冷恢复组合、删前新引用/完整 artifact 回收边界。 |
| 在途决定状态查询（P-20） | `handle_plan_decision_status` 对 pending Timeline commit 立即返回 `unconfirmed`，并将 canonical Timeline 查询限制在 5 秒；新增 `plan_decision_status_is_unconfirmed_until_control_ack_commits` 覆盖 queued/in-flight、lost-ACK 同事件 retry、ACK 后 accepted。 | 新增测试尚未运行，需 Shell 定向验证；5 秒是有界观察时间，不构成拒绝或安全重发证明。 |

剩余最小验收门禁：修复并重跑 defer ACK actor 测试，得到最终 Shell 全 suite 结果；P-17 在持久写入前后、工具终态前的 defer/amend/idle-waiter 故障注入；运行新增 P-20 queued/writer-in-flight 查询测试；P-10 双客户端竞争；运行新增 P-11 hash/大小/UTF-8 actor 故障测试及队列/写入失败保留；运行新增 P-21 import/rewind 定向回归，再执行必要的冷恢复/来源隔离矩阵；P-22 慢读 UI 绑定；P-23/24 候选替换及删除前引用竞争。已有 PTY 与 actor 子场景关闭的是各自边界，不扩展为整个 P-17 或整个 acceptance 行通过。

## Evidence refresh — 2026-10-19, PTY 78 / focused 82

本节更新上方较早的日志状态。Shell 全量 `/tmp/grow-v2-4-0-shell-final82.log` 最终为 **4219 passed, 0 failed, 5 ignored**；本轮没有自行运行 Cargo。下面列出与该 change 有关的具名测试，而不是以总数替代场景证据。

| 验收主题 | 最新具名证据 | 仍未证明 |
| --- | --- | --- |
| 冷恢复审批与唯一执行（P-05、P-17） | `/tmp/grow-v2-4-0-plan-pty78.log` 中真实 PTY `plan_approval_restored_after_resume` 1/1 通过：真实 `/plan` 提交、退出、`--continue` 恢复审批、批准后实施一次。final82 中 `defer_ack_retry_after_control_rebuild_keeps_one_terminal_and_never_resamples` 与 `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal` 均为 `ok`。后者核对 durable defer receipt、重建、lost-ACK exact retry、冲突拒绝和唯一 handoff；前者核对 live Plan tool terminal 至多一次且不 resample。 | PTY 冷恢复 fixture 在 AwaitingApproval 时退出，没有覆盖“已 defer、只读保留候选”重启。actor retry 没有注入 Control 写入前/写入中的崩溃；amend 与 idle approval waiter 的 defer 故障链仍缺。 |
| durable reverse request 边界（P-17、P-20 周边） | focused82 `plan_approval_reverse_request_requires_durable_open_and_resolution` 1/1 通过。测试注入 Opened 写失败时断言 ACP 不发送；Resolved 写失败时只留 exact Opened 事实，返回错误且不接纳 Plan handoff。 | 它检查 PlanApproval reverse request 的 durable 生命周期，不检查 decision-status 查询与 writer 并发；不能据此关闭 P-20。 |
| 决定重试与冲突（P-09、P-19） | final82 中 `deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal`、`retained_decisions_are_idempotent_and_reject_identity_conflicts`、`retained_review_rebinds_the_exact_candidate_and_decisions_create_matching_handoffs` 均为 `ok`。同 decision id/内容 exact retry 不增 Control；action 或 feedback 改变冲突；批准后 canonical Execute handoff 只有一个。 | 这些测试按 actor 顺序调用两次决定，不是两个真实 ACP client 同时争抢；没有候选替换与在途未确认决定交叉的竞争。 |
| 审阅仲裁和行为状态（P-12～13、P-18） | final82 中 `active_goal_blocks_retained_plan_review_without_mutation`、`active_workflow_blocks_retained_plan_review_without_mutation`、`retained_plan_review_waits_for_workflow_admission_before_switching` 及 Control 行为独立性测试均为 `ok`。 | 没有输入队首 edit hold/普通前台 owner 与审阅并发、Control 持久失败后 UI 保留评论/候选的运行故障场景，或 Normal/Goal/Workflow 切换后的冷恢复组合。 |

截至本节，最小必要未覆盖集合是：

1. **defer 后重启/接纳前失败**：真实 defer 后退出、只查看候选而不启动 waiter/实施；live submit、amend、idle waiter 各自经接纳前写失败或 ACK 未知后确认原候选与评论保留、旧工具至多终结一次。
2. **并发决定与 status owner**：两个 ACP client 同时提交同一身份的相同及冲突 payload；新增 P-20 手控 ACK handler test 尚待运行，需确认查询未知期间始终保留原 decision identity。现有 direct-call exact retry/conflict 覆盖最终幂等，不覆盖真实并发决定。
3. **artifact 和审阅失败保留**：缺失文件已有 actor 运行证据；hash、尺寸、UTF-8 的 actor 故障用例已新增但未运行。队列/owner 仲裁或 durable Control 写失败时保留候选、评论和未确认状态仍缺。
4. **来源及界面竞争**：同正文新 submission 与旧窗口迟到决定隔离；fork 有 Control sanitation 回归。Import 现在拒绝仍持有 Plan 执行关系的账本镜像；rewind 保持同 owner 的未消费交接。两项新回归待 Shell 运行。慢预览切 session/v1→v2 不串评论；放弃、候选替换、TTL scan 时有新引用仍保留 artifact。

P-01～P-24 的验收矩阵目前仍全部未整体验收。final82 Shell 全套通过关闭了综合回归门禁，但不是未执行 P 场景的证据。tasks 2.0–3.5 各项是复合行为范围；这些 actor/PTy 证据只关闭表中所写的子路径，任务项保持未勾选。