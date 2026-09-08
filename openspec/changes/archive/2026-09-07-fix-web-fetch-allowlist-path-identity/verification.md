# Verification

- 旧实现 `cargo test --locked --offline -p tools --lib path_scoped_ --quiet`：4 通过、2 失败。错误表现为不同大小写路径误匹配，以及带路径大写 WWW/主机尾点配置不能匹配。
- 修复后 tools 的 `implementations::grow_build::web_fetch::` 全组：132 通过；workspace 的 `web_fetch` 组：10 通过。均为 `cargo test --locked --offline -p <package> --lib <filter> --quiet`，设置 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。
- 路径回归覆盖大小写、末尾点、主机规范化、尾斜杠；现有测试覆盖兄弟前缀、多个路径和主机全路径许可。权限组验证已有用户配置/默认列表/会话授予等调用行为。
- 未执行外部请求。重定向授权传播另记 backlog，不以本次匹配修复宣称完整授权审计完成。
