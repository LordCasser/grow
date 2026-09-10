# 验证记录

## 事故与验证边界

原始 Timeline/request/response 的核对见 [investigation.md](investigation.md)。本次不重放真实 session，不再次运行其文件写入工具，也不调用线上模型做未受控 A/B。Mock 只验证 Grow 的输入和生命周期，不证明模型必然给出工具调用。

## 场景映射

| 契约场景 | 验证 |
| --- | --- |
| 旧任务完成，新任务仍在执行 | fixture 摘要明确旧任务完成/等待用户；所有提交场景验证摘要范围说明。 |
| 工具结果后的同 turn 续接 | 新测试 `async_compaction_between_steps_continues_the_latest_task_once` 用 blocked provider + step gate + Sideband activity drain 固定发布时序；核对实际 wire 中最新工具事实早于提示。 |
| 恰好一次、synthetic 身份 | 新测试验证 `AutoContinue` 恰好一项、无 PermissionEvidence/prompt index、位于保留结果之后，只有一个 TurnEnded，HTTP 请求仍为原有三次（摘要、工具响应、最终响应）。请求组装没有追加消息职责，重复构建仍读取同一持久化提示。 |
| 未就绪、失败、取消、控制切换 | 既有异步场景新增 AutoContinue=0 断言；后台范围和迟到结果测试保留。 |
| 完成边界发布 | publish 场景在原 turn 完成后发布摘要，AutoContinue=0，不新增请求。 |
| 新 turn 首 Step 发布 | cross_turn 场景保留 Recall 刷新，AutoContinue=0。 |
| 合法最终回复 | 新测试的下一响应为合法 end_turn、无工具调用，正常结束且没有额外调用。 |
| 续接持久化失败 | 源码核对：`push_user_message_durably(...).await?` 在同一 owner/gate 内先于 StepStarted 和 provider dispatch；ChatState 既有 durable append 的 uncertain/permanent failure 测试验证 ACK 边界。未新增专门在 Shell 续接单条写入处注入失败的测试。 |

## 已执行

- `openspec validate --all --strict --no-interactive`：19 passed、0 failed（归档前）。
- `cargo test --locked --lib -p shell async_compaction --no-run`：成功编译，产物 `target/debug/deps/shell-611fc7c908fcf979`。保留已有 sampler unused import 与 linker 大 `__eh_frame` warning。
- 上述产物执行 `async_compaction --test-threads=4`：17 passed、0 failed。
- 上述产物执行 `compaction_pre_prune_tests --test-threads=4`：25 passed、0 failed（包含上述17项，不重复相加）。
- 上述产物执行 `truncation_recovery_tests --test-threads=4`：18 passed、1 failed；单独重跑失败用例仍失败，见下文。
- `cargo test --locked --lib -p chat-state durable_user_message_retries_an_uncertain_persistence_failure -- --test-threads=4`：1 passed；生成 `target/debug/deps/chat_state-98c2a96c9303bf85`。
- ChatState 产物执行 `persistence_failure`：4 passed（包含上一项）；`push_user_message_durably_waits_for_timeline_commit`、`partial_compaction_preserves_unselected_surface_identity`、`restored_session_starts_with_portable_history_only`：各1项通过。共7项不重复测试，覆盖 ACK、永久失败关闭、范围 identity 和恢复无 native。
- `git diff --check`：本次触及文件通过。

## 失败及限制

1. 首次编译等待共享 artifact 锁后出现 `libmemchr-...rmeta: No such file or directory`。工作区同时有其他 Cargo 构建；重新编译成功，没有更改构建配置、清理他人产物或终止他人进程。
2. 测试编写期间曾直接执行先前生成的测试二进制，新增场景因测试提前消费 completed notification 而失败。修正 test-only notification drain 后重新编译，当前17/25项全部通过；没有删除失败场景。
3. `protocol_invalid_tools_do_not_execute_and_the_next_turn_recovers` 在 `truncation_recovery_tests.rs:759` 期待错误文本含 `protocol:`，实际是 `incomplete ChatCompletions stream: stream ended without a choice finish_reason`。fixture 发送无 finish_reason 的 Chat 工具流；该用例没有压缩，失败在首次错误分类断言，未进入本次提示分支。工作区正在实施 `unify-sampling-attempt-recovery` 并改变该错误分类，故保留此冲突供该 change 核对；没有以未验证的干净基线结果声称全套回归通过。
4. 未执行全 workspace 测试、线上模型 A/B 或替换用户正在运行的 Grow 二进制。仅修复可证明的任务输入衔接缺口，结构化 portable 工具历史已登记 backlog。

## 归档结果

- `openspec archive preserve-async-compaction-task-continuity --yes`：归档并合入 context-compaction 的2项新增要求。归档时仅归档操作自身任务未勾选，完成后在归档目录勾选。
- 归档后 `openspec validate --all --strict --no-interactive`：18 passed、0 failed。
- `openspec validate --archived --no-interactive`：304 passed、0 failed，包含本 change。
- 未提交 Git、未安装或替换正在运行的 Grow；代码和规范修改保留在当前工作区。
