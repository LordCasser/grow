## 现场证据

- Grow 会话 `01a1574e-6c7f-7921-a8be-fbfeafcc0a2a` 位于 skynet 工作区，运行进程 PID 27007 持有原 writer lock。
- Timeline 在 seq 338 记录 `SuspendRequested`，seq 339 记录带一个后台命令的 `CommandsSettled`，之后没有 Notification Received 或 Runtime checkpoint。用户按 `r` 重试后仍显示退出未确认，Timeline 没有推进。
- `suspension.rs` 原路径为 `TaskCompleted` 传入 `Opaque`；`chat-state/src/timeline.rs::valid_notification_source_version` 仅允许 `TaskCompleted` 搭配正数 `Ordinal`。拒绝路径清理未接纳通知的 payload，现场通知目录为空，符合该失败机制。
- 会话原始目录已另存为 `/Users/lordcasser/.grow/recovery/01a1574e-6c7f-7921-a8be-fbfeafcc0a2a-2026-10-20.tar.gz`，SHA-256 `223245c3b26fdf4aaa56611e2e4165e828e34a5e70d39693385c4b06462a5b83`。备份不解除原 writer lock。
- `grow export` 已从活跃 writer 的只读观察路径导出一份 Markdown 记录至 `/Users/lordcasser/.grow/recovery/01a1574e-6c7f-7921-a8be-fbfeafcc0a2a-export/transcript.md`。

## 验证

- `cargo test -p shell command_settlement_retry_reuses_evidence_after_terminal_eviction --lib --locked`：1 passed。
- `cargo test -p shell failed_notification_admission_reclaims_its_write_ahead_payload --lib --locked`：1 passed。
- `cargo fmt --all -- --check`：通过。
- `cargo build --locked -p cli --bin grow`：通过，生成 `target/debug/grow`（2.4.1 / 02432cb6）。编译器仅报告现有 deprecated `fetch_update` 与链接器 `__eh_frame` 警告。
- 修正处直接使用 `Ordinal { value: 1 }`，与 `valid_notification_source_version` 对 `TaskCompleted` 的正数 `Ordinal` 要求一致。当前测试没有驱动整个真实 `suspend_node` 和活跃终端进程；因此不声称旧会话已经恢复。
- `openspec validate --all --strict --no-interactive`：17 passed，0 failed（归档前）。
- `openspec archive fix-suspended-background-command-receipt --yes`：已合入 `session-timeline`，归档为 `2026-10-20-fix-suspended-background-command-receipt`。归档时最后一项归档任务尚未勾选，归档后已更新。
- 归档后 `openspec validate --all --strict --no-interactive`：16 passed，0 failed；`openspec validate --archived --no-interactive`：649 passed，0 failed；`git diff --check`：通过。
- 构建后用 `cargo clean -p shell` 清理本次 Shell 编译缓存 6.5 GiB；`target/debug/grow --version` 仍正常返回 2.4.1 / 02432cb6。
- 用修复后的 `target/debug/grow --fork-session --resume 01a1574e-6c7f-7921-a8be-fbfeafcc0a2a` 创建会话 `c09198e2-fd9f-4885-8d46-49ab06fcd07a`。副本加载了旧对话，summary 记录 parent ID 和 139 条更新；随后正常退出，Timeline 在 seq 74 提交 checkpoint、seq 75 提交 `TreeSuspended`。再次按该 ID resume 能加载对话并正常退出。原会话文件和 writer 未改动。

## 现场处理边界

修复代码不会热更新 PID 27007。此进程仍在运行，未执行 `f` 强退或修改原会话文件。原会话缺少 root checkpoint；在旧 writer 退出前，不能声称同 ID 的执行型 resume 已可用。
