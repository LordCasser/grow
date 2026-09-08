# announcements 逐包核查

包路径：`crates/codegen/announcements`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/codegen/announcements/Cargo.toml`
- `crates/codegen/announcements/src/lib.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [Local announcement payload](../specs/client-surfaces/spec.md#requirement-local-announcement-payload)：公告层 SHALL 提供包含可选 id、message、severity、title、CTA、expires_at、dismissible 和 persistent 的本地公告结构，以及带公告列表的更新 payload。
- [Announcement visibility and expiry](../specs/client-surfaces/spec.md#requirement-announcement-visibility-and-expiry)：公告过滤 SHALL 排除 trim 后为空的 message；合法 RFC3339 expires_at 在当前时间达到或超过它时过期，缺失或无法解析的期限不判为过期。
- [Announcement hidden state](../specs/client-surfaces/spec.md#requirement-announcement-hidden-state)：隐藏状态 SHALL 以 trim 后非空 id 为 key，无 id 时由 title、message 和单元分隔符生成 key；状态保存为有序 hidden_ids 集合，坏文件或非规范 JSON 读为无隐藏项。

## 边界

- 可解析，不要求 URL、caption 同时存在。
- 返回 grow-default 的 info 公告，dismissible=true、persistent=false。
- is_expired_at 为 true，过滤结果不包含该项。
- 本层不据此隐藏公告；message 过滤与时间过滤各自提供入口。
- prune_hidden_announcement_ids 删除失效 key，并返回集合是否变化。
- 读取返回空集合；写入当前是 best-effort，调用方不获得成功持久化保证。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
