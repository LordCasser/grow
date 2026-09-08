# Verification

- 旧实现真实 stdio 回归失败：取消后非 Empty。
- 最终 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：162 通过、0 失败、0 忽略。
- Unix 测试启动 /bin/sleep 60，经正式 start_mcp_server/ensure_initialized 进入握手后取消；验证 Empty、init_done 通知与下次调用立即报错。子进程使用生产 kill_on_drop 路径；未另断言进程退出时序。
- 既有 HTTP/ACP 取消恢复与提交锁等待取消回归继续通过。本次不解决取消时状态锁仍被占用的 try_lock 失败。
