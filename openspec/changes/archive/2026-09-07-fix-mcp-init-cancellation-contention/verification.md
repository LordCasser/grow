# Verification

- 旧实现定向 `init_guard_cancellation_survives`：1 失败，cleanup discarded its target under contention。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：163 通过、0 失败、0 忽略。
- 跨线程守卫回归覆盖 Empty/Pending 两种取消目标；主线程暂持状态锁，工作线程执行 Drop，释放锁并 join 后检查状态。
- 真实 ensure_initialized 的 ACP pending 握手 future 在另一线程销毁，验证状态锁竞争结束后恢复 Pending。旧异步提交锁等待窗口已消失，不能继续同线程持同步锁再 poll。
- 并发恢复回归仍检查两次恢复共享相同服务 Arc，初始化总次数为 2（初始一次、恢复一次）。revision 断言改为一次 reset 和一次 handshake 的总增量 2，符合新同步接纳在首次 poll 中可立即进入握手的时序。
- 所有生产 ClientState 锁入口已核对：初始化接纳/提交、恢复、超时重置及只读投影。Notify 等待和网络握手在独立锁作用域之外。McpState 的 async Mutex 未改。
- SafeTokioChildProcess Drop 调度异步 kill/reap；RunningService Drop 取消服务任务而不 join。未执行负载性能测试或全仓全部包测试。

Shell 集成调用方验证：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_ --quiet`，127 通过、0 失败、0 忽略，仅已有 __eh_frame 链接警告。
