## 会话证据

原 Grow 会话中的 Rust 预览子 Agent 在北京时间 12:34 至 12:38 的五条模型消息里重复确认规划上下文。该段没有新 PlanGuidance 记录。检查发现模型请求反复读取父计划，投影将待处理变更呈现为检查指令，通知正文也要求再次读取父计划。

## 验证

- `cargo test --locked -p tool-types --lib planning -- --test-threads=2`，55 项通过。进度切换更新 revision，保持零新重规划记录；目标、取消和重新打开的用例继续受检。
- `cargo test --locked -p shell --lib planning -- --test-threads=2`，117 项通过。父计划在同一回合的两个请求间变更，第二个请求复用初始父观察。旧措辞的已提交回执在冷恢复后按同一身份确认，不重复写入或中断。
- 构建默认 CLI 后，使用其路径作为 GROW_BINARY 运行 `cargo test --locked -p shell --test test_subagent_todo_control -- --ignored --test-threads=1`，1 项通过。子 Agent 收到修订后的目标与验收要求后直接调用结构化计划操作；编辑与停止后的请求仍引用同一初始父观察，实际控制通过通知送达，并完成子结果和父 review。
- 真实 CLI 验证使用 loopback provider 与测试自有会话。原会话只读诊断。
