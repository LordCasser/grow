# 验证记录

## 2026-09-23 收口价值评估

本 change 已有三个有界切片的可复核报告：model-sampling/attempt admission、已合并响应恢复，以及子 Agent 工具装配。其余 1.2、2.1、2.3、3.2 要求在持续改动的工作树上证明“当前源码的所有相关能力均已审查”，没有可稳定验收的边界；旧切片也已被后续独立行为 change 继续修改。把全仓逐特性审查作为单个长期 active change 不利于判断完成或防止证据失效。

保留现有报告作为其各自基线上的历史审计证据，不声称 2026-09-23 当前树已全量审查。原 1.2、2.1、2.3、3.2 的全仓任务已在 proposal、design 和 [收口记录](reviews/final-disposition-2026-09-23.md) 中明确撤出范围，没有记作完成。后续只对可复现故障、契约冲突或明确风险边界分别立项，按主规范和当前调用方复核并单独验收；已归档的 response replay projection 与 named SSE 修复就是此方式的实例。

## 子 Agent 工具装配与审核语义（2026-09-20）

基线 HEAD `3024dad1ef75a29b1e94a3b5e47083255ad5676e`，工作树已有其他任务修改。本次读取当前实现、调用方、主规范、架构说明、测试源码与真实会话，结论见 [审计报告](reviews/subagent-tool-assembly-2026-09-20.md)。仅更新审计记录与 backlog，没有修改 Rust 或主规范。

已完成：

- 沿 preset → authored snapshot → runtime injection → finalized bridge → child eligibility → permission judgment → one-shot permit → dispatch 核对当前调用路径。
- 真实会话 `01a0bc7a-09bd-7843-9f4a-0e9e6a065f80`：seq 409 的工具结果明确拒绝 `write`；seq 410 `outcome=not_dispatched` 且 `details.dispatched=false`。修正先前仅从 `completed` 推断执行成功的错误。
- `openspec validate --all --strict --no-interactive`：19 passed，0 failed。
- `git diff --check`：通过。

测试源码覆盖核对（本段不代表本轮执行通过）：

| 已有测试 | 证明目标 | 对本次 gap 的边界 |
| --- | --- | --- |
| `subagent_capability::tests::exact_identity_separates_available_locked_and_forbidden` | eligible / initial RWX / 未知 identity 分离 | 使用手工 identity map，不验证默认 preset 的 write 审核策略 |
| `delegation_intersects_rwx_but_retains_exact_transport_ceiling` | mode 求交及 MCP client 精确绑定 | 不验证 native 工具用途策略逐级继承 |
| `subagent_hard_forbidden_bash_rejects_before_permission` | 真实 actor 执行入口在审核前拦截 hard-ineligible | Bash 场景，不覆盖 runtime write 应可审核的期望 |
| `each_child_locked_call_is_judged_once` | locked Bash/MCP 每次调用一次 classifier | 输入已经是 AccessKind，不验证 Write identity 在降级前后保持 |
| `child_calls_inside_capability_fence_skip_auto_judgment_and_audit` | 普通 in-fence 调用省去模型判断 | 没有“RWX 覆盖但整文件覆盖仍必审”的策略 |
| `live_child_judge_receives_primary_context_without_chat_state_pollution` | 主模型 side query、first-party context、主会话不被污染 | 不证明长文件覆盖的完整审批内容被保留 |
| `permit_is_consumed_exactly_once`、`child_authorization_epoch_change_invalidates_permit` | permit 一次性和 epoch 失效 | permit 正确不能补足审核输入中已丢失的语义 |
| `write_tool_maps_to_edit_access`、`tool_name_for_access_pins_canonical_names` | 固定当前 Write→Edit、Edit→search_replace 映射 | 正好固化本次识别出的身份合并；测试通过不等于符合新用途意图 |

覆盖位置：`shell/src/session/subagent_capability.rs`、`shell/src/session/actor/tests/subagent_bash_permission_tests.rs`、`shell/src/session/actor/tests/permission_auto_mode_tests.rs`、`shell/src/session/actor/tool/dispatch.rs`、`workspace/src/permission/{manager,types,prompter}.rs`，均相对于 `crates/codegen/`。

本轮未执行 Rust 测试，未进行真实模型审批或文件竞态故障注入。静态路径和历史会话证据足以确认本报告的语义缺口，但不构成全量授权安全性证明。未来行为修复应补 builder→child→review→dispatch 的真实场景回归；文件并发保护独立验证。现有全仓审查仍未完成，本次不归档该 change，也不勾选涵盖其他主题的 2.1。

## Model sampling / attempt admission（2026-09-16）

基线：`main` at `bd1f5085`（`v2.1.9`）。

已完成的定向动态验证：

- `cargo test --locked -p chat-state model_attempt_usage_retries_exact_event_and_restores_dedup_index`：1 passed，证明同一 attempt 的 Timeline 用量事件可按原身份重试并在冷恢复后保持去重索引。
- `cargo test --locked -p sampler malformed_completed_tool_arguments_recover_with_bounded_accounted_attempts`：目标测试 1 passed；其他测试二进制均为 0 selected，证明 malformed completed tool candidate 在已结算、有限 attempt 路径中恢复。

补充完成的阶段验证：

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell sampling_attempt_mixed_clients_buffer_unknown_until_accept_and_drop_discard -- --test-threads=1`：目标测试 1 passed，其他测试二进制 0 selected。支持 attempt lifecycle 的客户端实时接收 provisional candidate；不支持的客户端在 Accepted 前不接收候选、Discarded 后不接收候选，并保留 irreversible 实时路径。
- `openspec validate --all --strict --no-interactive`：18 passed，0 failed。
- `git diff --check`：通过。
- 构建后磁盘检查：文件系统剩余 106 GiB，`target/` 为 22 GiB。为复用当前编译产物完成紧邻的 sampler shutdown 修复，本阶段暂不清理；修复验证结束后执行 `cargo clean` 并记录回收结果。

未动态覆盖：

- sampler shutdown 在 evidence/usage ACK 被阻塞时的故障注入；
- duplicate live `RequestId` 的双任务、旧任务清理和 settlement identity 冲突；
- `PushResponseDurably` 已提交但 reply 丢失后的 completion recovery；
- Timeline admission 后、Accepted cache 发布前的 kill-point 冷恢复；
- `updates.jsonl` 接纳候选写入失败后的 Timeline/UI 对账。

这些缺口不被格式校验、局部 helper 测试或历史测试结果替代。

## 已合并响应恢复提交复核（2026-09-17）

基线：`git fetch origin` 后，`main` 与 `origin/main` 同为 `c17b085d`（v2.1.10）。审查结论及局部复现见 [响应恢复复核](reviews/response-recovery-merged-2026-09-17.md)。只增加本 change 的审计记录，没有修改运行时源码。

以下 Cargo 命令统一使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`，测试参数为 `-- --test-threads=1`：

| 命令（省略统一环境变量和尾部参数） | 结果 |
| --- | --- |
| `cargo test --locked -p chat-state admitted_response_view` | 4 passed |
| `cargo test --locked -p chat-state response_admission` | 4 passed |
| `cargo test --locked -p shell --lib response_projection` | 19 passed |
| `cargo test --locked -p shell --lib 'session::persistence::durable_update_tests::'` | 16 passed |
| `cargo test --locked -p shell --lib 'session::storage::jsonl::tests::'` | 123 passed |
| `cargo test --locked -p shell --lib provider_completion_respects_schema_refusal_and_goal_budget_terminals` | 1 passed |
| `cargo test --locked -p shell --lib context_window_exceeded_triggers_compaction` | 1 passed |
| `cargo test --locked -p shell --lib pause_turn_resend` | 1 passed |
| `cargo test --locked -p shell --lib sampling_candidate_is_scoped_and_dropped_until_durable_admission` | 1 passed |
| `cargo test --locked -p shell --lib response_admission` | 1 passed |
| `cargo test --locked -p shell --lib exact_projection` | 5 passed |
| `cargo test --locked -p shell --lib barrier` | 4 passed |

共 180 次测试执行通过；filter 有交集，不能称为 180 个独立测试。构建仅出现已有的 macOS `__eh_frame` 链接 warning。

补充验证：

- 隔离 rustc harness 直接链接当前 ChatState，并包含当前生产 projection 模块：复现重复 rewind、fork seed/projection 归属不一致和旧 Timeline/新 cache 的 reconciliation 丢行。它们是边界级复现，不是完整 ACP 或终端端到端测试。
- `directory_barrier_failure_is_retried_even_after_file_exists`、`file_barrier_error_propagates` 通过，证明底层 seam 会返回同步失败；尚未将同步失败注入新的 exact-projection 高层入口，不能以这两个通过结果否定 R4。
- `openspec validate --all --strict --no-interactive`：19 passed，0 failed。
- `git diff --check`：通过。
- `session_fork_replay_memory` 需要额外 `test-support` feature，触发较大增量构建后中止；没有执行完成，不计为通过。没有运行全 workspace tests。
- 磁盘检查时 `target/` 为 23 GiB，文件系统剩余约 92 GiB。保留共享编译产物，没有执行 `cargo clean`，避免删除其他任务正在复用的产物。

本次复核没有关闭全仓架构审查，也没有把未归档的 `reconcile-response-replay-projection` tasks 标为实现完成。四项修复应单独进入行为 change。
