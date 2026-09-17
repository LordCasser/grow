# 验证记录

基线：`c17b085d`（v2.1.10）。修复范围来自 [响应恢复审计](../../review-grow-architecture/reviews/response-recovery-merged-2026-09-17.md) 的 R1–R4。

## 故障回归

| 边界 | 旧实现 | 修复后覆盖 |
| --- | --- | --- |
| R1 重复 rewind | 新回归在第二次 rewind 后得到空 admission，预期保留原 admission | 连续三次 rewind；普通及 compaction 前缀；每次 cold fold；原始 leaf 坐标保持，切出响应排除 |
| R2 fork 历史 | 审计 harness 中父 projection 复制后 replay 从 1 变为 0；本次三项 storage 回归在修复后加入，未另行回退旧实现执行 | 真实 `copy_session`；父投影去重和缺失重建；quarantine 排除；typed/raw/direct replay 一致；展开后 summary 行数 |
| R3 resident snapshot | 新回归执行 1 项并失败：初次 snapshot offset 为 1089，期望未来 projection 开始前的 195 | 初次读取前完成的未来 response 留给 delta；初次合成后落盘的 response 去重；带/不带 cursor；空行下物理 byte offset 和 thought/text/tool 顺序 |
| R4 exact durable ACK | `exact_` filter 在旧 ACK 实现中新增 4 项均失败，其余 66 项通过 | production append 路径注入 file/directory sync 失败；持续失败不 ACK；恢复后 exact retry 不重复追加；projection 与独立 ACP 行 |

## 执行结果

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p chat-state --lib -- --test-threads=4`：498 passed、0 failed、1 ignored。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib -- --test-threads=4`：3847 passed、0 failed、3 ignored。新增 resident、fork、exact sync 故障回归全部包含在本次全量执行中。
- Changed-file rustfmt（`skip_children=true`）与新增回归代码段的独立格式检查通过；已有 `rustfmt::skip` 模块及 MVP tests 的无关历史格式保持。
- `git diff --check` 通过。
- 归档前 `openspec validate --all --strict --no-interactive`：20 passed、0 failed。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo check --locked -p cli`：通过，验证非 test 配置及上层调用方编译。
- `openspec archive fix-response-recovery-boundaries --yes`：已归档，向主规范增加四项要求；其他进行中 change 保持。
- 归档后 `openspec validate --all --strict --no-interactive`：19 passed、0 failed。
- 首次 archive 校验因归档任务 2.3 自身尚未勾选而报告 343 passed、1 failed；归档完成后补齐该任务，再执行 `openspec validate --archived --no-interactive`：344 passed、0 failed。

## 场景核对

- Rewind twice / compacted history：`admitted_response_view_survives_repeated_rewind_and_cold_fold` 覆盖三轮与两种 compaction 分支；同时检查 Surface、admission、canonical leaf 和 unloaded 状态。Shell 全量也覆盖既有 response planner 和 fork rewind/truncation。
- Fork text/reasoning / discarded history：`fork_reconciles_and_expands_response_projection_into_child_history`、`fork_synthesizes_missing_projection_without_child_admission`、`fork_drops_quarantined_projection_and_parent_candidate_metadata`，加既有 fork_filter 与 rewind/truncation 测试。
- Resident authority/cache 窗口 / later physical projection：`resident_response_snapshot_delivers_future_projection_once_before_tools` 的四种组合（较晚 admission/较晚 cache × 有/无 cursor）。已有完整 JSONL framing 测试继续通过。
- Exact sync failure / recovered retry：projection 与 ACP 各有 file/directory 两项测试。每项验证写后失败、恢复同步、已有 exact 再次同步失败、再恢复，以及始终只有一条物理记录；既有 payload conflict 和 post-commit bookkeeping 测试继续通过。

## 验证边界

- Resident 测试用确定性写入时序驱动实际初次 replay / delta / gateway 发送路径，不依赖随机线程时序；没有运行外部 provider 或真实 TUI 端到端重连。
- 持久化回归注入真实同步调用之前的错误，验证 ACK 控制流及恢复重试；不模拟操作系统掉电后的磁盘介质状态。
- `reconcile-response-replay-projection` 和 `review-grow-architecture` 仍是独立进行中的 change；本次不将其未完成任务标为完成。
