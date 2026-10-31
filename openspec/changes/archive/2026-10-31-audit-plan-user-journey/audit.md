# Plan 用户旅程审计

## 范围与方法

从进入 Plan 到完成/放弃，逐项核对后端 phase、持久决定、前端焦点、输入去向和实际模型上下文。生产代码审阅、内存/存储故障注入测试与真实 PTY 分层使用；PTY 使用确定性 mock provider，不代表真实模型必然遵守计划，也不能证明所有终端/外部 ACP client 无缺陷。实际执行结果见 verification.md。

所有源码路径相对 `crates/codegen/`。已有 Plan 专项回归保留，不用重复测试数量代替场景检查。

| 旅程/分支 | 不变量与证据/回归 |
| --- | --- |
| dashboard /plan、现有会话 /plan、/plan 带描述、Behavior picker | `pager/src/slash/commands/plan.rs` 分别选择或先切换再发消息；`slash_plan_no_args_not_in_plan_enters_plan_mode`、`slash_plan_with_args_not_in_plan_enters_and_sends_prompt`；`picker_can_enter_plan_from_a_regular_normal_foreground`。dashboard 的 staged 行为不应写成所有会话都等下次 prompt 才进入。 |
| Drafting 调研与权限 | `shell/src/session/actor/tool/mod.rs` 的 drafting Bash/edit/MCP gate；`behavior.rs` 的 mutation capability tests。批准前即使 always-approve 也不能执行；允许的只读 MCP 仍受普通权限约束。 |
| 访谈正常答题、自由文本、Chat about this、Skip interview | `pager/src/views/question_view.rs` 的 partial answers 选择/未选择文本测试；`agent_view/interactions.rs` 的 `enter_on_chat_about_this_sends_partial_answers`、`enter_on_skip_interview_sends_current_freeform_as_other`；tools ask_user_question 的格式化和序列化回归。 |
| submit 空白、正文持久化失败、Control 写失败 | `plan_control_results_preserve_lifecycle_messages`、`failed_control_write_keeps_submitted_plan_candidate_unaccepted`。只有已持久候选才能打开审批；失败不能发接受回执。 |
| Preview / Prompt / Commenting、保存/取消、收起/重开 | 共用 `agent_view/{plan,viewer,input}.rs`；`empty_enter_on_revise_prompt_does_not_approve`、`a_on_empty_revise_prompt_approves`、`a_with_pending_comments_does_not_approve`。Esc 收起保留审批，Tab 重开；批注取消恢复既有修订草稿。 |
| 窄屏、鼠标动作、Minimal | 本轮发现并修复：窄屏整个 footer 隐藏；Minimal 普通/retained preview 不可见却截获输入；Commenting 无编辑框；空 Enter 与 q/Esc 提示不实。新增渲染/交互回归。 |
| 初次修订、连续修订、批注与自由文本一起提交 | `revision_context_preserves_current_feedback_across_restore_and_resubmission`、`plan_revision_feedback_reaches_durable_model_context_and_replay`；新增真实 PTY 两次修订到完成，检查后续 provider 请求中的反馈。 |
| approve 无批注/带批注 | `approving_with_comments_carries_feedback_in_the_approval_response`；`approval_handoff_is_bound_to_the_frozen_artifact_and_feedback_is_projected_once`。批准完整提交身份，而非当前任意正文。 |
| 决定确认早到查询、重复请求、新提交、迟到回执 | `early_unconfirmed_status_then_exact_mode_receipt_closes_plan`、`mode_update_receipt_is_required_and_checked_against_current_identity`、`new_submission_retires_pending_decision_but_duplicate_keeps_it_and_comments`、`duplicate_same_submission_keeps_pending_until_its_receipt_closes_replacement_view`；`accepted_plan_decisions_publish_the_committed_receipt`。 |
| Executing 的 handoff 与模型续跑 | `plan_control_live_handoff_is_consumed_between_steps_before_resampling`、`plan_live_handoff_recheck_rejects_dismissal_and_other_turn_consumption`、`plan_control_live_handoff_uses_submission_id_across_plan_cycles`。 |
| amend 替代计划、拒绝修改、再次批准、延期 | `amendment_rejection_stays_read_only_for_revision`、`deferring_an_amendment_clears_the_previous_execute_handoff`、`retained_review_rebinds_the_exact_candidate_and_decisions_create_matching_handoffs`。Amending 恢复只读，不沿用旧执行授权。 |
| l 延后、/view-plan、r Review | `idle_resume_receiving_deferred_keeps_plan_draft_without_starting_a_turn`、`deferred_plan_decision_is_recovered_and_reapproved_from_control_before_tool_terminal`；PTY `deferred_plan_reviewed_after_resume`。查看不等于批准，retained Review 不绕过现有 owner。 |
| 活跃 Goal/Workflow、queued input、held control 阻止 Review | `active_goal_blocks_retained_plan_review_without_mutation`、`active_workflow_blocks_retained_plan_review_without_mutation`、`retained_plan_review_rejects_foreground_queued_input_and_held_control`、`retained_plan_review_waits_for_workflow_admission_before_switching`。 |
| 退出进程/恢复待审批、延期恢复、Minimal scrollback | PTY `plan_approval_restored_after_resume`、`deferred_plan_reviewed_after_resume`、`minimal_parked_plan_commits_to_scrollback`、`minimal_parked_plan_survives_quit`。 |
| q 放弃、工具 cancel、切换 Behavior | `plan_control_results_preserve_lifecycle_messages`、`behavior_switch_preserves_deferred_candidate_and_cold_restore_identity`、`failed_behavior_selection_does_not_change_live_plan_state`。q 不应被提示为 keep planning；Esc 不能退出协议。 |
| complete、报告写失败、完成后详情/恢复 | `plan_completion_storage_and_control_failures_preserve_execution`、`completed_plan_viewer_switches_read_only_tabs_by_keyboard_and_mouse`、`completed_plan_stays_inspectable_until_a_behavior_change_applies`、`completed_plan_clears_only_after_successful_manual_behavior_selection`。正文和完整 report 分页只读，正常总结单独展示。 |
| 普通 preview 批注、忙时队列 | `send_casual_plan_comments` 返回普通 `Action::SendPrompt`。本轮只明确普通聊天/排队提示，不暗改为审批或中断；完成详情不能评论。 |
| stale 决定、并发决定、ACK 不明确、候选丢失/损坏 | `late_v1_decision_cannot_mutate_replacement_v2_submission`、`concurrent_conflicting_plan_decisions_commit_at_most_one_handoff`、`ambiguous_defer_control_ack_recovers_receipt_without_a_second_write`、`defer_ack_retry_after_control_rebuild_keeps_one_terminal_and_never_resamples`、missing/invalid artifact 回归。 |

## 未混入的边界观察

- 非 bundled ACP client 返回 malformed approval 后，待审批状态仍保留，但同连接没有自动重挂；再次 session/load/reconnect 可恢复。Bundled Pager 通过 typed struct 构造响应，未找到正常按键可复现入口；作为外部客户端恢复可用性债务记录，不加自动重试循环。
- 恢复 reconciliation 后再次读 artifact 失败的 fallback 使用无条件退出；若不可变 artifact 被外部移除且同时切换候选，存在旧恢复任务影响新状态的风险。正常 artifact 生命周期无删除入口，本次不按假设加新防御分支，登记独立故障注入验证。
