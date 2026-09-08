# Verification

- 旧实现 aborted_dispatcher_cancels 回归：1 失败，实际 run_dispatcher 被 abort 后仍发生 respawn。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_dispatcher --quiet`：35 通过、0 失败、0 忽略，包含 dispatcher 和 dispatcher_e2e 测试；仅已有 __eh_frame 链接警告。
- 新回归使用真实 run_dispatcher/LocalSet/事件 channel 与虚拟时钟，MockActions 共享真实去重标记，Notify 确认调度已取得标记再 abort。验证标记清空、推进退避后无恢复调用。没有启动真实进程。
- 新增 abort 回归直接覆盖 stdio；HTTP 使用同一 dispatcher-owned token，现有 HTTP 集成测试随组通过，未新增 HTTP abort 专项。
- 保留正常取消和 drain；守卫仅保证退出时通知取消，不提供强制终止时同步 join 的新承诺。
