# Verification

- reqwest 0.13 本地源码 ClientBuilder::resolve_to_addrs 将指定主机写入覆盖映射，保留请求 URL 端口；no_proxy 清除代理并禁用系统代理。
- 新真实网络测试使用 pinned.invalid 域名、127.0.0.1 监听地址及明确端口；客户端连接成功，服务端收到原域名 Host，证明不依赖系统 DNS 解析目标。该测试为 HTTP 底层连接，不替代 TLS 集成测试；生产仍只接受 HTTPS，URL 未改写。
- IPv6 字面测试：fd00::1 明确被 IP 分类拒绝；::1 返回指定 8443 端口的 loopback 地址，不进入域名解析分支。
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p hooks --quiet`：213 单元、13 集成、1 doctest 全通过，0 失败/忽略。
- 不跳转、请求错误 URL 脱敏、分块容量限制等既有测试通过。未启动真实代理做系统代理转发测试，代理关闭由 builder 配置及依赖实现核对；未重写 IP 分类清单。
- DNS 检查与请求分别计时的问题仍待下一项修复。
