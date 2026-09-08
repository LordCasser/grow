# auth 逐包核查

包路径：`crates/codegen/auth`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/auth/Cargo.toml`
- `crates/codegen/auth/src/auth_provider.rs`
- `crates/codegen/auth/src/header_middleware.rs`
- `crates/codegen/auth/src/lib.rs`
- `crates/codegen/auth/src/visibility.rs`

Cargo feature：`{"middleware": ["dep:reqwest-middleware", "dep:http"]}`。

## 功能与规范映射

- [Credential provider seam](../specs/http-credentials/spec.md#requirement-credential-provider-seam)：HTTP 认证 SHALL 通过 HttpAuth::apply 和 AuthCredentialProvider::snapshot 提供请求应用与凭据快照；StaticAuthCredentialProvider 将 apply 委托给 inner，并返回构造时传入的可选 bearer。
- [Optional bearer middleware](../specs/http-credentials/spec.md#requirement-optional-bearer-middleware)：启用 auth 的 middleware feature 后，AuthHeaderMiddleware SHALL 在每次请求读取当前 snapshot；合法 token 写入 Authorization，随后调用 next，不在本层重试 401。

## 边界

- snapshot.token 为 None；具体端点头由 inner.apply 决定。
- 只输出 has_bearer，不输出 bearer 内容。
- 返回该响应，测试观察到一次请求。
- None 不额外写头；非法值记录警告后继续 next，不凭空生成新 token。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
