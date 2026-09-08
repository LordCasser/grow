# Verification

- 旧实现运行 cancellation_interrupts_configuration_probes：1 失败，token 取消后调度仍等待未完成探测直至虚拟超时。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_restart::tests --quiet`：22 通过、0 失败、0 忽略；仅已有 __eh_frame 链接警告。
- 测试使用真实 Tokio LocalSet、虚拟时钟、Notify 和可阻塞 MockActions，覆盖 stdio/HTTP × 调度/循环四个位置；先确认进入探测，再取消，验证无恢复调用、无状态推送、无残留 claim。
- 生产探测源码确认 mcp_state.lock().await，但本轮未运行真实 MCP 进程或构造完整 SessionActor 持锁场景。同步磁盘读取不可被 select 中断；dispatcher 强制终止时取消源生命周期另记 backlog。
