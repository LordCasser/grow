# 实现证据与验证记录

2026-09-10。源码根为 `crates/codegen/`。这是本 change 的实施记录，不以模拟请求证明实际供应商事故。

## 根因与范围

报错来自 Chat parser：缺少 choice `finish_reason` 时调用 `protocol_failure`，后者包装成不可重试的 `Serialization`。因此原本可能由截流造成的失败在 Grow 内被当成确定性解析错误直接终止。Responses 缺 terminal、Messages 缺完整结束序列存在同类分类问题。真实事故的 provider、HTTP 尾帧、代理日志和当时客户端输出状态未取得；不能断定哪个远端组件切断了流。

同时核对发现三个恢复边界缺口：内部失败 attempt 没有统一进入普通账本/子任务输出预算；streaming 通知和合并缺少 attempt 身份；sampler 与 session 修复可以各自重新计数。实现沿既有 request task、Timeline、Goal window 和通知链收敛这些边界。

两个库存文件保留实施前快照。Atlas 已 project(open) 并做 scoped 查询；部分符号查询 unresolved 或因共享数据库忙锁失败，最终调用关系以实际源码为准。没有把空 callers 当成无调用方。

## 最终调用路径

| 路径 | 实现及能力 |
| --- | --- |
| 主 agent | `run_turn_via_sampler` 提交 accounted request；每次准入检查 owner 并重算输出 grant；结算普通账本及可选 Goal，成功接纳只更新 context anchor |
| 子 agent | 从父 `ToolContext` 继承交付能力，使用同一 sampler 路径；逐 attempt 扣自身输出预算，完成后沿既有 parent fold 汇总 |
| Pager Fullscreen/Inline | 初始化明确声明 `samplingAttemptLifecycle`，root/child handler 都处理 Started/Discarded/Accepted；仅撤回归属于失败 attempt 的条目 |
| Minimal / headless / 未声明能力的发起方 | 默认 Irreversible，包含 ResponseStarted/signature 等已发布帧后禁止透明重新采样；未修改标准 stdout 协议 |
| leader 附加观察者 | 对明确 Retractable 的会话，未知客户端只在 Accepted 后收到暂存候选；pending load 使用原 eventId 做 replay cutoff；不保留已接纳候选历史 |
| Sideband / auxiliary | 继续使用 `conversation_collect` 的单次收集和既有 Sideband attempt/evidence/Goal owner；不继承主步骤的输出预览，不新增自动重试。`/btw` 仍只按既有 overload 条件和 Sideband 预算恢复 |

普通账本的捕获坐标使用 `current_prompt_index()`；旧 `get_prompt_index()` 返回下一个空闲位置，不能用作当前 attempt 的归属。已知消费、费用和时长被冻结到 attempt，未知消费保留 incomplete。历史 settlement 只重建去重索引，不改变现有恢复后普通统计的 live-only 口径。

## 场景与证据对应

| 契约场景 | 回归 / 检查边界 |
| --- | --- |
| 三协议缺结束证据、半截工具、idle timeout | `stream::{chat_completions,responses,messages}::tests`；缺结束标记使用 IncompleteStream，完整 JSON 不能代替协议完成 |
| 已有身份冲突后 EOF | Chat `deterministic_protocol_failures_are_not_downgraded_to_eof`；Responses `conflicting_tool_stream_evidence_never_completes`；Messages 冲突/完整性矩阵，冲突优先 |
| 完整但非法工具 JSON及有效 sibling | 三协议 parser 原子拒收测试；`malformed_completed_tool_arguments_recover_with_bounded_accounted_attempts`；shell `protocol_invalid_tools_do_not_execute_and_the_next_turn_recovers` 真实 turn loop，零工具事件且下一用户 turn 可用 |
| 合法 length/context-window/pause/refusal | `drive_l2_*_outcome`、shell `truncation_recovery_tests` 和 Messages actor refusal 回归；保留既有语义终态 |
| 有效正文后缺 usage / 尾部 transport error | Chat `completed_chat_tail_is_bounded_without_inventing_usage`、`completed_candidate_survives_transport_failure_in_usage_tail`；Responses terminal 不继续消费尾流 |
| typed source / 本地生产者消失 | `synthesize_*`、`synthesize_without_typed_source_fails_closed`、事件类型投影及 collect 终态测试；内部缺 source 是 Lifecycle |
| 不明远端工具副作用 | `provider_operation_then_eof_is_not_replayed_even_with_retractable_output`，真实 HTTP 返回 provider tool operation 后 EOF；一次请求，保留原断流错误 |
| 同输入重试及逻辑总额 | `real_http_retry_recomputes_output_grant_and_unknown_spend_closes_admission` 比较两次请求输入相同、上限缩小；`shared_recovery_budget_blocks_second_real_http_attempt`；`repairs_share_count_and_cannot_extend_deadline` 验证重提交不能重置次数/期限 |
| 关闭恢复、服务端 veto、429、doom 分类 | retry 纯函数矩阵和 sampler `test_actor`；doom/非法生成都消耗同一总额，显式 0 只允许初次调用 |
| request/response/retry evidence ACK | `request_evidence_ack_gates_wire_for_every_backend`、`response_and_retry_evidence_ack_gate_resampling_and_keep_raw_error`；暂停 ACK 时无下一 HTTP，请求/证据写入失败停止 |
| 用量 ACK、重复、丢失确认 | chat-state `model_attempt_usage_*`；使用同一 immutable event 重试，ACK 前不 fold，重复 key/payload 不重复收费、冲突失败；恢复后去重 |
| Goal root/child 与无 Goal 普通预算 | shell `record_response_token_usage_tests` 真实 `sampling_usage_sink`，root/descendant 通过 Goal command ACK；scope=None 已知 4+3 各计一次、重复不扣预算；未知使精确输出 grant 归零 |
| 部分结算失败 | sampler `usage_settlement_failure_emits_terminal_before_completion`、非法工具故障矩阵的 usage_failure；Goal `failed_goal_settlement_retains_attempt_and_retries_exactly_once`；普通失败仍归还/结算已捕获 Goal lease，新 provider 停止 |
| cancel、backoff、owner失效 | `cancel_during_admission_never_starts_provider_for_every_backend`、`cancel_during_stream_open_settles_the_first_poll_scope_for_every_backend`、`drive_l2_buffered_terminal_outranks_simultaneous_cancel_and_preserves_usage`、`retry_sleep_*`；既有 Goal epoch fence、parent-message steering 回归 |
| 预览各类帧与接纳 | shell `sampling_candidate_is_scoped_and_dropped_until_durable_admission` 覆盖 ResponseStarted、text、reasoning、tool、signature；Completed 后仍 pending，guard drop 发 Discarded；生产路径仅在 `push_response_durably` ACK 后发 Accepted |
| merge、迟到事件、root/child | `update_chunk_merge::tests` 覆盖跨 attempt 拒绝合并、同 attempt 保留身份/eventId；Pager `sampling` 测试经真实 handler 及 tracker 覆盖撤回、迟到帧、未标记交错内容、已接纳水位 |
| 渲染模式与重连 | `sampling_attempt_lifecycle_matches_effective_screen_mode` 覆盖实际三种模式；`plan_reconnect_load_omits_cursor_for_unconfirmed_root_or_child_preview`、`pending_preview_forces_empty_full_replay_but_failed_load_restores_stash`、`child_preview_is_replaced_for_replay_and_restored_on_failure` 覆盖失败恢复、连续重连、子视图身份和独立内容 |
| 混合消费者及 load overlap | leader `sampling_attempt_mixed_clients_buffer_unknown_until_accept_and_drop_discard`、`accepted_sampling_candidate_flushes_pending_load_once`；discard零候选输出、irreversible保持实时、重载不重复补发；`driver_only_request_during_sampling_candidate_is_not_buffered` 验证文件/终端反向请求继续发给原 driver，不进入观察者缓冲 |
| 响应接纳 ACK / quarantine | 复用 chat-state `response_repair_waits_for_both_durable_acknowledgements` 及 immutable Timeline append 幂等测试；shell quarantine 拒绝整个 sibling 集；接纳失败退出当前逻辑采样，不进入 repair/resubmit |

这些测试按所有者边界组合验证，并非一个测试启动全部 UI、三个真实供应商和进程崩溃。没有运行真实供应商收费实验或 kill -9 全系统实验；进程终止语义由 durable ACK、精确事件重放、恢复去重和候选回放隔离验证，不能据此承诺远端 exactly-once。

## 回放审计追加

实时撤回测试通过后，审计发现 ACP 预览原先仍写入 `updates.jsonl`，而 Grow 废弃边界不参与重载。这会让已废弃文本在重载后出现。实现已在既有持久化投影所有者内暂存候选，Accepted 后才写入回放缓存，Discarded/未接纳关闭时丢弃；Timeline 继续是接纳事实源。

本轮在 `SessionPersistence` 内增加短生命周期候选窗口：带完整
`samplingRequestId`/`samplingAttempt` 的 ACP 文本和 reasoning 只有在同 key 的
`Accepted` 后写入既有 `updates.jsonl`；`Discarded`、`Stop` 和未接纳关闭只保留
窗口内未标记的交织 ACP，并按原队列顺序写出。候选窗口不保存 Grow 边界，也不累积
历史 request 集合。新增临时存储回归覆盖 discard 后 accepted retry、未接纳 shutdown、
untagged interleaving，以及 accepted 候选与交织通知的 eventId/replay 顺序。

首次局部命令因共享源码中新字段和 moved `backend` 编译错误未执行测试；主线程修复后，真实存储五条回归均在 shell 库测试中通过：
`replay_keeps_only_the_accepted_retry`、`unaccepted_sampling_candidate_is_absent_after_shutdown`、
`untagged_interleaving_survives_sampling_discard`、`untagged_interleaving_survives_channel_close`、
`accepted_sampling_and_untagged_interleaving_keep_event_order`。

新增真实 session turn-loop 回归
`retractable_stream_retry_accepts_only_the_second_candidate_and_bills_both` 也已通过：
HTTP 第一轮发出文本后缺 finish_reason，第二轮完整成功；实际请求两次、消费各结算一次，
有效历史仅包含第二轮，通知顺序为 Started(1) → Discarded(1) → Started(2) → Accepted(2)。

输出能力审计另确认 Minimal 会将已经 finalized 的 reasoning 等块写进 terminal native scrollback，后续 tracker 删除无法撤回。因此 capability 必须在 terminal probe 返回实际 ScreenMode 后设置；Fullscreen/Inline 明确声明支持，Minimal 保守声明不支持，不依据 clientType 猜测。新增 `sampling_attempt_lifecycle_matches_effective_screen_mode` 回归。

Pager reconnect 审计确认 `plan_reconnect_load` 会把 `last_seen_event_id` 作为
`session/load._meta.cursor`，而 `apply_reload_outcome` 在无 replay 时把 live tail
合回旧 stash。若 stash 的 root 或 descendant tracker 仍有未确认 sampling preview，
现已省略 cursor，并在 reload 窗口记录 full-replay 标志；即使 canonical replay 为空，
成功也不会把旧 preview 合回，失败仍完整恢复 stash。新增 root/child cursor 计划及空
full-replay/失败恢复回归。进一步审计发现子视图会被复用，因此在既有 root reload 窗口暂存受影响子视图的 transcript/tracker，保留控制对象身份；成功只丢弃旧候选，独立历史和新尾部仍保留；失败恢复完整条目归属，连续重连不会失去撤回能力。加载成功通知若被新一轮 live preview 抢先到达，通用 turn cleanup 同样需要保留新候选的条目归属；`live_preview_before_reload_completion_remains_retractable` 与 `failed_reload_preserves_entry_ownership_for_later_discard` 专门验证后到的 Discard 仍能只移除候选。其余回归为 `child_preview_is_replaced_for_replay_and_restored_on_failure`，最终统一执行 Pager 库测试。

## 执行记录

- 首轮全量 surface 测试：chat-state 472 passed / 1 ignored；Pager 7178 passed / 10 ignored；shell 3782 passed / 2 failed / 3 ignored。失败暴露旧分类断言及 Chat 中间 usage 不能冒充终态消费的约束，保留场景并修复实现/断言后重跑。sampler integration 的旧事件/期限断言也已按共享总额与新增 lifecycle 边界更新，后续最终回归通过。
- `RUST_MIN_STACK=16777216 cargo test --locked -p sampler -p sampling-types -- --test-threads=4`：sampler 库 240 passed、集成 33 passed；sampling-types 库 270 passed；doc tests 无失败。日志 `/tmp/grow-recovery-core-final.log`，共 543 passed。
- `RUST_MIN_STACK=16777216 cargo test --locked --lib -p shell -p chat-state -p pager -- --test-threads=4`：chat-state 473 passed / 1 ignored；Pager 7179 passed / 10 ignored；shell 3790 passed / 3 ignored。包含持久化五条回归与真实 turn-loop 两次调用测试。日志 `/tmp/grow-recovery-surface-final.log`。
- 上述结果合计 11985 passed / 14 ignored。后续新增 leader 驱动请求隔离、定向 replay 以及 Pager reconnect 边界追加全量和定向回归；结果见下方最终验证。
- `cargo check --locked -p cli` 已通过首轮；最终源码补检和 OpenSpec 校验见下方最终验证。
- 定向 usage/Pager/leader 测试是全量的子集，不重复累计。验证使用同一共享工作树，保留同时进行的 Goal usage 和异步 compaction 改动；没有为使测试通过删除错误场景。

## 最终验证

2026-09-10，在最终修补后执行：

| 命令 / 范围 | 结果 |
| --- | --- |
| sampler + sampling-types 库/集成/doc tests | 543 passed，0 failed；`/tmp/grow-recovery-core-final.log` |
| shell + chat-state + Pager 库测试全量补跑 | chat-state 473 passed / 1 ignored；Pager 7183 passed / 10 ignored；shell 3791 passed / 3 ignored；`/tmp/grow-recovery-surface-verified.log` |
| 最后两处 reload cleanup 修补后，以相同 package/features 执行 `cargo test --locked --lib -p shell -p chat-state -p pager reconnect -- --test-threads=4` | Pager 70 passed / 1 ignored；shell 15 passed；0 failed。包含新加的两条 preview 与 load completion 时序测试；`/tmp/grow-recovery-reconnect-verified.log` |
| `cargo check --locked -p cli` | 通过，涵盖 pager-minimal 最终 capability 接线；`/tmp/grow-recovery-cli-verified.log` |
| 受修改文件 scoped rustfmt、`git diff --check` | 通过 |
| `openspec validate --all --strict --no-interactive`（归档前） | 18 passed，0 failed；`/tmp/grow-recovery-spec-final.log` |

全量结果为 11990 passed / 14 ignored；最后的 85 项重连专项含 2 条新增测试，其余是全量子集，不重复累计。没有删除失败场景来降低验证范围。链接测试二进制时出现现有大体积 `__eh_frame` 的系统 linker 提示；最终 CLI check 无告警/错误。

归档已完成：`openspec archive unify-sampling-attempt-recovery --yes`，生成
`2026-09-10-unify-sampling-attempt-recovery` 并合入三个正式规范。
归档后 `openspec validate --all --strict --no-interactive` 为 17 passed / 0 failed；
`openspec validate --archived --no-interactive` 为 305 passed / 0 failed。
归档第一次历史校验只指出作为最后一步尚未勾选的 5.3 任务；归档动作完成后勾选并重跑通过。
相应日志为 `/tmp/grow-recovery-spec-postarchive.log` 和 `/tmp/grow-recovery-archive-verified.log`。
全部 17 项任务完成；未创建 Git commit。

## 未混入的范围

不实现供应商切换、远端操作幂等协议、跨进程自动续跑、不改变 Sideband 统计产品口径，也不迁移全部配置字段名称。通用 body/缓冲资源上限与辅助调用账本统一属于独立架构债务，登记 backlog。
