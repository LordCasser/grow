# Verification

- 旧实现运行 `cargo test --locked --offline -p tools --lib response_limit_stops_unfinished --quiet`：1 项失败；loopback 服务保持响应打开，已发出超限正文，客户端 2 秒后仍未完成。
- 修复后运行 `cargo test --locked --offline -p tools --lib implementations::grow_build::web_fetch:: --quiet`：131 通过，0 失败，0 忽略。两次均设置 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。
- 真实 loopback chunked HTTP 回归覆盖未结束超限响应、恰好上限、零上限空响应和非空响应。服务器通过 oneshot 清理，客户端测试显式 no_proxy；无外部网络或用户数据变更。
- 实现沿用 reqwest 的解码，限制检查作用于 Response::chunk 返回的数据；本轮未新增压缩格式专项集成测试。
- 累计正文长度受限；不声称限制 HTTP 库单块分配、Vec 预留容量或后续解析内存。授权/重定向审计仍未整体完成。
