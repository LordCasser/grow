## ADDED Requirements

### Requirement: ACP v1 local handler boundary
ACP适配层 SHALL 重导出SDK v1 schema及连接原语，以Grow的AgentSide/ClientSide和?Send handler将本地会话实现与SDK连接隔离。

#### Scenario: Agent方法
- **WHEN** 实现AcpAgentHandler
- **THEN** 必须实现initialize/authenticate/new_session/prompt/cancel；load、mode、config option、list默认method_not_found。

#### Scenario: Client方法
- **WHEN** 实现AcpClientHandler
- **THEN** 必须实现request_permission和session_notification；文件读写与五种terminal方法默认method_not_found；两侧ext method默认JSON null，ext notification默认成功，Rc/Arc委托全部方法。

证据：`crates/codegen/acp-transport/src/protocol.rs` — `schema::v1`；`crates/codegen/acp-transport/src/handler.rs` — `AcpAgentHandler`；`crates/codegen/acp-transport/src/handler.rs` — `delegate_client_handler`。

### Requirement: ACP typed bidirectional messages
ACP内部消息 SHALL 为每侧定义11个变体，将request与对应response oneshot绑定，支持boxed/unboxed存储和method_name查询。

#### Scenario: Agent入站
- **WHEN** 构造Agent消息
- **THEN** 包含initialize/authenticate/session new/load/list/set mode/set config option/prompt/cancel及ext method/notification。

#### Scenario: Client入站
- **WHEN** 构造Client消息
- **THEN** 包含permission、read/write text、session update、terminal create/output/release/wait/kill及ext method/notification；route_to方法按变体spawn本地handler并尽力送回结果。

证据：`crates/codegen/acp-transport/src/message.rs` — `AcpAgentMessageGeneric`；`crates/codegen/acp-transport/src/message.rs` — `AcpClientMessageGeneric`；`crates/codegen/acp-transport/src/message.rs` — `route_to_agent`；`crates/codegen/acp-transport/src/message.rs` — `route_to_client`。

### Requirement: ACP internal message serialization
Agent消息序列化 SHALL 只输出method_name与request，扩展消息使用内部ext_method/ext_notification标签；反序列化按已知方法选择类型。

#### Scenario: 未知方法
- **WHEN** 反序列化不认识的method_name
- **THEN** 返回错误，不自动归为扩展。

#### Scenario: 响应接收端
- **WHEN** 反序列化成功或转换boxed
- **THEN** 反序列化创建oneshot但立即丢弃receiver，不提供可等待响应；boxed转换保留原response sender。

证据：`crates/codegen/acp-transport/src/message.rs` — `RawMessage`；`crates/codegen/acp-transport/src/message.rs` — `boxed`。

### Requirement: ACP channel round trip failures
ACP双向通道 SHALL 使用unbounded mpsc；acp_send先enqueue再等待typed oneshot，无内置deadline或重试。

#### Scenario: 发送失败
- **WHEN** 接收端已关闭
- **THEN** 返回InternalError，data.growAcpChannelFailure=send_failed。

#### Scenario: 响应丢失
- **WHEN** 请求已入队但response sender被丢弃
- **THEN** 返回InternalError且tag为recv_failed；classifier对未知或无tag返回None，不解析错误文本。

证据：`crates/codegen/acp-transport/src/channel.rs` — `acp_send`；`crates/codegen/acp-transport/src/channel.rs` — `acp_channels`；`crates/codegen/acp-transport/src/common.rs` — `AcpChannelFailure`。

### Requirement: ACP gateway forwarding modes
Gateway sender SHALL 支持等待响应、返回completion receiver和只报告enqueue成功的fire-and-forget三种入口，clone共享队列。

#### Scenario: Client通知发送
- **WHEN** AgentSide通过trait发送session或ext notification
- **THEN** fire-and-forget后返回Ok，即使队列拒绝；需要判断接受与否的调用方使用bool入口。

#### Scenario: Agent通知发送
- **WHEN** ClientSide通过trait发送cancel或ext notification
- **THEN** 等待本地forward响应，与AgentSide通知入口不同；completion只代表所接handler完成，不代表远端已确认业务处理。

证据：`crates/codegen/acp-transport/src/gateway.rs` — `forward_with_completion`；`crates/codegen/acp-transport/src/gateway.rs` — `forward_fire_and_forget`；`crates/codegen/acp-transport/src/gateway.rs` — `session_notification`。

### Requirement: ACP gateway dispatch and trace context
Gateway receiver SHALL 按队列接收后为各消息独立spawn任务，默认spawn_local，可覆盖spawner；结束接收循环不等待已spawn任务。

#### Scenario: 元数据链路
- **WHEN** 配置on_meta且普通request存在meta
- **THEN** 构造span并instrument任务，缺meta使用空span，ExtRequest/ExtNotification不调用on_meta。

#### Scenario: 调试追踪
- **WHEN** 启用tracing
- **THEN** 以compact JSON记录完整request和成功response，无ANSI且无字段脱敏；序列化失败返回空字符串。独立任务不保证任意异步handler完成顺序，现有ordering测试handler在首个await前记录。

证据：`crates/codegen/acp-transport/src/gateway.rs` — `with_on_meta`；`crates/codegen/acp-transport/src/gateway.rs` — `with_spawn_fn`；`crates/codegen/acp-transport/src/gateway.rs` — `before_request`；`crates/codegen/acp-transport/src/common.rs` — `compact_json`。

### Requirement: ACP SDK byte stream connection lifecycle
connect_agent_v1和connect_client_v1 SHALL 接受Send字节流，以unbounded队列桥接SDK回调和本地handler，返回sender与需驱动的连接future。

#### Scenario: 入站分派
- **WHEN** SDK接收request或notification
- **THEN** request保存responder与cancellation并独立spawn，handler结果转JSON后响应；notification错误被忽略，未知request返回method_not_found，未知notification忽略。

#### Scenario: 连接结束
- **WHEN** outgoing gateway结束或incoming_closed完成
- **THEN** 连接回调返回成功，不在本层join已spawn本地handler；LineBufferedRead由调用方选择包装，connect本身不自动加行缓存。

证据：`crates/codegen/acp-transport/src/connection.rs` — `connect_agent_v1`；`crates/codegen/acp-transport/src/connection.rs` — `connect_client_v1`；`crates/codegen/acp-transport/src/connection.rs` — `incoming_closed`。

### Requirement: ACP request cancellation dispatch
入站request SHALL 由SDK RequestCancellation::run_until_cancelled包裹handler；Agent prompt取消后另调用session cancel，忽略cancel返回错误。

#### Scenario: 在线取消prompt
- **WHEN** 收到对应$/cancel_request
- **THEN** 现有字节流测试观察prompt future被drop、cancel一次及-32800 Request cancelled响应。

#### Scenario: 其他请求取消
- **WHEN** Client request或非prompt Agent request取消
- **THEN** 取消其handler future，不额外构造session cancel；此层不保证handler派生后台工作被取消。

证据：`crates/codegen/acp-transport/src/connection.rs` — `dispatch_agent_request`；`crates/codegen/acp-transport/src/connection.rs` — `dispatch_client_request`；`crates/codegen/acp-transport/src/connection.rs` — `online_request_cancellation_reaches_prompt_handler`。

### Requirement: ACP extension wire names and peer requests
两侧Peer SHALL 对普通请求调用SDK send_request并等待block_task，对通知调用send_notification；扩展请求和通知的wire method无条件前置下划线。

#### Scenario: 扩展往返
- **WHEN** 发送逻辑grow/coordination/list
- **THEN** wire为_grow/coordination/list，现有测试验证入站handler收到无前缀逻辑名称；扩展响应由JSON反序列化，失败为InternalError。

#### Scenario: 批量协议
- **WHEN** 接收含cancel通知与list请求的JSON-RPC batch
- **THEN** 现有SDK适配测试只对list生成batch response，cancel送至handler；碎片化initialize按V1返回。

证据：`crates/codegen/acp-transport/src/connection.rs` — `wire_ext_request`；`crates/codegen/acp-transport/src/connection.rs` — `wire_ext_notification`；`crates/codegen/acp-transport/src/connection.rs` — `stable_v1_initialize_batch_and_unknown_notification`。

### Requirement: ACP complete line buffering
LineBufferedRead SHALL 在后台读取完整换行分隔byte行，经容量参数64的channel供AsyncRead消费；缓冲行内连续返回Ready，Pending发生在无行可用时。

#### Scenario: EOF与小buffer
- **WHEN** 输入最后一行无换行或消费buffer小于行
- **THEN** 保留最终部分行并跨多次read交付，结束返回0，不做UTF8验证。

#### Scenario: 超大行
- **WHEN** 累计读取超过64MiB含终止换行
- **THEN** 在扩展当前fill buffer并consume后返回InvalidData且停止生产；限制不是全队列总内存上限，内置同名oversize测试实际只测普通行。

证据：`crates/codegen/acp-transport/src/line_reader.rs` — `LineBufferedRead`；`crates/codegen/acp-transport/src/line_reader.rs` — `read_line_capped`；`crates/codegen/acp-transport/src/line_reader.rs` — `read_line_capped_rejects_oversized`。

### Requirement: ACP dedicated stdin line reader
spawn_stdin_line_reader SHALL 启动名为acp-stdin的OS线程，以blocking read_until读取原始byte行并经容量64通道blocking_send，保留末尾无换行行。

#### Scenario: EOF错误和接收端关闭
- **WHEN** read返回0/错误或blocking_send失败
- **THEN** 退出线程并关闭发送端；接收端drop不能主动中断正在阻塞的stdin read，无join/cancel handle，单行无大小上限。

#### Scenario: Windows隔离
- **WHEN** 可复制原始stdin句柄
- **THEN** 线程使用私有副本并尝试将进程stdin置NUL；duplicate失败回退全局stdin，NUL失败仍使用副本，SetStdHandle返回值未检查，不保证隔离总成功。

证据：`crates/codegen/acp-transport/src/stdin_reader.rs` — `spawn_stdin_line_reader`；`crates/codegen/acp-transport/src/stdin_reader.rs` — `forward_lines`；`crates/codegen/acp-transport/src/stdin_reader.rs` — `isolate_process_stdin`。
### Requirement: Pager session match priority and unassigned-active fallback

The shared session matcher SHALL scan agents once, return the first exact root session-id match immediately, otherwise retain the first child-view key match, and only after the scan use a child match. When neither exists, an active Agent view whose root session id is absent is returned as a Root fallback; all other unmatched notifications return none. Root precedence applies even if the same string is also a child key. Hash-map iteration determines which parent wins if duplicate child keys exist. The fallback does not verify that the notification originated from the active agent's in-flight creation.

#### Scenario: Root precedence
- **WHEN** one root id matches
- **THEN** that root is returned even if a child key also matches.

#### Scenario: Child fallback
- **WHEN** no root matches and a child map contains the id
- **THEN** the first encountered owning parent is returned as Child.

#### Scenario: Race fallback
- **WHEN** nothing matches and the active root has no assigned session id
- **THEN** the active agent is returned as Root.

#### Scenario: Known active root
- **WHEN** nothing matches but the active root already has an id
- **THEN** none is returned.

#### Scenario: Duplicate child key
- **WHEN** multiple parents contain the same child id
- **THEN** the hash-map's first encountered match wins.

证据：`crates/codegen/pager/src/app/acp_handler/routing.rs` — `find_session_match`、`SessionMatch`。

### Requirement: Pager matched notification owner and MCP root resolution

Notification-owner resolution SHALL combine shared session matching, parent-agent active status and mutable parent lookup, returning the SessionMatch, active-parent flag and owner. MCP lifecycle resolution reuses that result but rejects Child matches, because progress is stored only on the root agent. The active flag identifies the parent container rather than whether a particular child is fullscreen. Missing matches or missing owner entries return none.

#### Scenario: Notification owner
- **WHEN** session matching and parent lookup succeed
- **THEN** match kind, active-parent flag and mutable parent are returned.

#### Scenario: Missing parent
- **WHEN** a match cannot be resolved to an agent entry
- **THEN** none is returned.

#### Scenario: MCP root
- **WHEN** the payload id resolves to a root
- **THEN** the root and active-parent flag are returned.

#### Scenario: MCP child
- **WHEN** the payload id resolves to a child
- **THEN** MCP target resolution returns none.

证据：`crates/codegen/pager/src/app/acp_handler/routing.rs` — `resolve_notif_agent`、`mcp_target_agent`。

### Requirement: Pager concrete root-child session and scrollback borrowing

The concrete target-view resolver SHALL return the child AgentSession and ScrollbackState from `subagent_views[child_sid]` for a Child match, or the parent root session and scrollback for a Root match. A missing child key returns none; the Root branch does not compare child_sid. This helper borrows transcript state only and does not decide visibility, redraw or notification validity.

#### Scenario: Root pair
- **WHEN** the match is Root
- **THEN** the parent session and scrollback are returned.

#### Scenario: Child pair
- **WHEN** the match is Child and child_sid exists
- **THEN** that child view's session and scrollback are returned.

#### Scenario: Missing child
- **WHEN** the match is Child but child_sid is absent
- **THEN** none is returned.

#### Scenario: Visibility
- **WHEN** a parked child resolves
- **THEN** the borrow succeeds without implying the child is visible.

证据：`crates/codegen/pager/src/app/acp_handler/routing.rs` — `resolve_target_view`。

### Requirement: Pager parent-active and concrete-view-active distinction

The parent-active predicate SHALL be true when active_view is the matched AgentId. The concrete-view predicate additionally requires the parent active and then treats a root as visible only when no active_subagent is attached, while a child is visible only when active_subagent exactly equals the addressed session string. Missing parents, dashboard or other active agents return false. These predicates do not validate that active_subagent still names an existing child view.

#### Scenario: Parent active
- **WHEN** active_view names the owner
- **THEN** the parent-active predicate is true.

#### Scenario: Root hidden by child
- **WHEN** the owner is active with any active_subagent
- **THEN** the root concrete view is inactive.

#### Scenario: Fullscreen child
- **WHEN** the owner is active and active_subagent equals the child session
- **THEN** that child concrete view is active.

#### Scenario: Stale active child
- **WHEN** active_subagent equals the string but its map entry is absent
- **THEN** this predicate alone can still return true.

证据：`crates/codegen/pager/src/app/acp_handler/routing.rs` — `is_matched_agent_active`、`is_matched_view_active`。

### Requirement: Pager concrete interactive AgentView resolution

Interactive AgentView resolution SHALL return the parent itself for Root or dereference the matching boxed child view for Child. A missing child key returns none. It does not re-check session identity, ownership, visibility or active_subagent, relying on the previously produced SessionMatch and supplied child string.

#### Scenario: Root
- **WHEN** the prior match is Root
- **THEN** the mutable parent AgentView is returned.

#### Scenario: Child
- **WHEN** the prior match is Child and its key exists
- **THEN** the mutable child AgentView is returned.

#### Scenario: Missing child
- **WHEN** the supplied child key is absent
- **THEN** none is returned.

#### Scenario: Prior evidence
- **WHEN** match and child_sid disagree
- **THEN** the function uses the supplied key without independently validating the original session id.

证据：`crates/codegen/pager/src/app/acp_handler/routing.rs` — `resolve_target_agent_view`。
### Requirement: Pager partial settings DTO presence and tolerant tag decoding

PagerSettingsUpdate SHALL accept omitted fields through serde defaults. permission_mode distinguishes omission as None, JSON null as Some(None), and a string as Some(Some(value)); a malformed nonstring fails the enclosing update. slash_command_tags likewise distinguishes absent, null and a valid ordered string map, but a malformed present value logs a warning and becomes outer None so sibling fields remain usable. tips and ordinary optional booleans do not distinguish absent from null. The pager DTO intentionally lists only TUI-consumed shell fields and ignores unknown additions.

#### Scenario: Permission omit
- **WHEN** permission_mode is absent
- **THEN** no update is represented.

#### Scenario: Permission null
- **WHEN** permission_mode is null
- **THEN** an explicit remote clear is represented.

#### Scenario: Tags map
- **WHEN** slash_command_tags is a string map
- **THEN** the ordered map is retained inside both option layers.

#### Scenario: Malformed tags
- **WHEN** slash_command_tags is an array or otherwise wrong
- **THEN** it becomes absent while sibling fields parse.

#### Scenario: Unknown fields
- **WHEN** the shell adds unlisted fields
- **THEN** default serde behavior ignores them.

证据：`crates/codegen/pager/src/app/acp_handler/settings.rs` — `PagerSettingsUpdate`、`deserialize_presence_aware_string`、`deserialize_settings_update_tags`、`presence_aware_dto_tests`。
### Requirement: Pager ACP client message dispatch acknowledgement contract

The top-level ACP handler SHALL route SessionNotification, RequestPermission, ExtNotification and ExtMethod to their dedicated paths. SessionNotification and ExtNotification always attempt an Ok unit response after processing even when ignored, rejected or not visually affected; response-send failures are ignored. WaitForTerminalExit returns the pager-specific unsupported error and false. Unhandled message variants return false without a response in this function. The boolean denotes redraw effect according to each branch rather than protocol success.

#### Scenario: Session notification
- **WHEN** processing completes or drops the update
- **THEN** Ok is attempted and the branch's affected boolean is returned.

#### Scenario: Extension notification
- **WHEN** method dispatch returns
- **THEN** Ok is attempted even for unknown methods.

#### Scenario: Permission or method
- **WHEN** an interactive request arrives
- **THEN** ownership of its response follows the dedicated handler.

#### Scenario: Wait for exit
- **WHEN** terminal-exit waiting is requested
- **THEN** an unsupported error is sent and false returned.

#### Scenario: Other
- **WHEN** another ACP client message variant arrives
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle`。

### Requirement: Pager Grow extension notification method dispatch table

Grow ExtNotification dispatch SHALL recognize two aliases for session updates plus follow-ups, background start/completion, models, settings, roster, queue, interjection, monitor, scheduled create/fire/delete, announcements, git head and three MCP lifecycle methods, forwarding each to its owning handler. Unknown method strings return false. The outer ACP handler still acknowledges every ExtNotification Ok regardless of this boolean, so false is not a protocol rejection.

#### Scenario: Session aliases
- **WHEN** method is grow/session_notification or grow/session/update
- **THEN** the shared Grow session handler is called.

#### Scenario: Known method
- **WHEN** one listed method matches
- **THEN** its dedicated handler determines affected state.

#### Scenario: Unknown method
- **WHEN** no table arm matches
- **THEN** false is returned.

#### Scenario: Acknowledgement
- **WHEN** the dispatcher returns false
- **THEN** the outer ExtNotification path still attempts Ok.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle_ext_notification`。

### Requirement: Pager blocking extension method dispatch and method-not-found response

Blocking ExtMethod dispatch SHALL route exactly `grow/ask_user_question` and `grow/plan_approval` to their interactive handlers, transferring response ownership. Every other method logs a warning, attempts ACP -32601 with `Method not found: <method>`, ignores response-send failure and returns false. It performs no prefix matching, version negotiation or generic forwarding.

#### Scenario: Question
- **WHEN** the exact ask-user method arrives
- **THEN** the question handler owns state and response.

#### Scenario: Plan approval
- **WHEN** the exact plan-approval method arrives
- **THEN** the plan handler owns state and response.

#### Scenario: Unknown
- **WHEN** another method arrives
- **THEN** a method-not-found error is attempted and false returned.

#### Scenario: Send failure
- **WHEN** the unknown-method response channel is closed
- **THEN** the failure is ignored.

证据：`crates/codegen/pager/src/app/acp_handler/mod.rs` — `handle_ext_method`。

