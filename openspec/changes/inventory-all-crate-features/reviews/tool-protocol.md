# tool-protocol 逐包核查

包路径：`crates/common/tool-protocol`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读，本批尚未运行动态测试。

## 模块与开关

- `crates/common/tool-protocol/Cargo.toml`
- `crates/common/tool-protocol/src/capabilities.rs`
- `crates/common/tool-protocol/src/ids.rs`
- `crates/common/tool-protocol/src/lib.rs`
- `crates/common/tool-protocol/src/turn_hook.rs`
- `crates/common/tool-protocol/tests/identifier_validation.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [RWX authority algebra](../specs/tool-authorization/spec.md#requirement-rwx-authority-algebra)：ToolAccess SHALL 表示 None、Read、Write、Execute 及其并集，提供 union、covers 和单项测试；未知访问类型的默认值为 All。
- [Native capability declaration](../specs/tool-authorization/spec.md#requirement-native-capability-declaration)：ToolCapabilities SHALL 严格解析 streaming、supports_cancel、max_concurrency、hooks、max_frame_bytes、timeout_ms 与 max_access；StreamingSpec 声明 subkind 与可选 delta cap。
- [Validated local invocation identifiers](../specs/tool-authorization/spec.md#requirement-validated-local-invocation-identifiers)：ToolCallId SHALL 是非空 opaque 字符串且可生成 UUID v7；ToolId SHALL 限于 name 或 namespace:name，每段非空且只含 ASCII 字母数字、下划线或连字符。
- [Typed turn hook exchange](../specs/extension-runtime/spec.md#requirement-typed-turn-hook-exchange)：TurnHookRequest SHALL 以 phase 区分 before/after，payload 保留 turn/model 与 before 的消息数/关系/schema 或 after 的 outcome/耗时/调用数/写入路径/可选取消信息；HookReply 支持有序 System/Developer/User 注入和 Auto/ForceContinue/ForceStop 控制。

## 边界

- covers 返回 false；All 可覆盖所有 RWX 组合。
- 仅 None 与 Read 返回 true；None 是控制平面标记，不自动授予工具 identity。
- streaming=None、supports_cancel=false、max_concurrency=None、hooks 为空、frame/timeout=None、max_access=All；并发 None 的含义为不设上限。
- 因 deny_unknown_fields 拒绝；声明本身不执行取消、超时或权限策略。
- 返回校验错误；ToolCallId 不额外要求输入必须是 UUID。
- 得到空 injections 与 Auto；未知 reply 字段被拒绝。
- 拒绝反序列化；completed 的取消信息可省略，outcome 仅接受 completed/cancelled/error。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。
