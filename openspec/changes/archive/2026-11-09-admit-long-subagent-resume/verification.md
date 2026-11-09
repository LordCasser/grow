## 会话证据

同一父会话里，TS 和 Rust 的两次派生恢复在启动时分别以约 841,884 和 987,922 token 的 active Surface 粗估值触发了 1,000,000 token 窗口的 80% 门槛。失败发生在普通模型请求前。用模拟模型移除门槛后复现了第二个阻塞，派生 Seed 的用户项没有本地 prompt 编号，压缩器找不到闭合历史选区。

## 验证

- `cargo test --locked -p shell --lib task_resume_presentation_tests -- --test-threads=1`，2 项通过。长来源从 128,000 窗口派生到 44,000 窗口，先发送摘要请求，再发送包含摘要、近期响应和新任务的普通请求。完整继承展示在 live 和 cold replay 中可读，来源 Timeline 字节不变。极小窗口沿正常压缩路径报告无闭合选区且没有 provider 请求。
- `cargo test --locked -p chat-state --lib compaction_utils -- --test-threads=1`，146 项通过，包含运行时前缀、Goal 回合、完整工具往返及近期保留预算。
- 运行验证使用本地模拟 provider。原线上会话仅作只读诊断，需要新 Grow 可执行文件的 ignored CLI 测试未运行。
