## 调用链核对

主 Agent 与子 Agent 都通过 `spawn_session_on_thread` 创建 SessionActor。两者共用闭合 Step 后的后台压缩、请求投影后的同步压缩、工具输出后的压力检查以及 provider 超窗后的恢复。普通子 Agent 与 Workflow 入口当前都给任务输出预算传 `None`。Workflow 阈值继续来自捕获的模型路由。

## 验证

- `cargo test --locked -p shell --lib task_resume_presentation_tests -- --test-threads=2`，2 项通过。新增用例在 64,000 token 的子模型上设 60% 阈值，确认派生子 Agent 的首个普通请求之前已生成摘要，并带上近期响应与新任务。
- `cargo test --locked -p shell --lib compaction_pre_prune_tests -- --test-threads=2`，56 项通过，包含请求压力、工具结果裁剪、后台提升、取消、路由变化及超窗恢复。
- `cargo test --locked -p shell --lib auto_compact_threshold -- --test-threads=2`，4 项通过。
- 行为回归使用本地模拟 provider。可执行文件安装与原会话重放不属于本次验证。
