## 验证

- `cargo test --locked -p shell --lib planning -- --test-threads=2` 通过 101 项。子会话同时具有直属父绑定和本地 TODO，连续请求没有规划摘要，也未向父计划 backend 发送读取请求。主会话的本地计划、交付绑定和既有通知回执继续受检。
- `cargo build --locked -p cli --bin grow` 成功。用当前产物运行 `GROW_BINARY="$PWD/target/debug/grow" cargo test --locked -p shell --test test_subagent_todo_context --test test_subagent_todo_control -- --ignored --test-threads=1`，两个场景均通过。首次 spawn/resume 和后续无变更请求没有规划摘要或控制通知。编辑与删除各在下一实际请求末尾交付一次，随后可通过原结构化入口确认，停止与交付验收继续完成。
- CLI 回归使用 loopback provider 和测试独占存储。
