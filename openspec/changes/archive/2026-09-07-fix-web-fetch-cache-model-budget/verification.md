# Verification

- 旧实现运行 `cargo test --locked --offline -p tools --lib cached_text_follows_model_budget --quiet`：1 项失败，小窗口缓存命中仍返回完整页面。
- 修复后运行 `cargo test --locked --offline -p tools --lib implementations::grow_build::web_fetch:: --quiet`：128 项通过，0 失败，0 忽略。两次均使用 `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`。
- 回归通过真实 WebFetchClient::fetch 缓存命中路径验证缩小窗口、恢复文件完整内容、旧路由克隆以及再次放大窗口。缓存注入完整无路径文本，未访问网络或真实用户会话。
- 代码核对：prepare_web_fetch_config 在开关关闭/显式空域名列表时返回 Disabled；AgentBuilder 默认工具注入检查 is_enabled。该检查不构成全部显式自定义工具注册路径的审计完成声明。
- 未改网络请求、缓存 TTL、SSRF 或模型切换流程。
