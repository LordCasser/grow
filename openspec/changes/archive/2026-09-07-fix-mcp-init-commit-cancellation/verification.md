# Verification

- 旧实现定向 regression：1 失败，取消后非 Pending。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：161 通过、0 失败、0 忽略。
- 新回归使用真实 ensure_initialized 和 ACP transport，pending invoker 保持握手等待，暂停时钟触发超时；持状态锁将结果提交挂起，释放锁后不再 poll 而销毁 future。验证取消恢复责任覆盖最后提交等待。
- 未覆盖取消时其他任务继续持锁的 try_lock 失败，亦未修复 stdio 不可恢复 transport 的取消状态；两项已登记 backlog，不能以本测试声明整个初始化取消已完全收敛。
