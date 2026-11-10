## Verification

- 会话 `01a1bc26-aa9a-7fd1-a833-d742cd964792` 的 10:05–10:07 时间段记录了八次规划确认和一次 TODO 提交。10:07:55 提交 rev 42。普通请求此前会反复投影主 Agent 自己的本地计划。
- `cargo test --locked -p shell --lib planning -- --test-threads=2` 通过 93 项，覆盖主 Agent 多次采样无快照、子 Agent 不自动查询以及通知重试去重。
- `cargo test --locked -p tool-types --lib planning` 通过 55 项，覆盖无效更新、状态推进和有效目标修改，另一个 TODO 绑定的子 Agent 不受影响。
- 以 `cargo build --locked -p cli --bin grow` 构建本地 CLI。使用该二进制运行 `test_subagent_todo_context` 和 `test_subagent_todo_control`，两项均通过，覆盖启动和恢复请求、主 Agent 请求无快照以及子 Agent 在采样边界接收修改和停止通知。
