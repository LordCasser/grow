# 验证记录

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
