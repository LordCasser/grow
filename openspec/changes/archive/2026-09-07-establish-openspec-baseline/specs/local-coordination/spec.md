## ADDED Requirements

### Requirement: Peer handshake identity
本地协调连接 SHALL 使用包含 protocol version、peer identity、incarnation、bearer token 和 source session 的握手。

#### Scenario: 建立协调连接
- **WHEN** 客户端发送 ClientHello
- **THEN** 服务端通过 ServerHello 返回是否 accepted 及可能的错误。

证据：`crates/codegen/shell/src/coordination/protocol.rs` — `ClientHello`。

### Requirement: Inquiry scoped operations
协调协议 SHALL 以 inquiry_id 与 target_session_id 标识询问和取消，并区分进度、结果、取消回执与错误。

#### Scenario: 取消询问
- **WHEN** 客户端对已知 inquiry_id 发送 Cancel
- **THEN** 响应保留 inquiry_id 和 accepted 状态，不将取消回执伪装成询问答案。

证据：`crates/codegen/shell/src/coordination/protocol.rs` — `Request`。
