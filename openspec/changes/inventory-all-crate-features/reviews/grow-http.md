# grow-http 逐包核查

包路径：`crates/codegen/grow-http`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/grow-http/Cargo.toml`
- `crates/codegen/grow-http/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Origin identity and User Agent](../specs/http-credentials/spec.md#requirement-origin-identity-and-user-agent)：HTTP identity SHALL 支持环境、session meta 和 ClientType 来源；meta 优先 clientIdentifier 后 clientType，合并时保留 primary product 并仅回填缺失 version。
- [Shared non sampling HTTP clients](../specs/http-credentials/spec.md#requirement-shared-non-sampling-http-clients)：非采样 HTTP SHALL 复用 OnceLock 客户端；async 客户端设置 30 秒连接超时及连接池/keepalive，startup blocking 客户端设置 5 秒连接和请求总超时。
- [Bounded pool escape retry](../specs/http-credentials/spec.md#requirement-bounded-pool-escape-retry)：send_with_retry_escaping_pool SHALL 至少执行一次 op，并在多次尝试的最后一次使用新建无 idle pool 的 HTTP/1.1 client；是否重试由调用方判断，backoff 在第 N 次重试前等待。
- [Transport errors and startup budget constants](../specs/http-credentials/spec.md#requirement-transport-errors-and-startup-budget-constants)：TransportFailure SHALL 先判断 connect 错误为 Unreachable，再将 timeout/request/body 判为 Interrupted，其余判 Permanent，并保留完整 source 链。

## 边界

- User Agent 折叠重复部分；否则保留 origin 与 agent 两部分及 OS/arch。
- 第二次设置 panic；headless mode 设置可重复且只向 headless 单向锁存，未设置默认为 interactive。
- 返回缓存客户端 clone，不重复创建 TLS client；async 调用方仍需自行设置请求总超时。
- 安装 auth::AuthHeaderMiddleware，凭据策略遵循认证 provider。
- 立刻返回错误，不等待后续尝试。
- 记录警告并用 pooled client 完成最后尝试；op 包含的 send/body/decode 全部位于重试单元内。
- 优先读取 raw_os_error，必要时解析其显示文本中的 os error 后缀；未找到则 None。
- fetch/refresh 为 5 秒、auth 为 60 秒、reapply 为 30 秒及 3 次尝试、minimum connect 为 120 秒；常量本身不执行请求或认证刷新。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
