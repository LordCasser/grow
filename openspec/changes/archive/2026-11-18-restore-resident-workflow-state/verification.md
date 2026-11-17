# Verification

- 两个真实 ACP 客户端连接同一个隔离 Leader。A 切到 Workflow 模式，运行包含 `phase` 和 `agent` 的测试脚本。loopback provider 保持子任务运行。A 收到 revision 7，`active_agents=1`。旧构建中 B 加载只收到 revision 3 和 4，`active_agents=0`。
- 使用 Rust 1.93.1 重建的 v2.6.16 CLI 重复实验。B 在 `session/load` 完成前收到仅发给 B 的 `grow/taskStateOnly` 更新，revision 7、`active_agents=1`，子任务 ID 与 A 一致。A 没有收到 B 的恢复更新。
- Pager 的 `resident_state_advances_revision_without_adding_history` 通过，验证旧 revision 稍后到达时无法覆盖状态，定向更新也没有新增正文行。独立审查后在 load 最终回包前补上了原 actor 身份和存活检查。
