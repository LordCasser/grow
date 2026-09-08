# Verification

- 旧实现运行 unpolled_recovery_tasks 回归：1 项失败，stdio 调度后销毁 LocalSet 留下 claim。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib mcp_restart::tests --quiet`：21 通过，0 失败，0 忽略；出现已有 __eh_frame 链接警告，退出码 0。
- 新回归循环验证 stdio 和 HTTP 调度：首次 poll 前 drop LocalSet，不执行 respawn/reset 或状态推送，claim 释放且可重新取得。已有测试覆盖退避、成功/耗尽、去重与运行中取消。
- 使用真实 Tokio LocalSet 和 MockActions，未启动真实 MCP 进程或远程连接，不宣称完整会话关闭已覆盖。配置探测 await 的取消边界另记 backlog。
