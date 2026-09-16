# 验证记录（2026-09-16）

## 结果

- 新增 Pager 回归 7 项通过，覆盖真实 ACP/Grow handler 的完成工具重放、live 并行乱序、重复快照、合并 Edit 与跨 turn 归属、1300 条历史记录、子会话路由、早到 Hook、隐藏工具与终态 fence。
- `cargo test --locked -p pager --lib app::acp_handler::tests -- --test-threads=4`：328 passed，1 ignored。
- `cargo test --locked -p pager --lib acp::tracker::tests -- --test-threads=4`：159 passed。
- `cargo test --locked -p pager --lib scrollback:: -- --test-threads=4 --quiet`：971 passed。
- `cargo test --locked -p shell --lib hook_snapshot_keeps_tool_identity_and_stays_passive -- --test-threads=1`：1 passed。测试写入合法因果 Timeline，再调用生产 `publish_completed_hook_projections` 两次，确认身份、快照标记、说明、transient 元数据、无持久写入、Timeline 数量不变、rewind 保持。
- 上述 Cargo 命令均使用 `RUST_MIN_STACK=16777216 CARGO_BUILD_JOBS=2`，未并行启动构建。

## 原问题复现

在保留新 DTO 与测试夹具的情况下，临时从 HEAD 取回旧 Hook 展示分支，替换共用处理函数中的展示逻辑：7 项回归中 6 项失败。大历史场景产生 **1301 个条目，预期为 1**；其他失败覆盖工具失去归属、早到事件错挂/独立化、隐藏工具详情丢失、子视图重复行。随后恢复修复并运行以上完整相关模块，全部通过（原有 1 项 ignored 保留）。临时旧逻辑没有保留在工作树。

## 场景核对

| 契约场景 | 验证 |
| --- | --- |
| 对话先恢复、Hook 后到达 | completed_replay_tools_accept_late_snapshots_without_tail_rows，使用服务端实际发送的 precompleted ACP ToolCall |
| 并行工具、重复快照 | live_parallel_hooks_keep_exact_ownership_and_ignore_snapshot_duplicate |
| 合并 Edit、跨 turn 归属 | merged_edits_retain_each_same_phase_hook_occurrence + tracker 既有合并回归 |
| 早到 Hook、加载中实时事件 | live_hook_before_tool_call_waits_for_exact_owner |
| 隐藏工具、无 owner 终态 | hidden_tool_and_turn_fence_preserve_unowned_live_hooks |
| 大历史、失败与说明可见、历史 stop 隔离 | restored_lifecycle_snapshots_are_one_compact_entry_and_keep_details，同时检查 collapsed/expanded 实际渲染文本 |
| 子视图独立归属 | snapshot_hooks_route_through_child_view |
| 只读重复发布 | Shell hook_snapshot_keeps_tool_identity_and_stays_passive |

## 检查与限制

- `git diff --check` 通过。本次修改的 Rust 文件逐个 `rustfmt --edition 2024 --check` 通过；测试 mod.rs 仅添加模块声明，检查其子模块会命中既有 announcements.rs 格式差异。
- `cargo fmt --all -- --check` 未通过：未修改的 announcements、config、mcp、Pager 其他模块及 workspace 等已有格式差异。本次未扩大范围重排这些文件。
- macOS 链接器提示测试二进制的 `__eh_frame` 超过 compact unwind 的 16MB 限制；所有上述测试正常完成。
- 首轮 Pager 编译发现新增测试夹具的两处错误（缺失局部 agent_id、BlockLine 应访问 content.spans），修正后相关测试与模块通过。
- 未取得新截图对应会话 ID，未对该用户会话作现场恢复操作；没有运行真实外部 Hook 命令的端到端 resume 测试。历史 session_end 的红色失败状态是否源于配置/脚本错误，不能仅凭截图断定。
- 每次加载的 Hook 全量查询/传输仍与历史长度相关，独立评估项已登记 backlog。

## 磁盘

起始 target 约 17G，可用约 31 GiB。完成验证后执行 `cargo clean`，移除 85,243 个文件、26.6 GiB 构建产物；清理后可用空间约 **47 GiB**。

## OpenSpec

归档前 `openspec validate --all --strict --no-interactive`：18 passed。已归档为 `2026-09-16-fix-hook-resume-projection`；归档后全量严格校验 17 passed，`openspec validate --archived --no-interactive` 337 passed。归档时仅保留收尾任务 2.2 未勾选，待归档成功及全量复核后完成勾选，归档验证通过。
