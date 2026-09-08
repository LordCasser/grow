# Verification

- 旧实现 late_tool_error_reuses 回归：1 失败，initialize 总数为3而非2，证明旧错误重置了新连接。
- 修复后 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p mcp --lib --quiet`：158 通过、0 失败、0 忽略，5.02秒。
- 真实 loopback HTTP fixture 通过 Notify 控制旧 tools/call 错误释放；先等待另一 recover 完成，再释放旧错误。断言成功、调用总数2、握手总数2及非超时。全组覆盖原错误透传、至多重试一次与超时不重放。
- 公共主动 recover 语义不变；私有错误恢复只在失败服务仍为当前 Ready 时重置。使用已有 Arc 身份，无额外计数器或兼容层。
- 超时分支的迟到 reset 仍列 backlog，不宣称已修复。无外部网络或用户配置更改。
