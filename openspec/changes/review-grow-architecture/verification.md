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
