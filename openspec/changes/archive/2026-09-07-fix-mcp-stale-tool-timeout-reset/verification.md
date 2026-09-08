# Verification

- 旧实现 late_tool_timeout_preserves 回归：1 失败，新 Ready 被旧超时重置。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：159 通过、0 失败、0 忽略，5.02秒。随后只扩充测试的 Initializing 保护断言，单独运行该回归再次通过（1.01秒），生产代码无后续变动。
- 真实 loopback HTTP fixture 挂住旧请求，另一 recover 完成后等待一秒工具超时；断言原超时错误、新服务 Arc 保留、initialize总数2、工具调用总数1。额外将测试状态置于 Initializing，确认带旧身份的 reset 不改变状态或 revision，再恢复测试状态。
- 原 reset 的低层强制状态测试显式传 None；唯一生产 reset 调用传本次服务 Some，已用全仓引用搜索核对。合并仅服务 reset 的 replace_state helper，保留 notify_waiters。无外部网络和用户配置改动。
