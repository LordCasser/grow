# 验证记录

## 场景映射

| 契约场景 | 验证 |
| --- | --- |
| 异步提交后等待 request projection | `async_compaction_commits_at_boundary_and_notifies_after_request_projection` 在后台结果提交后先断言没有完成通知及没有第二个后台任务；下一请求在 provider 屏障处停住后，断言恰好一条通知。 |
| `tokens_after` 使用已物化请求 | 同一测试在 provider 响应前读取 ChatState projected tokens，并与通知 payload 精确比较，避免 provider usage anchor 掩盖投影差异。 |
| 后继请求前保持 pending | `async_compaction_goal_concurrency_is_exactly_charged` 的无后继请求分支断言通知仍 pending；between-step 与 cross-turn 场景验证各自下一请求会消费它。 |
| 恰好一次及防重入 | publish 场景重复调用发布入口后断言没有第二条通知，并在 pending 时重复边界检查，断言不会启动另一轮后台压缩。 |
| 后续 replacement 取代旧展示 | `pre_prune_insufficient_projection_runs_summary`、`pre_prune_under_sticky_suppress_clears_it_on_success`、`pre_prune_error_fails_open_to_summary` 分别覆盖同步摘要、同步 prune 和手动压缩；rewind 两项测试覆盖 conversation rewind 清除、files-only rewind 保留。 |
| promoted/sync 语义保持 | 完整 async compaction 场景组覆盖 promotion、控制取消、预算、失败、timeout 和 model route；promoted 完成仍立即发布 `async_compact=false`。 |

## 已执行

- `cargo test --locked --lib -p shell async_compaction_commits_at_boundary_and_notifies_after_request_projection -- --nocapture --test-threads=1`：1 passed、0 failed。
- `cargo test --locked --lib -p shell async_compaction -- --nocapture --test-threads=4`：17 passed、0 failed。
- `cargo test --locked --lib -p shell compaction_pre_prune_tests -- --nocapture --test-threads=4`：27 passed、0 failed（包含上述 17 项，不重复相加）。
- `cargo test --locked --lib -p shell rewind_pre_compaction_with_cancelled_turns_truncates_context_gb2961 -- --nocapture --test-threads=1`：1 passed、0 failed。
- `cargo test --locked --lib -p shell files_only_rewind_is_exempt_from_chat_state_bound -- --nocapture --test-threads=1`：1 passed、0 failed。
- `openspec validate --all --strict --no-interactive`：20 passed、0 failed（归档前）。
- `git diff --check -- <本 change 触及文件>`：通过。
- `cargo clean`：成功，删除 39768 个文件、12.1 GiB 构建产物；清理发生在全部 Cargo 测试之后。

## 归档

- `openspec archive finalize-async-compaction-notice-projection --yes`：成功，新增 requirement 已合入 `context-compaction` 主规范并归档为 `2026-09-21-finalize-async-compaction-notice-projection`。
- 首次归档后校验按预期只报告归档操作自身任务尚未勾选（348 passed、1 failed）；完成本记录和任务状态后重新执行最终校验。
- 最终 `openspec validate --all --strict --no-interactive`：19 passed、0 failed；`openspec validate --archived --no-interactive`：349 passed、0 failed。

## 限制与非阻断告警

- Rust 测试链接产生既有 macOS `__eh_frame section too large` warning，不影响编译或测试结果。
- `cargo fmt --all -- --check` 仍会报告仓库中大量既有、与本 change 无关的格式差异；为避免改写用户工作区中的并行改动，没有批量格式化。触及 diff 的 whitespace/error marker 检查通过。
- 未运行全 workspace 测试或替换正在运行的 Grow 二进制；验证范围集中于 Shell 的压缩、请求投影和 rewind 生命周期。
