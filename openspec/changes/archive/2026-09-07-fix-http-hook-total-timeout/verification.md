# Verification

- 旧实现校验阶段 tokio timeout 与请求 ClientBuilder timeout 分别取完整 spec.timeout_ms，顺序执行形成重复预算，源码已确认。
- 新回归注入 120 ms 校验延迟并连接真实本地 HTTP server，响应头立即返回 200，正文延迟 120 ms；整体预算 200 ms，结果为 TimedOut，保留状态 200 与 URL，正文预览为空。每阶段均短于单独预算，验证总预算不刷新。
- 测试注入受控校验结果是为了避免依赖公网 DNS 延迟；生产固定使用 validate_hook_url，不开放替换入口。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：214 单元、13 集成、1 doctest 全部通过，0 失败/忽略。
- 未模拟 DNS 系统调用卡死或同步代码不可抢占；外层 timeout 在异步挂起点生效，不宣称硬实时 deadline。

Shell 调用方：`CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p shell --lib hook_ --quiet`，25 通过、0 失败/忽略，仅已有 __eh_frame 链接警告。
