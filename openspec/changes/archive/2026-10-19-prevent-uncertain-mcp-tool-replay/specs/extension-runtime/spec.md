## ADDED Requirements

### Requirement: Uncertain MCP tool calls are never automatically replayed

Grow SHALL dispatch an ordinary MCP `tools/call` at most once after invocation begins unless it has explicit evidence that the tool request was never sent. Transport loss, HTTP MCP errors, timeouts, identical arguments, and tool annotations SHALL NOT independently authorize replay. An uncertain failure SHALL explain that effects may already have occurred. Connection recovery MAY prepare a later independent call and SHALL remain conditional on the originating service identity.

#### Scenario: Server writes before its reply is lost
- **WHEN** a server applies a tool effect and the invocation loses its transport or reply
- **THEN** Grow reports an uncertain outcome without sending a second tool request, and the server effect count remains one.

#### Scenario: Later explicit call uses recovered connectivity
- **WHEN** a failed originating transport has been invalidated and a new independent tool invocation is admitted
- **THEN** existing initialization may reconnect for that new invocation without replaying the previous one.

#### Scenario: Replacement and annotations do not grant replay
- **WHEN** an old invocation fails after replacement, or its tool advertises read-only or idempotent hints
- **THEN** Grow neither replays it on the old or replacement client nor resets the replacement using the old failure.

## MODIFIED Requirements

### Requirement: Abandoned MCP calls notify their originating service

Grow 对 stdio/Streamable HTTP MCP `tools/call` SHALL 持有本次请求的 JSON-RPC id 与发起时的 peer。请求在收到回复前因工具期限到期或调用 future 被取消而放弃时，SHALL best-effort 向原 peer 发送匹配 id 的 `notifications/cancelled`，而不是向按服务器名称查得的替代连接发送。已完成调用 SHALL 不发取消；通知失败 SHALL 不修改工具结果。取消通知不承诺 server 停止或撤销副作用，也不授权重放。

#### Scenario: Completed result or business error
- **WHEN** 一次工具调用正常返回结果或 `isError` 业务结果
- **THEN** 守卫解除，不发送取消通知，结果按原语义交付。

#### Scenario: Tool deadline expires
- **WHEN** 在途请求到达其配置期限而没有回复
- **THEN** 原 peer 收到至多一次与该请求 id 匹配的取消通知；Grow 返回说明可能已有副作用的 timeout，HTTP 仅按原服务身份条件重置 transport，不自动重放该工具。

#### Scenario: Turn cancellation drops the call
- **WHEN** 有 request id 的调用 future 在收到回复前被丢弃
- **THEN** 不等待远端处理即 best-effort 发匹配 id 的取消通知；没有 id 时不伪造通知。

#### Scenario: Retry belongs to another request
- **WHEN** 原调用失败后，调用方明确接纳一次新的独立操作
- **THEN** 新旧操作使用各自的 id/peer，任一轮放弃只通知该轮；连接恢复不自动生成第二次工具调用。

#### Scenario: Replacement service or failed notification
- **WHEN** 旧调用被取消时新 Ready 服务已安装，或取消通知发送失败
- **THEN** 新服务不接收旧 id 的通知也不被旧请求重置；通知失败仅留下诊断，原调用终态/取消语义不被覆盖。

#### Scenario: ACP reverse bridge limitation
- **WHEN** `tools/call` 经过当前不转发无 id 通知的 ACP reverse bridge
- **THEN** Grow 不宣称该 bridge 的远端必然收到取消；stdio/HTTP 的协议通知行为不因此退化。
