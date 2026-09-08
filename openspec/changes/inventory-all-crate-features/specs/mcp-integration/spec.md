## ADDED Requirements

### Requirement: MCP transport ownership
mcp SHALL re-export rmcp 并提供 stdio、streamable HTTP 及 ACP reverse bridge；构建使用关闭默认 feature 的 rmcp 2.2 和 reqwest 0.13 no-provider TLS 配置。

#### Scenario: 当前行为边界
- **WHEN** 启动 HTTP 或旧 Sse 配置
- **THEN** 两者均创建 streamable HTTP pending client；未知 transport variant 返回错误，不实现独立旧 SSE 协议栈。

证据：`crates/codegen/mcp/src/servers.rs` — `start_mcp_server`。

### Requirement: MCP ACP wire constants
MCP-over-ACP SHALL 使用 grow/mcp/call 转发调用、grow/mcp/sdk_call 反向调用、grow/mcp/servers 注册列表和 grow/mcp/sdk 能力标记。

#### Scenario: 当前行为边界
- **WHEN** 区分客户端调用与 SDK 内存服务调用
- **THEN** forward/reverse 使用不同方法字符串，schema 不能混用。

证据：`crates/codegen/mcp/src/wire.rs` — `MCP_SDK_CALL`。

### Requirement: ACP MCP half duplex bridge
ACP bridge SHALL 逐行解析请求，丢弃空行、坏 JSON 及缺失或 null id 的通知；每个有效请求通过独立 invoke task 转交 SDK，响应由单 writer 写回。

#### Scenario: 当前行为边界
- **WHEN** 服务器需要 initialized 通知或主动 sampling/roots 请求
- **THEN** 当前桥接不提供这些方向；initialized 无 id 也在本地丢弃。

证据：`crates/codegen/mcp/src/acp_transport.rs` — `read_requests`。

### Requirement: ACP response correlation and errors
bridge SHALL 将响应 object 的 id 覆盖为请求 id，invoker Err 或非 object 响应转 JSON-RPC -32603；其它响应 schema 不在此验证。

#### Scenario: 当前行为边界
- **WHEN** SDK 返回数组或错误
- **THEN** 构造带原 id 的 Internal error，避免无可关联响应；合法 object 不保证含 result/error。

证据：`crates/codegen/mcp/src/acp_transport.rs` — `with_id`。

### Requirement: ACP bridge concurrency and teardown
bridge SHALL 使用两个 256 KiB duplex、128 项响应 channel 和 JoinSet；完整 read_line 后回收完成任务，reader 结束时 abort 剩余 invokes。

#### Scenario: 当前行为边界
- **WHEN** 一个请求很慢或消息跨多次写入
- **THEN** 其它请求可以先响应，读取不因任务回收而取消半条 line；缓冲容量不等于请求总长度或 active task 数硬上限。

证据：`crates/codegen/mcp/src/acp_transport.rs` — `pump`。

### Requirement: ACP reverse timeout budget
ACP handshake SHALL 给 invoker 传 max(startup timeout, server tool timeout) 作为每轮 reverse budget；bridge 只透传预算，外层 serve 和运行时 tool call 另有 timeout。

#### Scenario: 当前行为边界
- **WHEN** per-tool override 大于 server tool timeout
- **THEN** 外层按 per-tool 计算，但 reverse budget 不包含该 override，不能承诺 invoker 不提前超时。

证据：`crates/codegen/mcp/src/servers.rs` — `try_handshake`。

### Requirement: MCP pool initialization state
InitProgress SHALL 使用 NotStarted、Starting、Finished 与 handshaking set；只有 Finished 且 set 为空才算 initialized。

#### Scenario: 当前行为边界
- **WHEN** finish_init 在背景握手完成前调用
- **THEN** 保留未完成 set，is_initializing 仍真；mark_server_ready 仅移除该名称，成功与失败原因分开记录。

证据：`crates/codegen/mcp/src/servers.rs` — `InitProgress`。

### Requirement: MCP init failure registry
McpState SHALL 单独保存 init_failed 原因，fresh mark_servers_initializing 清对应旧失败；record/clear 与握手集合状态分别管理。

#### Scenario: 当前行为边界
- **WHEN** 在 NotStarted 标记 initializing
- **THEN** 先清旧 failure，随后 InitProgress 忽略新增握手集合并警告；不能据此认定初始化已启动。

证据：`crates/codegen/mcp/src/servers.rs` — `mark_servers_initializing`。

### Requirement: MCP full config reset
update_configs SHALL 按配置 Vec JSON 比较，变化时清 owned clients、事件 authority、工具 meta 和 disabled registrations，取消 init 并增加 generation；保留 shared clients、ACP registry 和独立权限配置。

#### Scenario: 当前行为边界
- **WHEN** 只是改变配置顺序
- **THEN** 仍判为变化；不变则返回 false，不增加 generation。

证据：`crates/codegen/mcp/src/servers.rs` — `update_configs`。

### Requirement: MCP differential config replacement
配置 diff SHALL 按 server name 的序列化配置识别 added/removed/retained，同名变化同时 removed 与 added；只删除受影响 owned，未变 owned 保留其事件 episode。

#### Scenario: 当前行为边界
- **WHEN** 全局 generation 因新增别的 server 改变
- **THEN** retained owned 的原 client/config episode 仍有效；retained-but-not-owned 的在途 authority 被撤销。

证据：`crates/codegen/mcp/src/servers.rs` — `update_configs_diff`。

### Requirement: MCP owned and shared client lookup
McpState SHALL 在同名时 owned 优先 shared；SharedMcpPool 保存 Arc 客户端及配置快照，并携带 live eligibility authority。

#### Scenario: 当前行为边界
- **WHEN** 通过 get_client 或 pool 快照查找
- **THEN** 获得当前 map 或快照中的 Arc；这些访问器本身不执行 live eligibility、disabled tool 或 access ceiling 检查。

证据：`crates/codegen/mcp/src/servers.rs` — `SharedMcpPool`。

### Requirement: MCP live inherited eligibility
SharedMcpEligibility SHALL 逐层检查 server scope、客户端 incarnation 及工具成员资格；上游同名替换或撤权可使后代当前权限立即失效。

#### Scenario: 当前行为边界
- **WHEN** 根客户端替换而中间父级尚未 reconcile
- **THEN** 后代拒绝旧 incarnation；父级 reconcile 并 publish 后才恢复新绑定资格。

证据：`crates/codegen/mcp/src/servers.rs` — `SharedMcpEligibility`。

### Requirement: MCP inheritance filtering and reconciliation
restrict/exclude SHALL 累积收窄继承 server scope，reconcile 从 live authority 重建 shared map 并排除本地 configs 名称；返回前后涉及的名称供调用方注销旧定义。

#### Scenario: 当前行为边界
- **WHEN** spawn 后父级新增 server
- **THEN** 若固定 scope 允许，则 live current_clients 可接纳；config/meta 快照不随过滤同步删改。

证据：`crates/codegen/mcp/src/servers.rs` — `reconcile_inherited_clients`。

### Requirement: MCP eligibility publication generations
McpEligibilityAuthority SHALL 对客户端 ID 集合及工具集合改变递增 generation，不改变则保持；shared generation 为本层与上游 wrapping sum。

#### Scenario: 当前行为边界
- **WHEN** 同名但 client_id 改变
- **THEN** 视为 transport 资格改变；generation 与锁 poison/耗尽行为不能当持久化或跨进程身份机制。

证据：`crates/codegen/mcp/src/servers.rs` — `McpEligibilityAuthority`。

### Requirement: MCP client event authority
MCP client-origin 事件 SHALL 携带私有构造的 client_id、config_generation、transport_revision；admission 比较当前 authority 的完整 episode。

#### Scenario: 当前行为边界
- **WHEN** 旧连接延迟通知到达
- **THEN** 旧 handler 固定的 revision 不会被当前共享 sender 重新标记为新 transport，dispatcher 可拒绝旧事件。

证据：`crates/codegen/mcp/src/servers.rs` — `McpClientEpisode`。

### Requirement: MCP event channel ownership
McpState SHALL 内部创建 unbounded 事件 channel，仅公开 receiver；给 owned clients 绑定 sender，shared clients 保持父级事件所有权。

#### Scenario: 当前行为边界
- **WHEN** 重新安装或关闭 channel
- **THEN** fanout 遍历已 owned clients，不遍历 shared 或尚未 owned 的在途实例；发送失败不回滚状态。

证据：`crates/codegen/mcp/src/servers.rs` — `install_client_event_channel`。

### Requirement: MCP typed lifecycle events
McpState SHALL 从真实配置 transition 发布 ConfigDiff，并提供按当前 server authority 标记 ToolsChanged、TransportClosed、HandshakeFailed 的入口。

#### Scenario: 当前行为边界
- **WHEN** 缺失 dispatcher 或当前 authority
- **THEN** typed client emit 返回 false；config 事件不携带 client episode。

证据：`crates/codegen/mcp/src/servers.rs` — `emit_current_tools_changed`。

### Requirement: MCP SDK server registry
ACP registry SHALL 将 name/serverId 注册表和 invoker 一起保存，跨普通 config reset 保留；每次 init 依据调用方传入 override 构造尚无 owned/shared 同名客户端。

#### Scenario: 当前行为边界
- **WHEN** 已存在同名客户端
- **THEN** 不为该注册构造 pending ACP client；本方法不去重同批重复 registration。

证据：`crates/codegen/mcp/src/servers.rs` — `build_pending_acp_clients`。

### Requirement: MCP timeout precedence
MCP timeout SHALL 采用 meta 毫秒向上取整为秒，其次外部秒配置，再用 startup30/tool6000 默认；per-tool meta 覆盖同名外部 per-tool，再回退 server 默认。

#### Scenario: 当前行为边界
- **WHEN** 配置为 1 ms 或 0 ms
- **THEN** 分别解析为 1 秒或 0 秒；tool_timeout_for 注释中的默认60秒不是当前实现。

证据：`crates/codegen/mcp/src/servers.rs` — `load_timeouts`。

### Requirement: MCP metadata parsing
parse_mcp_meta_config SHALL 从 _meta.mcpConfig 解析 camelCase 可选 startupTimeoutMs、toolTimeoutMs、toolTimeoutsMs、exposeImageBase64，缺失或整体非法返回空 map。

#### Scenario: 当前行为边界
- **WHEN** 任一 server 配置类型非法
- **THEN** 整 map 反序列化失败并回退空，不逐条保留其它合法配置。

证据：`crates/codegen/mcp/src/servers.rs` — `parse_mcp_meta_config`。

### Requirement: MCP single flight handshake
ensure_initialized SHALL 在 Pending 时由一个调用者持 transport 执行握手，其余 Initializing 调用者等待 Notify；Ready 返回缓存 service，Empty 返回无 transport 错误。

#### Scenario: 当前行为边界
- **WHEN** 等待握手的调用者超过 startup+1 秒
- **THEN** 返回 init still in progress 错误，不在这个超时分支恢复 state；Ready 分支不检测 transport 是否已关闭。

证据：`crates/codegen/mcp/src/servers.rs` — `ensure_initialized`。

### Requirement: MCP handshake restoration boundary
握手结果 SHALL 成功进入 Ready，失败的 HTTP/ACP 恢复 Pending，stdio 进入 Empty；取消时 InitGuard 对可恢复 transport 尝试 try_lock 还原并通知。

#### Scenario: 当前行为边界
- **WHEN** stdio 取消或还原锁竞争
- **THEN** 不保证恢复；guard disarm 后至发布结果前的等待也不受其保护，不承诺所有取消路径无卡住状态。

证据：`crates/codegen/mcp/src/servers.rs` — `InitGuard`。

### Requirement: MCP protocol and client capabilities
MCP initialize SHALL 显式使用协议2025-06-18，客户端名 grow-shell-<server> 与 version::VERSION，声明 UI extension MIME text/html;profile=mcp-app。

#### Scenario: 当前行为边界
- **WHEN** rmcp 默认最新协议改变
- **THEN** 此处仍使用显式版本；声明 MIME 不等于本包实现完整 UI 渲染。

证据：`crates/codegen/mcp/src/servers.rs` — `make_client_info`。

### Requirement: MCP transport recovery
HTTP/ACP SHALL 可由构造时保存的地址状态 reset→ensure→arm 恢复；reset 与每次 handshake 增加 transport revision，stdio 不能从已消费 child 在此重新启动。

#### Scenario: 当前行为边界
- **WHEN** 多个 recovery 或外部 reset 并发
- **THEN** state_kind 与 reset 分别加锁，结果发布也未比较 revision；当前不据注释承诺任意并发恢复只发生一次。

证据：`crates/codegen/mcp/src/servers.rs` — `recover`。

### Requirement: MCP liveness observation
is_healthy 和 liveness_check SHALL 只读 state 与 rmcp transport closed 标记，不执行网络握手；Ready/open 健康，非Ready属于 transient。

#### Scenario: 当前行为边界
- **WHEN** 远端 HTTP 已不可用但 service channel 仍开放
- **THEN** 本地健康谓词可能仍返回 true，不是远端主动探活。

证据：`crates/codegen/mcp/src/servers.rs` — `liveness_check`。

### Requirement: MCP one shot liveness watcher
非ACP客户端有 sender 且 Ready、slot为空时 SHALL 可启动 liveness poller，默认500ms、首tick立即、错过tick跳过；closed发一次事件，transient静默退出，两者清slot。

#### Scenario: 当前行为边界
- **WHEN** handle 被 drop
- **THEN** CancellationToken 取消 poller；ACP不主动watch，依赖惰性恢复；Ready+closed实测不由Empty stub测试证明。

证据：`crates/codegen/mcp/src/liveness.rs` — `spawn_transport_liveness`。

### Requirement: MCP HTTP headers and compatibility
HTTP transport SHALL 逐项解析 headers，非法项警告并跳过，重复项后者覆盖；Figma名字或figma.com子域且未设UA时补grow-cli。

#### Scenario: 当前行为边界
- **WHEN** 凭据被拒绝
- **THEN** 本包无交互式OAuth或credential refresh；重建仍使用显式headers快照。

证据：`crates/codegen/mcp/src/servers.rs` — `build_http_transport`。

### Requirement: MCP session header expansion
HTTP header value SHALL 支持 {{session_id}} 与 ${session_id} 替换；无session时丢弃含任一占位符的整项。

#### Scenario: 当前行为边界
- **WHEN** 替入session值自身含第二种token
- **THEN** 当前顺序两次replace可再次展开；不是统一single-pass模板。

证据：`crates/codegen/mcp/src/servers.rs` — `expand_session_id_headers`。

### Requirement: MCP SSE reconnect backoff
McpHttpClient SHALL 只在 get_stream 前按距上次成功建立 <2 秒判定rapid，第二次rapid开始500ms指数退避至30秒；POST/delete直接委托。

#### Scenario: 当前行为边界
- **WHEN** 连接建立失败或stream持续较久
- **THEN** 失败不更新建立时间；>=2秒间隔重置episode，此wrapper不主动发起重试或限制重试总数。

证据：`crates/codegen/mcp/src/mcp_http_client.rs` — `plan_on_get_stream`。

### Requirement: MCP reconnect warning budget
SSE warning SHALL 每个episode至多一次，并使用可跨客户端重建共享的1小时WarnBudget；被冷却抑制的episode可在后续边界补一次warn。

#### Scenario: 当前行为边界
- **WHEN** 同episode已warn且再次跨过1小时
- **THEN** 仍只debug；恢复后新episode重新检查共享cooldown。

证据：`crates/codegen/mcp/src/mcp_http_client.rs` — `WarnBudget`。

### Requirement: MCP qualified tool registration
MCP qualified ID SHALL 恰有一个 overlap-aware __ 且两段非空并满足ToolId，注册另检查完整ASCII名称字母/下划线起始、仅字母数字下划线连字符且最多64字符。

#### Scenario: 当前行为边界
- **WHEN** 名称含___、多分隔或provider非法字符
- **THEN** 跳过注册并日志；不同server的同raw tool得到不同qualified运行时ID。

证据：`crates/codegen/mcp/src/servers.rs` — `into_registration`。

### Requirement: MCP tool visibility and schema
工具注册 SHALL 保留meta，缺description为空；schema缺type/properties时补object/空对象，已有字段不覆盖。ui.visibility数组只有含model才model-visible，其它类型默认可见。

#### Scenario: 当前行为边界
- **WHEN** visibility为仅app或空数组
- **THEN** model_visible=false；此包返回registration，实际model/UI派发门禁由调用者接入。

证据：`crates/codegen/mcp/src/servers.rs` — `get_tool_registrations`。

### Requirement: MCP tool directory pagination
工具列表和描述文件 SHALL 分页读取直到next_cursor=None，先收集全量内容；本层不限制页数、不检测重复游标且无独立list timeout。

#### Scenario: 当前行为边界
- **WHEN** 服务持续给重复cursor
- **THEN** 当前循环没有本地终止门槛；不能把握手startup timeout套到list阶段。

证据：`crates/codegen/mcp/src/servers.rs` — `get_tool_registrations`。

### Requirement: MCP tool descriptor materialization
materialize_descriptors SHALL 把原name/description/inputSchema写入 server_dir/tools/<sanitized>.json，使用spawn_blocking和逐文件temp+persist原子替换；资源不落地。

#### Scenario: 当前行为边界
- **WHEN** 工具删掉或sanitize同名碰撞
- **THEN** 不清理旧文件，碰撞可覆盖；返回成功写次数而非唯一文件数，不提供整目录事务。

证据：`crates/codegen/mcp/src/servers.rs` — `materialize_descriptors`。

### Requirement: MCP server instruction access
server_instructions SHALL 仅从Ready peer_info读取非空白instructions并返回原文本，不触发init；调试call_tool直接ensure后调用MCP。

#### Scenario: 当前行为边界
- **WHEN** 调用调试call_tool而非运行时Tool包装器
- **THEN** 不获得本地per-tool timeout、recovery和内容转换；非object args被置None。

证据：`crates/codegen/mcp/src/servers.rs` — `server_instructions`。

### Requirement: MCP runtime tool timeout and retry
运行时McpErasedTool SHALL 在ensure之后为tools/call设置per-tool秒timeout；transport闭合/发送错误可恢复一次，HTTP业务RPC除四个客户端错误码及auth拒绝外也可恢复一次。

#### Scenario: 当前行为边界
- **WHEN** 首次调用超时
- **THEN** 不重试可能有副作用的操作；HTTP reset供下次用，返回ToolError；恢复失败保留原错误，重试失败返回第二次错误。

证据：`crates/codegen/mcp/src/servers.rs` — `try_call_tool`。

### Requirement: MCP auth error classification
McpError SHALL 对指定auth词及右侧非ASCII字母数字的上下文401模式作字符串分类；typed Spawn/Timeout不归认证拒绝。

#### Scenario: 当前行为边界
- **WHEN** 错误仅含403 Forbidden或401ms
- **THEN** 不匹配；403混authentication文字仍可能匹配，不能声明所有403都被排除。

证据：`crates/codegen/mcp/src/servers.rs` — `is_auth_rejection_message`。

### Requirement: MCP runtime content projection
MCP业务错误 SHALL 聚合Text为errored输出；正常结果聚合Text、Image及Resource，image blob转data URI，其它Resource序列化JSON，未匹配内容不输出。

#### Scenario: 当前行为边界
- **WHEN** 结果含audio、resource link或structuredContent
- **THEN** 当前该run分支不保留这些部分；transport ToolError提前返回，尾部McpToolCalled诊断和MCPOutput标志不产生。

证据：`crates/codegen/mcp/src/servers.rs` — `impl tool_runtime::Tool for McpErasedTool`。

### Requirement: MCP image base64 exposure
图片 SHALL 默认只发data URI，exposeImageBase64按meta>override>false决定是否追加raw base64 wrapper。

#### Scenario: 当前行为边界
- **WHEN** 启用raw暴露
- **THEN** 额外mcp_image_base64块不含data:image前缀；本函数不验证mime/base64，也不证明下游图像提取已成功。

证据：`crates/codegen/mcp/src/servers.rs` — `format_mcp_image`。

### Requirement: MCP resilient stdio framing
ResilientRwTransport SHALL 按LF读取JSON-RPC，去末CR、跳空行，坏行继续读取且不回复错误；未知缺id且method字符串通知静默忽略。

#### Scenario: 当前行为边界
- **WHEN** 其它坏行
- **THEN** tracing warn最多取200个字符sample，不是200字节；当前没有注释所称独立session decode事件，也不限制输入整行长度。

证据：`crates/codegen/mcp/src/servers.rs` — `ResilientRwTransport`。

### Requirement: MCP stdio process lifecycle
stdio SHALL 配管道、kill_on_drop和detach，best-effort登记ProcessGroup/Scope；关闭stdin后给3秒退出，再杀进程组并回收leader。

#### Scenario: 当前行为边界
- **WHEN** drop时没有entered runtime
- **THEN** 尝试cleanup线程与临时runtime；创建失败降级日志/信号，不承诺任意故障或取消路径永不遗留进程。

证据：`crates/codegen/mcp/src/servers.rs` — `SafeTokioChildProcess`。

### Requirement: MCP stdio spawn planning
Windows裸程序名 SHALL 用PATH解析launcher，带分隔符或非Windows直接使用；显式env覆盖继承env，当前cwd继承，批量启动buffer_unordered8。

#### Scenario: 当前行为边界
- **WHEN** 原命令非UTF8或需要保序结果
- **THEN** 调用helper前已有lossy转换；批量结果按完成顺序，不保证原输入顺序。

证据：`crates/codegen/mcp/src/servers.rs` — `start_mcp_servers`。

### Requirement: MCP stderr log capture
stdio stderr SHALL 复制到grow_home/logs/mcp/<sanitized96chars>.stderr.log，每spawn截断，打开后后台copy。

#### Scenario: 当前行为边界
- **WHEN** 同名实例或持续大量日志
- **THEN** 文件可能共用且无大小上限/轮转；目录或文件打开失败则不继续drain。

证据：`crates/codegen/mcp/src/servers.rs` — `drain_mcp_stderr_to_log`。


### Requirement: Shell MCP project replacement and setup resolution order

MCP项目读取 SHALL 按find_project_configs顺序覆盖同名全局定义，较近目录优先，整体替换不深合并；单server查询反向寻找首个可解析定义再回全局。这些读取函数自身无folder trust检查。merged运行列表先按name读取preferences解决setup，Required跳过、Invalid warning跳过，再展开环境字符串并转ACP；全局加载失败仍继续项目加载。scope只检查项目是否有可解析该name，否则user，不表示server确实存在。

#### Scenario: Project setup required replaces global
- **WHEN** 项目同名配置覆盖可运行全局定义但setup尚未完成
- **THEN** 该name从本次ACP列表省略，不回退全局。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn get_mcp_server_config_with_project`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub(crate) fn reload_mcp_servers_merged`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub(crate) fn mcp_server_scope`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/mcp.rs` — `fn test_project_scoped_mcp_override_replaces_entirely`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn test_project_scoped_mcp_adds_new_servers`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn test_project_scoped_mcp_can_disable_server`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn test_project_scoped_mcp_preserves_unrelated_global_servers`。

### Requirement: Shell MCP preferences read write recovery boundary

MCP preferences SHALL 使用grow_home/mcp_preferences.json；NotFound为Missing，其他读错/JSON错Corrupt，两者解析视默认空但Corrupt不可写。save先重新检查Corrupt拒绝覆盖，pretty JSON写PID时间临时文件，Unix chmod0600成功后rename；无事务锁、create_new或fsync，失败无统一临时清理。restore仅重载后替换/删除单server key再保存，Corrupt返回Ok跳过，不代表恢复成功或并发更新安全。

#### Scenario: Corrupt preferences during restore
- **WHEN** restore读到Corrupt
- **THEN** 返回Ok但不写任何恢复内容。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn load_mcp_preferences_from`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn save_mcp_preferences_to`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn restore_mcp_preference_server`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `async fn mcp_preferences_missing_malformed_and_save_round_trip`。

### Requirement: Shell MCP setup catalog conflict ordering

setup目录 SHALL 先收集配置中enabled且setup存在的项，记录config和scope；再遍历active plugins，各plugin文件定义先于inline且同名first wins。任何TOML有效解析定义的name阻断plugin，包括disabled或非setup定义；plugin中disabled/无setup省略，跨plugin同名保留先项，记录plugin名称。不在此验证setup字段输入或启动连接。

#### Scenario: Disabled TOML claims plugin name
- **WHEN** TOML声明disabled同名server且plugin提供可用setup
- **THEN** plugin setup项仍被屏蔽。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn collect_mcp_setup_configs`。

### Requirement: Shell MCP disabled tool list persistence

save_mcp_disabled_tools SHALL 写独立disabled_mcp_tools表内server数组，空列表删该key并删空表；非空名称原样保存不去重。读取或TOML解析失败回退空root，已存在非table目标节报错；pretty重写固定toml.tmp后rename，无SAVE_LOCK、权限复制、fsync或统一清temp。该路径比server toggle保存宽松，不可套用后者拒绝坏TOML规则。

#### Scenario: Malformed original TOML
- **WHEN** 禁用工具写入读到坏TOML
- **THEN** 以空root继续构建并可能替换原文件。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn save_mcp_disabled_tools`。

### Requirement: Shell MCP server toggle user and project effects

server toggle SHALL 先更新用户disabled_mcp_servers字符串列表及已有table定义enabled；列表非字符串丢弃，enable删同名、disable仅无同名才追加，不创建server定义。enable随后仅将最近项目定义的enabled=false改true，disable不写项目；user-only入口不解除项目禁用。返回实际修改路径，用户成功后项目失败不回滚，函数内无trust gate。

#### Scenario: Project unstick fails after user write
- **WHEN** 用户启用成功而项目配置不可读或不可解析
- **THEN** 返回错误但用户修改保留。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn save_mcp_server_enabled_in`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn save_user_mcp_server_enabled`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn apply_mcp_server_enabled`；`crates/codegen/shell/src/util/config/mcp.rs` — `async fn clear_sticky_project_disabled_at`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn apply_mcp_server_enabled_updates_array_and_per_server_field`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn apply_mcp_server_enabled_managed_name_only_updates_array`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `async fn enable_unstick_only_touches_nearest_project_definition`。

### Requirement: Shell MCP toggle conditional rewrite safety scope

write_toml_table_if_changed SHALL 仅路径精确等于config_path时获取进程SAVE_LOCK；用户缺失视空，其他目标缺失返回false，其他读错或坏TOML拒绝。修改前后按pretty序列化语义比较，无变化不写，有变化委托atomic_write_string；不保证注释保留。项目sticky helper只对table-like server的显式false改true，经toml_edit保留其他布局，不获取该用户锁；其原文件缺失返回false。

#### Scenario: No semantic toggle change
- **WHEN** 配置语义已为目标状态但原格式不同
- **THEN** 比较规范序列化相同，保留原文本不重写。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `async fn write_toml_table_if_changed`；`crates/codegen/shell/src/util/config/mcp.rs` — `async fn clear_sticky_project_disabled_at`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `async fn clear_sticky_project_disabled_at_only_flips_false`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `async fn write_toml_table_if_changed_refuses_unparseable`。

### Requirement: Shell MCP server upsert and delete asymmetry

upsert SHALL 整体替换目标mcp_servers.name为序列化配置，并从disabled_mcp_servers数组去掉该name，不强制config.enabled=true；读取/解析错按空root继续。delete读取错误返回false，解析错按空root后因无entry返回false；只有成功移除entry才清disabled server及tools对应项和空表，否则遗留禁用记录不动。两者固定toml.tmp后rename，无用户SAVE_LOCK、权限复制或统一失败清理；显式path可指项目文件。

#### Scenario: Delete absent definition with disabled records
- **WHEN** server定义不存在但禁用列表有name
- **THEN** 返回false且保留禁用记录。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn save_mcp_server_config_at`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub async fn delete_mcp_server_config_at`。

### Requirement: Shell MCP disabled tools and access ceiling projection

get_all_mcp_disabled_tools SHALL 忽略cwd，仅读effective全局disabled_mcp_tools，逐server数组保留字符串去重为set，空set/错型省略，加载错返回空。max_access读取已按项目覆盖的scoped配置，再按project_scope_allowed过滤project项，返回各config.max_access；过滤后不恢复被项目覆盖的全局同名配置，不根据工具annotation推导权限。此函数只投影声明值，不执行审批。

#### Scenario: Untrusted project shadows global access
- **WHEN** 同名项目定义已覆盖全局，项目scope不允许
- **THEN** 该name从access map移除，不恢复全局max_access。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn get_all_mcp_disabled_tools`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn get_mcp_server_max_access`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_server_max_access_from_scoped_config`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_server_max_access_matches_config`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_server_max_access_includes_trusted_project_winner`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_server_max_access_drops_untrusted_project_winner_without_global_fallback`。

### Requirement: Shell MCP tolerant parser diagnostics and layer fallback

TOML MCP解析 SHALL 逐entry反序列化，未知字段生成Warning但保留有效定义；enabled且blank_transport_field存在生成Error并省略，disabled空transport可保留但仍须通过类型解析；坏entry生成Error不影响兄弟。整节非table返回空且无problem。scoped合并只使用有效entry，因此坏项目同名项不遮蔽有效全局项；problem聚合保留所有层报告，不因后层有效覆盖消除前层问题，层加载错误不加入此problem列表。

#### Scenario: Invalid project override
- **WHEN** 全局定义有效，项目同名entry反序列化失败
- **THEN** 全局仍保留，项目错误可出现在problem列表。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub(crate) fn parse_mcp_servers_with_problems`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn load_mcp_server_configs_with_project`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn load_mcp_server_problems_with_project`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/mcp.rs` — `fn parse_mcp_servers_rejects_blank_transport`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn test_parse_mcp_servers_stdio`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn test_mcp_server_config_parses_tool_timeouts`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn test_mcp_server_config_tool_timeouts_defaults_to_none`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn test_mcp_server_config_parses_expose_image_base64`。

### Requirement: Shell MCP JSON setup and conversion pipeline

JSON读取 SHALL 在IO或顶层JSON错误warning并None，mcpServers非object视空配置；逐entry反序列化失败仅跳过该entry。parse_mcp_config先按preferences解决setup，Required跳过、Invalid warning，随后展开字符串并转ACP；转换None输出缺transport warning，不在JSON读取阶段运行TOML未知字段/blank transport诊断。load_mcp_json_file先is_file，不是文件直接空。

#### Scenario: One malformed JSON server
- **WHEN** mcpServers中一个entry坏而另一个有效
- **THEN** 有效兄弟仍进入后续setup及ACP转换。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_config_from_json_value`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub(crate) fn read_mcp_json`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub(crate) fn parse_mcp_config`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/mcp.rs` — `fn json_map_skips_bad_entry_and_keeps_the_rest`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn plugin_mcp_json_parses`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn plugin_mcp_json_missing_file_is_empty`。
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn plugin_mcp_json_env_var_default_value`。

### Requirement: Shell MCP disabled and CLI known name scope

disabled_mcp_server_names SHALL 合并有效scoped定义中enabled=false名称与effective全局disabled_mcp_servers字符串数组，不读取项目单独禁用数组。cli_known_names先取禁用集合及有效TOML名称，再加CLI插件registry经catalog转换得到的非空名称。all_toml_mcp_server_names实际来自有效解析配置，不包含反序列化失败或enabled空transport项；注释声称包含invalid不能作为可枚举保证。

#### Scenario: Invalid TOML name without other source
- **WHEN** name只出现在无效TOML entry，不在禁用数组或有效catalog
- **THEN** 不会仅因原始key存在进入CLI known集合。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn disabled_mcp_server_names`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn cli_known_mcp_server_names`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn all_toml_mcp_server_names`。

补充测试源码证据（本轮未执行）：
- `crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_json_all_toml_names_includes_disabled`。

### Requirement: Shell MCP definition location raw key semantics

mcp_server_defined_at SHALL 加载可解析配置后只检查mcp_servers表的原始name键，不反序列化entry；因此坏server定义仍算存在，但整个文件加载失败返回false。nearest_project_mcp_definition反向遍历项目路径取首个原始定义，可能与只合并有效entry的运行catalog来源不同。user_config_path与project_config_path只拼接路径，不验证存在或归属。

#### Scenario: Malformed nearest server definition
- **WHEN** 最近项目有坏entry且远处有有效同名entry
- **THEN** nearest定义定位选择最近坏entry，运行解析可继续使用远处有效entry。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn mcp_server_defined_at`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub(crate) fn nearest_project_mcp_definition`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn project_config_path`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn mcp_server_defined_at_checks_raw_key_presence`。

### Requirement: Shell CLI plugin registry and auxiliary config readers

CLI插件registry SHALL 加载TrustStore及effective全局plugins节，加载/反序列化失败用默认PluginsConfig；以cwd调用resolve_and_record且remote=None，再discover、populate_plugin_lists并按enabled/disabled建registry。本函数不调用resolve_effective_plugins_config项目列表overlay。management key读取仅返回effective endpoints字符串原样，不trim或认证；use_leader可选读取只认cli bool，便利形式缺失false。worktree_pool整节反序列化失败默认。

#### Scenario: Invalid plugin config section
- **WHEN** effective plugins无法反序列化
- **THEN** 使用默认discovery配置继续构建registry，不返回配置错误。

证据：`crates/codegen/shell/src/util/config/mcp.rs` — `fn load_cli_plugin_registry`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn load_management_api_key_sync`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn use_leader_from_toml_opt`；`crates/codegen/shell/src/util/config/mcp.rs` — `pub fn worktree_pool_from_toml`。

补充测试源码（未执行）：`crates/codegen/shell/src/util/config/mcp.rs` — `fn test_use_leader_opt_returns_some_false`；`crates/codegen/shell/src/util/config/mcp.rs` — `fn test_use_leader_opt_returns_none_when_absent`。


### Requirement: Shell ACP SDK MCP reverse bridge
parse_acp_mcp_servers SHALL 从meta的grow/mcp/servers数组逐项反序列化AcpServerEntry，缺失或非数组返回空；坏条目告警跳过，按name精确字符串去重保留首个成功解析条目并保持输入顺序。GatewayAcpInvoker将serverId及原message编码成grow/mcp/sdk_call ExtRequest，以调用方传入Duration包裹gateway.send，超时返回含server ID和毫秒的字符串错误，gateway或JSON解析错误转String。成功响应解析成任意JSON Value，桥接本身不核对JSON-RPC ID或结果形状，不实施重试；超时结束本地等待不证明SDK端工具执行已停止。

#### Scenario: Duplicate registration
- **WHEN** 两个成功解析条目name相同但serverId不同
- **THEN** 保留第一个，第二个告警跳过。

#### Scenario: Reverse timeout
- **WHEN** gateway请求在传入Duration内未完成
- **THEN** 返回超时字符串，不在此桥接重试或确认远端取消。

源码证据：
- `crates/codegen/shell/src/session/acp_mcp.rs` — `pub fn parse_acp_mcp_servers`。
- `crates/codegen/shell/src/session/acp_mcp.rs` — `impl AcpReverseInvoker for GatewayAcpInvoker`。
- `crates/codegen/shell/src/session/acp_mcp.rs` — `mod tests`。


### Requirement: Shell MCP startup override and pending client assembly
Shell MCP wrapper SHALL 按cwd是否存在选择项目感知或全局server config，startup timeout优先每server值否则全局resolved startup值，tool timeout、tool_timeouts和expose_image_base64仅取该server配置。resolve_overrides始终返回Some，即使找不到server配置仍携带全局startup。单server启动将override和meta传给mcp crate；批量启动先按server name收集HashMap，再交内层启动，同名key按collect覆盖而不在wrapper拒绝。build_pending_clients先await配置HTTP/stdio批次，再短锁读取pending ACP names，锁外重新读取配置，随后另一次锁同步构建ACP clients并以Ok追加到结果末尾。两次锁之间不是同一个状态快照；wrapper不重排前批错误，也不把部分失败转整批Err。

#### Scenario: No per server config
- **WHEN** server配置缺失
- **THEN** 仍向底层提供全局startup timeout，其他override为None。

#### Scenario: Mixed pending clients
- **WHEN** 配置server启动结果含Err且存在pending ACP clients
- **THEN** 保留前批结果并在末尾追加ACP Ok，不因前批错误放弃ACP组装。

源码证据：
- `crates/codegen/shell/src/session/mcp_servers.rs` — `fn resolve_overrides`。
- `crates/codegen/shell/src/session/mcp_servers.rs` — `pub async fn build_pending_clients`。
- `crates/codegen/shell/src/session/mcp_servers.rs` — `pub async fn start_mcp_servers`。
### Requirement: Pager MCP initialization progress owner update

The MCP initialization progress handler SHALL require a camelCase payload containing unsigned total, connected and sessionId fields. A successfully resolved MCP target has its session progress updated and the handler returns whether that target is active; malformed or unresolved payloads return false. This function does not itself validate connected against total, distinguish a no-op update from a mutation, log rejected payloads or schedule a catalog fetch.

#### Scenario: Routed progress
- **WHEN** the payload parses and mcp_target_agent resolves an owner
- **THEN** that session receives total and connected and active status is returned.

#### Scenario: Malformed payload
- **WHEN** a required field is absent or has the wrong type
- **THEN** the handler returns false.

#### Scenario: Unknown session
- **WHEN** target resolution fails
- **THEN** no progress is updated and false is returned.

#### Scenario: Range relationship
- **WHEN** connected exceeds total
- **THEN** this handler performs no explicit rejection.

证据：`crates/codegen/pager/src/app/acp_handler/mcp.rs` — `handle_mcp_init_progress`。

### Requirement: Pager MCP initialization completion root-only clearing

The MCP initialized handler SHALL require a camelCase sessionId, resolve it through the shared session matcher, reject child and unknown sessions, and clear initialization progress only on the matched root agent. It requests redraw only when clearing actually changed state and the matched agent is active. Malformed payloads, absent agents, already-clear state and inactive owners return false, although an inactive owner's progress can still be cleared.

#### Scenario: Active root
- **WHEN** a known active root reports initialized and progress was present
- **THEN** progress clears and true is returned.

#### Scenario: Inactive root
- **WHEN** a known inactive root reports initialized
- **THEN** progress may clear but false is returned.

#### Scenario: Child
- **WHEN** the session matcher identifies a child session
- **THEN** the notification is dropped.

#### Scenario: No mutation
- **WHEN** the root progress was already clear
- **THEN** false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/mcp.rs` — `handle_mcp_initialized`。

### Requirement: Pager MCP server status modal patch and owner-scoped refetch

The MCP server-status handler SHALL parse the shell's canonical payload, route by sessionId, and warn then reject malformed input. When tools are omitted for ConfigAdded, ConfigRemoved or ConfigChanged, an open modal with no pending fetch for that agent and a current session id schedules one owner-scoped FetchMcpsList effect. If the target modal is open with loaded server data, Ready, Initializing or Unavailable and any supplied tool detail are passed to `patch_server_row`; closed, loading or errored modals skip the patch. The return value requests redraw only for an active owner when either a refresh was scheduled or the named row mutated; an absent named server can still return true through the scheduled refresh path.

#### Scenario: Catalog delta
- **WHEN** a qualifying config reason omits tools for an open modal
- **THEN** one fetch is scheduled unless that agent already has a pending fetch.

#### Scenario: Loaded row
- **WHEN** the matched modal has loaded server data
- **THEN** the named row is offered mapped status and optional replacement tools.

#### Scenario: Unavailable surface
- **WHEN** the modal is closed or its data is loading or errored
- **THEN** the row patch is skipped.

#### Scenario: Redraw ownership
- **WHEN** the matched owner is inactive
- **THEN** state/effects may update but the handler returns false.

#### Scenario: Malformed status
- **WHEN** canonical payload parsing fails
- **THEN** a bounded warning is emitted and false is returned.

证据：`crates/codegen/pager/src/app/acp_handler/mcp.rs` — `handle_mcp_server_status`、`agent_has_pending_mcps_fetch`。

### Requirement: Shell crates/codegen/shell/src/extensions/mcp.rs extension method and user-facing command boundary contract

crates/codegen/shell/src/extensions/mcp.rs SHALL 维护 extension method and user-facing command boundary 的入口 PREFIX, LIST, READ_RESOURCE, SETUP, TOGGLE, TOGGLE_TOOL, UPSERT, DELETE, INIT_PROGRESS, McpListRequest, McpListResponse, McpServerEntry, McpServerConfig, McpEnvVar, McpServerSessionState, McpSessionStatus, McpToolEntry, default_true (plus 46 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PREFIX, LIST, READ_RESOURCE, SETUP, TOGGLE, TOGGLE_TOOL, UPSERT, DELETE, INIT_PROGRESS, McpListRequest, McpListResponse, McpServerEntry, McpServerConfig, McpEnvVar, McpServerSessionState, McpSessionStatus, McpToolEntry, default_true (plus 46 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/extensions/mcp.rs`。

### Requirement: Shell crates/codegen/shell/src/mcp_doctor.rs shell module boundary contract

crates/codegen/shell/src/mcp_doctor.rs SHALL 维护 shell module boundary 的入口 ConfigSourceStatus, ConfigSourceState, McpServerStatus, Check, pass, fail, fail_no_hint, DoctorReport, DiscoveredServer, discover_servers, resolve_command, check_command_exists, check_server_start, check_handshake, check_tools_list, format_mcp_error, describe_server, check_server (plus 6 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** ConfigSourceStatus, ConfigSourceState, McpServerStatus, Check, pass, fail, fail_no_hint, DoctorReport, DiscoveredServer, discover_servers, resolve_command, check_command_exists, check_server_start, check_handshake, check_tools_list, format_mcp_error, describe_server, check_server (plus 6 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/mcp_doctor.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/mcp.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/mcp.rs SHALL 维护 session actor lifecycle and notifications 的入口 wait_for_mcp_initialized, register_shared_client_tools, register_mcp_tool, emit_mcp_catalog_updates, refresh_mcp_snapshot_and_schedule_reminder, persist_announcement_state, maybe_inject_mcp_reminder, is_stdio_server_configured, is_http_server_configured, reset_http_client, unregister_server_tools, respawn_stdio, maybe_inject_mcp_connecting_reminder, ensure_mcp_tools_initialized, str, McpErrorCategory, connected_server_summaries, rendered_mcp_hint (plus 2 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、child process lifecycle、timeout/deadline or timing decisions、session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** wait_for_mcp_initialized, register_shared_client_tools, register_mcp_tool, emit_mcp_catalog_updates, refresh_mcp_snapshot_and_schedule_reminder, persist_announcement_state, maybe_inject_mcp_reminder, is_stdio_server_configured, is_http_server_configured, reset_http_client, unregister_server_tools, respawn_stdio, maybe_inject_mcp_connecting_reminder, ensure_mcp_tools_initialized, str, McpErrorCategory, connected_server_summaries, rendered_mcp_hint (plus 2 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

证据：`crates/codegen/shell/src/session/actor/mcp.rs`。

### Requirement: Shell crates/codegen/shell/src/session/actor/mcp_snapshot.rs session actor lifecycle and notifications contract

crates/codegen/shell/src/session/actor/mcp_snapshot.rs SHALL 维护 session actor lifecycle and notifications 的入口 MCP_INIT_CANCELLED_CONFIG_CHANGED, from_env, refresh_mcp_snapshot_and_schedule_reminder_with, restore_mcp_tools_from_snapshot。实现显示该边界包含 session/timeline state projection、MCP integration boundary；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Primary module path
- **WHEN** 调用 MCP_INIT_CANCELLED_CONFIG_CHANGED, from_env, refresh_mcp_snapshot_and_schedule_reminder_with, restore_mcp_tools_from_snapshot 的主入口
- **THEN** 按源码声明的转换或调度路径返回结果。

证据：`crates/codegen/shell/src/session/actor/mcp_snapshot.rs`。

### Requirement: Shell crates/codegen/shell/src/session/mcp_catalog.rs session timeline and state model contract

crates/codegen/shell/src/session/mcp_catalog.rs SHALL 维护 session timeline and state model 的入口 normalize_url, mcp_server_key, mcp_server_name, merge_mcp_servers, merge_and_send_mcp_update, merge_mcp_servers_sourced, load_plugin_mcp_servers, load_plugin_mcp_servers_from_value, load_plugin_mcp_servers_from_config, empty_cwd, client_provided_servers_survive_merge, disabled_grow_config_server_is_excluded, project_mcp_server_reports_project_source, untrusted_workspace_drops_project_mcp_servers, repo_with_project_server, load_plugin_mcp_creates_stdio_server_with_env_substitution, load_plugin_mcp_disabled_server_excluded_from_merge, load_plugin_mcp_from_value_accepts_direct_map (plus 2 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、filesystem or durable record I/O、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、session/timeline state projection；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Durable boundary
- **WHEN** normalize_url, mcp_server_key, mcp_server_name, merge_mcp_servers, merge_and_send_mcp_update, merge_mcp_servers_sourced, load_plugin_mcp_servers, load_plugin_mcp_servers_from_value, load_plugin_mcp_servers_from_config, empty_cwd, client_provided_servers_survive_merge, disabled_grow_config_server_is_excluded, project_mcp_server_reports_project_source, untrusted_workspace_drops_project_mcp_servers, repo_with_project_server, load_plugin_mcp_creates_stdio_server_with_env_substitution, load_plugin_mcp_disabled_server_excluded_from_merge, load_plugin_mcp_from_value_accepts_direct_map (plus 2 additional private symbols) 执行文件或持久记录读写
- **THEN** 沿实现的读写、解析、发布和失败路径返回，不把内存状态当作已落盘。

#### Scenario: Async lifecycle
- **WHEN** normalize_url, mcp_server_key, mcp_server_name, merge_mcp_servers, merge_and_send_mcp_update, merge_mcp_servers_sourced, load_plugin_mcp_servers, load_plugin_mcp_servers_from_value, load_plugin_mcp_servers_from_config, empty_cwd, client_provided_servers_survive_merge, disabled_grow_config_server_is_excluded, project_mcp_server_reports_project_source, untrusted_workspace_drops_project_mcp_servers, repo_with_project_server, load_plugin_mcp_creates_stdio_server_with_env_substitution, load_plugin_mcp_disabled_server_excluded_from_merge, load_plugin_mcp_from_value_accepts_direct_map (plus 2 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/mcp_catalog.rs`。

### Requirement: Shell crates/codegen/shell/src/session/mcp_dispatcher.rs session timeline and state model contract

crates/codegen/shell/src/session/mcp_dispatcher.rs SHALL 维护 session timeline and state model 的入口 COALESCE_WINDOW, SERVER_STATUS_METHOD, McpServerStatusPayload, surfaced, McpServerStatus, McpServerStatusReason, documented, ShutdownState, mark, is_shutting_down, forget, begin_restart, end_restart, has_in_flight_restarts, SharedShutdownState, new_shutdown_state, CoalescedWindow, collect_window (plus 50 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** COALESCE_WINDOW, SERVER_STATUS_METHOD, McpServerStatusPayload, surfaced, McpServerStatus, McpServerStatusReason, documented, ShutdownState, mark, is_shutting_down, forget, begin_restart, end_restart, has_in_flight_restarts, SharedShutdownState, new_shutdown_state, CoalescedWindow, collect_window (plus 50 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** COALESCE_WINDOW, SERVER_STATUS_METHOD, McpServerStatusPayload, surfaced, McpServerStatus, McpServerStatusReason, documented, ShutdownState, mark, is_shutting_down, forget, begin_restart, end_restart, has_in_flight_restarts, SharedShutdownState, new_shutdown_state, CoalescedWindow, collect_window (plus 50 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/mcp_dispatcher.rs`。

### Requirement: Shell crates/codegen/shell/src/session/mcp_dispatcher_e2e_tests.rs session timeline and state model contract

crates/codegen/shell/src/session/mcp_dispatcher_e2e_tests.rs SHALL 维护 session timeline and state model 的入口 PAST_WINDOW, E2eActions, new, configure, configure_http, script_reset, reset_calls, unconfigure, script, respawn_calls, pushes, pushes_with_reason, is_stdio_server_configured, is_in_shutting_down, respawn_stdio, push_status, is_http_server_configured, reset_http_client (plus 18 additional private symbols)。实现显示该边界包含 explicit error/result paths、async task lifecycle and cancellation、channel or acknowledgement flow、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** PAST_WINDOW, E2eActions, new, configure, configure_http, script_reset, reset_calls, unconfigure, script, respawn_calls, pushes, pushes_with_reason, is_stdio_server_configured, is_in_shutting_down, respawn_stdio, push_status, is_http_server_configured, reset_http_client (plus 18 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** PAST_WINDOW, E2eActions, new, configure, configure_http, script_reset, reset_calls, unconfigure, script, respawn_calls, pushes, pushes_with_reason, is_stdio_server_configured, is_in_shutting_down, respawn_stdio, push_status, is_http_server_configured, reset_http_client (plus 18 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/mcp_dispatcher_e2e_tests.rs`。

### Requirement: Shell crates/codegen/shell/src/session/mcp_restart.rs session timeline and state model contract

crates/codegen/shell/src/session/mcp_restart.rs SHALL 维护 session timeline and state model 的入口 BACKOFF, HTTP_RECOVERY_BACKOFF, SkipReason, as_label, str, so, RestartActions, is_stdio_server_configured, is_in_shutting_down, respawn_stdio, push_status, begin_restart, end_restart, is_http_server_configured, reset_http_client, unregister_server_tools, maybe_schedule_restart, RestartInFlightGuard (plus 52 additional private symbols)。实现显示该边界包含 serde-backed wire/config types、explicit error/result paths、async task lifecycle and cancellation、child process lifecycle、platform or feature-gated branches、timeout/deadline or timing decisions；函数按源码显式的返回值、错误分支和状态转换交付结果，未在本条之外推断调用方契约。

#### Scenario: Error result
- **WHEN** BACKOFF, HTTP_RECOVERY_BACKOFF, SkipReason, as_label, str, so, RestartActions, is_stdio_server_configured, is_in_shutting_down, respawn_stdio, push_status, begin_restart, end_restart, is_http_server_configured, reset_http_client, unregister_server_tools, maybe_schedule_restart, RestartInFlightGuard (plus 52 additional private symbols) 中的输入触发显式错误分支
- **THEN** 返回或传播源码声明的错误结果，并保留已完成的前置状态变化。

#### Scenario: Async lifecycle
- **WHEN** BACKOFF, HTTP_RECOVERY_BACKOFF, SkipReason, as_label, str, so, RestartActions, is_stdio_server_configured, is_in_shutting_down, respawn_stdio, push_status, begin_restart, end_restart, is_http_server_configured, reset_http_client, unregister_server_tools, maybe_schedule_restart, RestartInFlightGuard (plus 52 additional private symbols) 启动异步任务或等待通道结果
- **THEN** 按源码的完成、关闭、取消或回执分支结束；没有额外推定硬超时。

#### Scenario: Platform branch
- **WHEN** 编译条件选择对应平台/feature实现
- **THEN** 只执行该条件下的源码分支；其他平台行为须由对应构建验证。

证据：`crates/codegen/shell/src/session/mcp_restart.rs`。
