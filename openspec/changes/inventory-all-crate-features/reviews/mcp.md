# mcp 逐包核查

包路径：`crates/codegen/mcp`。全部所属 Rust 模块和 Cargo.toml 已阅读；动态测试及边界见本文件末尾。

## 模块与开关

- `crates/codegen/mcp/Cargo.toml`
- `crates/codegen/mcp/src/acp_transport.rs`
- `crates/codegen/mcp/src/lib.rs`
- `crates/codegen/mcp/src/liveness.rs`
- `crates/codegen/mcp/src/mcp_http_client.rs`
- `crates/codegen/mcp/src/servers.rs`
- `crates/codegen/mcp/src/wire.rs`
- `crates/codegen/mcp/tests/repro_sse_flood.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [MCP transport ownership](../specs/mcp-integration/spec.md#requirement-mcp-transport-ownership)：mcp SHALL re-export rmcp 并提供 stdio、streamable HTTP 及 ACP reverse bridge；构建使用关闭默认 feature 的 rmcp 2.2 和 reqwest 0.13 no-provider TLS 配置。
- [MCP ACP wire constants](../specs/mcp-integration/spec.md#requirement-mcp-acp-wire-constants)：MCP-over-ACP SHALL 使用 grow/mcp/call 转发调用、grow/mcp/sdk_call 反向调用、grow/mcp/servers 注册列表和 grow/mcp/sdk 能力标记。
- [ACP MCP half duplex bridge](../specs/mcp-integration/spec.md#requirement-acp-mcp-half-duplex-bridge)：ACP bridge SHALL 逐行解析请求，丢弃空行、坏 JSON 及缺失或 null id 的通知；每个有效请求通过独立 invoke task 转交 SDK，响应由单 writer 写回。
- [ACP response correlation and errors](../specs/mcp-integration/spec.md#requirement-acp-response-correlation-and-errors)：bridge SHALL 将响应 object 的 id 覆盖为请求 id，invoker Err 或非 object 响应转 JSON-RPC -32603；其它响应 schema 不在此验证。
- [ACP bridge concurrency and teardown](../specs/mcp-integration/spec.md#requirement-acp-bridge-concurrency-and-teardown)：bridge SHALL 使用两个 256 KiB duplex、128 项响应 channel 和 JoinSet；完整 read_line 后回收完成任务，reader 结束时 abort 剩余 invokes。
- [ACP reverse timeout budget](../specs/mcp-integration/spec.md#requirement-acp-reverse-timeout-budget)：ACP handshake SHALL 给 invoker 传 max(startup timeout, server tool timeout) 作为每轮 reverse budget；bridge 只透传预算，外层 serve 和运行时 tool call 另有 timeout。
- [MCP pool initialization state](../specs/mcp-integration/spec.md#requirement-mcp-pool-initialization-state)：InitProgress SHALL 使用 NotStarted、Starting、Finished 与 handshaking set；只有 Finished 且 set 为空才算 initialized。
- [MCP init failure registry](../specs/mcp-integration/spec.md#requirement-mcp-init-failure-registry)：McpState SHALL 单独保存 init_failed 原因，fresh mark_servers_initializing 清对应旧失败；record/clear 与握手集合状态分别管理。
- [MCP full config reset](../specs/mcp-integration/spec.md#requirement-mcp-full-config-reset)：update_configs SHALL 按配置 Vec JSON 比较，变化时清 owned clients、事件 authority、工具 meta 和 disabled registrations，取消 init 并增加 generation；保留 shared clients、ACP registry 和独立权限配置。
- [MCP differential config replacement](../specs/mcp-integration/spec.md#requirement-mcp-differential-config-replacement)：配置 diff SHALL 按 server name 的序列化配置识别 added/removed/retained，同名变化同时 removed 与 added；只删除受影响 owned，未变 owned 保留其事件 episode。
- [MCP owned and shared client lookup](../specs/mcp-integration/spec.md#requirement-mcp-owned-and-shared-client-lookup)：McpState SHALL 在同名时 owned 优先 shared；SharedMcpPool 保存 Arc 客户端及配置快照，并携带 live eligibility authority。
- [MCP live inherited eligibility](../specs/mcp-integration/spec.md#requirement-mcp-live-inherited-eligibility)：SharedMcpEligibility SHALL 逐层检查 server scope、客户端 incarnation 及工具成员资格；上游同名替换或撤权可使后代当前权限立即失效。
- [MCP inheritance filtering and reconciliation](../specs/mcp-integration/spec.md#requirement-mcp-inheritance-filtering-and-reconciliation)：restrict/exclude SHALL 累积收窄继承 server scope，reconcile 从 live authority 重建 shared map 并排除本地 configs 名称；返回前后涉及的名称供调用方注销旧定义。
- [MCP eligibility publication generations](../specs/mcp-integration/spec.md#requirement-mcp-eligibility-publication-generations)：McpEligibilityAuthority SHALL 对客户端 ID 集合及工具集合改变递增 generation，不改变则保持；shared generation 为本层与上游 wrapping sum。
- [MCP client event authority](../specs/mcp-integration/spec.md#requirement-mcp-client-event-authority)：MCP client-origin 事件 SHALL 携带私有构造的 client_id、config_generation、transport_revision；admission 比较当前 authority 的完整 episode。
- [MCP event channel ownership](../specs/mcp-integration/spec.md#requirement-mcp-event-channel-ownership)：McpState SHALL 内部创建 unbounded 事件 channel，仅公开 receiver；给 owned clients 绑定 sender，shared clients 保持父级事件所有权。
- [MCP typed lifecycle events](../specs/mcp-integration/spec.md#requirement-mcp-typed-lifecycle-events)：McpState SHALL 从真实配置 transition 发布 ConfigDiff，并提供按当前 server authority 标记 ToolsChanged、TransportClosed、HandshakeFailed 的入口。
- [MCP SDK server registry](../specs/mcp-integration/spec.md#requirement-mcp-sdk-server-registry)：ACP registry SHALL 将 name/serverId 注册表和 invoker 一起保存，跨普通 config reset 保留；每次 init 依据调用方传入 override 构造尚无 owned/shared 同名客户端。
- [MCP timeout precedence](../specs/mcp-integration/spec.md#requirement-mcp-timeout-precedence)：MCP timeout SHALL 采用 meta 毫秒向上取整为秒，其次外部秒配置，再用 startup30/tool6000 默认；per-tool meta 覆盖同名外部 per-tool，再回退 server 默认。
- [MCP metadata parsing](../specs/mcp-integration/spec.md#requirement-mcp-metadata-parsing)：parse_mcp_meta_config SHALL 从 _meta.mcpConfig 解析 camelCase 可选 startupTimeoutMs、toolTimeoutMs、toolTimeoutsMs、exposeImageBase64，缺失或整体非法返回空 map。
- [MCP single flight handshake](../specs/mcp-integration/spec.md#requirement-mcp-single-flight-handshake)：ensure_initialized SHALL 在 Pending 时由一个调用者持 transport 执行握手，其余 Initializing 调用者等待 Notify；Ready 返回缓存 service，Empty 返回无 transport 错误。
- [MCP handshake restoration boundary](../specs/mcp-integration/spec.md#requirement-mcp-handshake-restoration-boundary)：握手结果 SHALL 成功进入 Ready，失败的 HTTP/ACP 恢复 Pending，stdio 进入 Empty；取消时 InitGuard 对可恢复 transport 尝试 try_lock 还原并通知。
- [MCP protocol and client capabilities](../specs/mcp-integration/spec.md#requirement-mcp-protocol-and-client-capabilities)：MCP initialize SHALL 显式使用协议2025-06-18，客户端名 grow-shell-<server> 与 version::VERSION，声明 UI extension MIME text/html;profile=mcp-app。
- [MCP transport recovery](../specs/mcp-integration/spec.md#requirement-mcp-transport-recovery)：HTTP/ACP SHALL 可由构造时保存的地址状态 reset→ensure→arm 恢复；reset 与每次 handshake 增加 transport revision，stdio 不能从已消费 child 在此重新启动。
- [MCP liveness observation](../specs/mcp-integration/spec.md#requirement-mcp-liveness-observation)：is_healthy 和 liveness_check SHALL 只读 state 与 rmcp transport closed 标记，不执行网络握手；Ready/open 健康，非Ready属于 transient。
- [MCP one shot liveness watcher](../specs/mcp-integration/spec.md#requirement-mcp-one-shot-liveness-watcher)：非ACP客户端有 sender 且 Ready、slot为空时 SHALL 可启动 liveness poller，默认500ms、首tick立即、错过tick跳过；closed发一次事件，transient静默退出，两者清slot。
- [MCP HTTP headers and compatibility](../specs/mcp-integration/spec.md#requirement-mcp-http-headers-and-compatibility)：HTTP transport SHALL 逐项解析 headers，非法项警告并跳过，重复项后者覆盖；Figma名字或figma.com子域且未设UA时补grow-cli。
- [MCP session header expansion](../specs/mcp-integration/spec.md#requirement-mcp-session-header-expansion)：HTTP header value SHALL 支持 {{session_id}} 与 ${session_id} 替换；无session时丢弃含任一占位符的整项。
- [MCP SSE reconnect backoff](../specs/mcp-integration/spec.md#requirement-mcp-sse-reconnect-backoff)：McpHttpClient SHALL 只在 get_stream 前按距上次成功建立 <2 秒判定rapid，第二次rapid开始500ms指数退避至30秒；POST/delete直接委托。
- [MCP reconnect warning budget](../specs/mcp-integration/spec.md#requirement-mcp-reconnect-warning-budget)：SSE warning SHALL 每个episode至多一次，并使用可跨客户端重建共享的1小时WarnBudget；被冷却抑制的episode可在后续边界补一次warn。
- [MCP qualified tool registration](../specs/mcp-integration/spec.md#requirement-mcp-qualified-tool-registration)：MCP qualified ID SHALL 恰有一个 overlap-aware __ 且两段非空并满足ToolId，注册另检查完整ASCII名称字母/下划线起始、仅字母数字下划线连字符且最多64字符。
- [MCP tool visibility and schema](../specs/mcp-integration/spec.md#requirement-mcp-tool-visibility-and-schema)：工具注册 SHALL 保留meta，缺description为空；schema缺type/properties时补object/空对象，已有字段不覆盖。ui.visibility数组只有含model才model-visible，其它类型默认可见。
- [MCP tool directory pagination](../specs/mcp-integration/spec.md#requirement-mcp-tool-directory-pagination)：工具列表和描述文件 SHALL 分页读取直到next_cursor=None，先收集全量内容；本层不限制页数、不检测重复游标且无独立list timeout。
- [MCP tool descriptor materialization](../specs/mcp-integration/spec.md#requirement-mcp-tool-descriptor-materialization)：materialize_descriptors SHALL 把原name/description/inputSchema写入 server_dir/tools/<sanitized>.json，使用spawn_blocking和逐文件temp+persist原子替换；资源不落地。
- [MCP server instruction access](../specs/mcp-integration/spec.md#requirement-mcp-server-instruction-access)：server_instructions SHALL 仅从Ready peer_info读取非空白instructions并返回原文本，不触发init；调试call_tool直接ensure后调用MCP。
- [MCP runtime tool timeout and retry](../specs/mcp-integration/spec.md#requirement-mcp-runtime-tool-timeout-and-retry)：运行时McpErasedTool SHALL 在ensure之后为tools/call设置per-tool秒timeout；transport闭合/发送错误可恢复一次，HTTP业务RPC除四个客户端错误码及auth拒绝外也可恢复一次。
- [MCP auth error classification](../specs/mcp-integration/spec.md#requirement-mcp-auth-error-classification)：McpError SHALL 对指定auth词及右侧非ASCII字母数字的上下文401模式作字符串分类；typed Spawn/Timeout不归认证拒绝。
- [MCP runtime content projection](../specs/mcp-integration/spec.md#requirement-mcp-runtime-content-projection)：MCP业务错误 SHALL 聚合Text为errored输出；正常结果聚合Text、Image及Resource，image blob转data URI，其它Resource序列化JSON，未匹配内容不输出。
- [MCP image base64 exposure](../specs/mcp-integration/spec.md#requirement-mcp-image-base64-exposure)：图片 SHALL 默认只发data URI，exposeImageBase64按meta>override>false决定是否追加raw base64 wrapper。
- [MCP resilient stdio framing](../specs/mcp-integration/spec.md#requirement-mcp-resilient-stdio-framing)：ResilientRwTransport SHALL 按LF读取JSON-RPC，去末CR、跳空行，坏行继续读取且不回复错误；未知缺id且method字符串通知静默忽略。
- [MCP stdio process lifecycle](../specs/mcp-integration/spec.md#requirement-mcp-stdio-process-lifecycle)：stdio SHALL 配管道、kill_on_drop和detach，best-effort登记ProcessGroup/Scope；关闭stdin后给3秒退出，再杀进程组并回收leader。
- [MCP stdio spawn planning](../specs/mcp-integration/spec.md#requirement-mcp-stdio-spawn-planning)：Windows裸程序名 SHALL 用PATH解析launcher，带分隔符或非Windows直接使用；显式env覆盖继承env，当前cwd继承，批量启动buffer_unordered8。
- [MCP stderr log capture](../specs/mcp-integration/spec.md#requirement-mcp-stderr-log-capture)：stdio stderr SHALL 复制到grow_home/logs/mcp/<sanitized96chars>.stderr.log，每spawn截断，打开后后台copy。

## 边界

- 两者均创建 streamable HTTP pending client；未知 transport variant 返回错误，不实现独立旧 SSE 协议栈。
- forward/reverse 使用不同方法字符串，schema 不能混用。
- 当前桥接不提供这些方向；initialized 无 id 也在本地丢弃。
- 构造带原 id 的 Internal error，避免无可关联响应；合法 object 不保证含 result/error。
- 其它请求可以先响应，读取不因任务回收而取消半条 line；缓冲容量不等于请求总长度或 active task 数硬上限。
- 外层按 per-tool 计算，但 reverse budget 不包含该 override，不能承诺 invoker 不提前超时。
- 保留未完成 set，is_initializing 仍真；mark_server_ready 仅移除该名称，成功与失败原因分开记录。
- 先清旧 failure，随后 InitProgress 忽略新增握手集合并警告；不能据此认定初始化已启动。
- 仍判为变化；不变则返回 false，不增加 generation。
- retained owned 的原 client/config episode 仍有效；retained-but-not-owned 的在途 authority 被撤销。
- 获得当前 map 或快照中的 Arc；这些访问器本身不执行 live eligibility、disabled tool 或 access ceiling 检查。
- 后代拒绝旧 incarnation；父级 reconcile 并 publish 后才恢复新绑定资格。
- 若固定 scope 允许，则 live current_clients 可接纳；config/meta 快照不随过滤同步删改。
- 视为 transport 资格改变；generation 与锁 poison/耗尽行为不能当持久化或跨进程身份机制。
- 旧 handler 固定的 revision 不会被当前共享 sender 重新标记为新 transport，dispatcher 可拒绝旧事件。
- fanout 遍历已 owned clients，不遍历 shared 或尚未 owned 的在途实例；发送失败不回滚状态。
- typed client emit 返回 false；config 事件不携带 client episode。
- 不为该注册构造 pending ACP client；本方法不去重同批重复 registration。
- 分别解析为 1 秒或 0 秒；tool_timeout_for 注释中的默认60秒不是当前实现。
- 整 map 反序列化失败并回退空，不逐条保留其它合法配置。
- 返回 init still in progress 错误，不在这个超时分支恢复 state；Ready 分支不检测 transport 是否已关闭。
- 不保证恢复；guard disarm 后至发布结果前的等待也不受其保护，不承诺所有取消路径无卡住状态。
- 此处仍使用显式版本；声明 MIME 不等于本包实现完整 UI 渲染。
- state_kind 与 reset 分别加锁，结果发布也未比较 revision；当前不据注释承诺任意并发恢复只发生一次。
- 本地健康谓词可能仍返回 true，不是远端主动探活。
- CancellationToken 取消 poller；ACP不主动watch，依赖惰性恢复；Ready+closed实测不由Empty stub测试证明。
- 本包无交互式OAuth或credential refresh；重建仍使用显式headers快照。
- 当前顺序两次replace可再次展开；不是统一single-pass模板。
- 失败不更新建立时间；>=2秒间隔重置episode，此wrapper不主动发起重试或限制重试总数。
- 仍只debug；恢复后新episode重新检查共享cooldown。
- 跳过注册并日志；不同server的同raw tool得到不同qualified运行时ID。
- model_visible=false；此包返回registration，实际model/UI派发门禁由调用者接入。
- 当前循环没有本地终止门槛；不能把握手startup timeout套到list阶段。
- 不清理旧文件，碰撞可覆盖；返回成功写次数而非唯一文件数，不提供整目录事务。
- 不获得本地per-tool timeout、recovery和内容转换；非object args被置None。
- 不重试可能有副作用的操作；HTTP reset供下次用，返回ToolError；恢复失败保留原错误，重试失败返回第二次错误。
- 不匹配；403混authentication文字仍可能匹配，不能声明所有403都被排除。
- 当前该run分支不保留这些部分；transport ToolError提前返回，尾部McpToolCalled诊断和MCPOutput标志不产生。
- 额外mcp_image_base64块不含data:image前缀；本函数不验证mime/base64，也不证明下游图像提取已成功。
- tracing warn最多取200个字符sample，不是200字节；当前没有注释所称独立session decode事件，也不限制输入整行长度。
- 尝试cleanup线程与临时runtime；创建失败降级日志/信号，不承诺任意故障或取消路径永不遗留进程。
- 调用helper前已有lossy转换；批量结果按完成顺序，不保证原输入顺序。
- 文件可能共用且无大小上限/轮转；目录或文件打开失败则不继续drain。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 逐次阅读证据

# mcp 逐包审阅（进行中）

当前完成 Cargo.toml、lib.rs、wire.rs、acp_transport.rs 735 行、liveness.rs 357 行、mcp_http_client.rs 495 行及 tests/repro_sse_flood.rs 216 行。servers.rs 7660 行仅阅读至 230 行，包保持 pending，尚未生成最终功能映射或运行本包测试。

## 包与 wire

- lib re-export rmcp，公开 acp_transport/liveness/mcp_http_client/servers/wire。manifest rmcp 2.2 关闭默认 feature，开启 client、async-rw、streamable HTTP reqwest 及 no-provider TLS；reqwest 0.13 关闭默认 feature，启用 blocking/json/rustls-no-provider/stream。依赖 tools 等大型链路，验证须控制独立 target 磁盘。
- wire 固定四项 grow/mcp/call、grow/mcp/sdk_call、grow/mcp/servers、grow/mcp/sdk；forward 与 reverse 方法和 schema 分开。这里只定义常量，不能据注释声称 SDK 实现已核对。

## ACP bridge 全文

- 两条 256 KiB duplex，响应 channel 容量 128；每请求独立 JoinSet task，单 writer 防止响应字节交错。read_line 完整完成后才 try_join_next 回收，不用可取消 select 中途丢 line。
- 空行略过，JSON 解析失败 warn 后丢弃；缺 id 或 null id 在本地丢弃，包括 initialized 通知。没有 server-initiated notifications/request 通道，不支持一般双向 sampling/roots/elicitation。非空 id 的类型没有在此校验。
- invoke_timeout 仅作为参数传给 AcpReverseInvoker::invoke，本 bridge 没有 tokio timeout 包裹；实现者必须执行预算。错误转 -32603，非 object 响应也转此错误；object 只覆盖 id，不校验 jsonrpc/result/error schema。
- reader EOF/error 返回时 drop JoinSet abort invoke；writer 写/flush 错返回，但 join!(reader,writer) 不反向取消仍读请求的 reader。正常两端都关闭的 teardown 测试不证明单向关闭也立即退出。
- 256 KiB duplex 是流缓冲，不限制 read_line 字符串长度；128 限响应队列，不限制该模块 active invoke 数或每条响应字节。注释依赖 rmcp 自身并发限制，须核对实际调用方，不能直接宣称全流程有界。
- 八项测试阅读：通知与响应、慢快请求乱序、chunk 中途另请求完成、invoke error、非object、4242 秒预算透传、双方 drop 后 abort future、真实 rmcp handshake/list/call+cancel。chunk fixture 仅故意拆两次写，不实际大于 reader 缓冲；timeout 测试仅检查透传，不验证强制截止。尚未运行。

## liveness 全文

- 默认 500 ms；interval 首 tick 立即，显式 Skip missed ticks。spawn 时固定 transport_revision，发事件时读取当前 sink/配置绑定；Healthy 继续，Ready+closed 清 slot 发一次 TransportClosed 后退出，Transient 清 slot 无事件退出。
- DropGuard 取消；cancel 分支不清 slot。清 slot 先 take 再锁外 drop，不核对 handle 身份；是否可能旧 watcher 清新 slot 须结合 arm/revision 逻辑确认。tick 分支中的 liveness_check().await 不被外层 select 的 cancel 同时抢占，不能声称任意锁等待都立即退出。
- 三项测试使用 Empty stub 和 paused time，两个证明静默清 slot，第三 drop 后无事件；第三注释长 interval 延迟首 tick 不成立（interval 首 tick 总是立即），无事件不能区分 cancellation 与 Empty transient。Ready+closed 真链路声称在 servers.rs，待读。

## HTTP backoff 全文

- McpHttpClient 泛型 wrapper 只处理 get_stream；post_message/delete_session 参数原样委托。clone 共享 ThrottleState；单独 WarnBudget clone 跨 wrapper rebuild 保留警告冷却。
- 用下一次 GET 时刻与上次成功建立时刻之差 <2s 近似快速死亡，不直接观察 stream death。连续快速次数 saturating 增，前两次 GET（初次+第一次 rapid）无延迟；rapid attempt 2 从 500 ms 起指数增加，最大 30 秒。sleep 发生在 inner GET 前，成功后才更新 last_established；连接失败不更新。该策略不判断 body 具体错误、不设置最大重试次数，也不主动发起重试。
- >=2s 重置 consecutive 与 episode_warned。每 episode 至多一条 warn，并有共享 1h 冷却；若 episode 开始被冷却抑制，后续达到冷却边界可补一次 warn；成功恢复前已发 warn 的同 episode 不因一小时到期重复 warn。其余 debug 包含 suppressed_warn。时间来自 tokio Instant 转 std，paused test 能控制。
- 四项本模块测试覆盖跨 episode/冷却、抑制后恢复、rebuild 共享 budget、真实 wrapper 日志等级；MockInner 总返回空 stream，不覆盖 inner GET error、并发 clone 或 POST/delete 参数透传。
- 黑盒三个集成测试用本机随机端口 fake MCP，ctor 安装 ring provider。无 wrapper 观察 3 秒 GET>20 来证明依赖漏洞仍存在；wrapper 1.2 秒<=4、4秒总3..8；健康3秒恰好1次且 tools/list 返回echo。数 GET 而非实际 WARN，未捕获上游日志。server task 无显式 shutdown，由测试 runtime 收尾；wall-clock 阈值并非全部网络环境时序保证。尚未运行。

## servers.rs 起始 1–230

- validate_tool_name 本地固定 ASCII regex ^[a-zA-Z_][a-zA-Z0-9_-]{0,63}$；provider 限制注释不作为外部最新事实。空名称单独错误。sanitize_descriptor_segment 保留 ASCII alnum、-、_、.，其他每字符替 _，空串变 _；因此 . 或 .. 本身仍保留，是否落地逃逸须后续 materialize 调用方确认。
- InitProgress 三态 NotStarted/Starting/Finished，后两者携带 handshaking set；Finished 并不等于全部完成，只有空 set 才 is_complete；try_start 只从 NotStarted 成功。finish 方法后半与其余实现尚待读。

本轮未启动 Cargo，target 保持此前 clean 状态。发现的未证实架构疑点留在审阅，不混入运行时代码修改。

## servers.rs 231–2770 阅读

- InitProgress finish 只将 Starting 变 Finished 并保留 set，NotStarted warn/no-op；cancel 全清，mark_handshaking 在未启动时忽略，complete 无论成功失败都只移除名称。McpState 的 ready 方法代表握手结算，不等于工具目录加载成功；init_failed 另存失败文字。
- AcpServerEntry serde name/serverId；registry 将 entries 与 invoker 一起保存，set_acp_servers 不在此去重。pending 仅排除 owned/shared map 已有名称，未判定同批重复项；timeout overrides 由调用方每次传入，不在 state 锁内读配置。普通 config 更新保留 ACP registry。
- McpEligibilityAuthority 用 std RwLock 保存 generation、当前 clients 和 qualified_tools；相同 client_id 集合与 tool 集合不增代次，改变 checked_add，poison/exhaust panic。replace_clients 先读工具集合再单独 replace，不是持同一写锁的 read-modify-write；并发发布语义待调用方核对。
- SharedMcpEligibility 保留 scope 与 upstream 全链，工具必须 qualified name 合法、current client 存在且所有 authority 含工具；客户端 incarnation 必须与上游一致，同名不够。generation 为本层与上游 wrapping_add，不是永久唯一 token。current_clients 先 snapshot 列表后逐项重新查 current 身份。
- owned 优先 shared；McpState 的 get_client/all_clients 本身不检查 inherited eligibility。reconcile 清 shared 后按 live authority 重建，排除 configs 声明名称（即使 owned 尚未就绪）；调用方需在发现/派发边界调用权限判断，不能单凭这些 map 访问器声称实时撤权。
- SharedMcpPool from_state 发布当前 transports，保存客户端 Arc/config/meta 快照与 live authority。restrict/exclude 累积收窄 scope，仅过滤 clients map，configs/meta 保留。retain_clients 实际经 restrict 更新 live scope，与其旧注释“only clients map”不符；import 实际走 live current_clients，不是旧注释所称只迭代 snapshot clients。
- 事件 channel 为 unbounded，只返回 receiver；安装/撤销只给 owned clients 传 sender，shared 保留父 session。pending-but-not-owned 已绑定 client 不在该 fanout 中，是否同时关闭要看后续生命周期。bind_client_events 在握手前指定 server sole authority；revoke 仅匹配整个 episode 才移除；admission 比较 client/config/transport 三元身份，transport revision 由共享 atomic 实时读取。
- 配置事件由 update_configs_diff_and_emit 生成真实 diff；current tools/closed/failed 发布只选择 server 和 payload，不允许外部重新标记 episode。无 channel/authority 或接收端关闭返回 false；update diff 不因发送失败回滚。
- 全量 config equality 是 Vec JSON 字节序列相等，顺序变化也触发更新。full update 清 owned/authority/tool meta/disabled registrations，保留 shared、disabled_tools、max_access、init_failed/meta_config_map/ACP registry；取消 init、checked generation+1 并同步 publish transports。
- diff 按 name→JSON 比较（重复 name 后者覆盖），同名变化同时 removed+added；HashMap 顺序不保证输出排序。仅删受影响 owned/authority 与 name__ 前缀 meta/stashed registration，保留未变化 owned 的原 episode；取消所有 init 进度并撤销 retained-but-not-owned authority。先 mark removed handshake complete 随后 cancel 清全 set；init_failed 并未在此清。串行化失败从 by_name 略去，不能保证错误变更仍完整 diff。
- meta parser 从 _meta.mcpConfig 反序列化整 map，任一 invalid value 可使整 map 回退空；camelCase 可选 startup/tool ms/per-tool ms/expose base64。默认 startup 30s、tool 6000s；具体优先级/毫秒取整实现待读。
- qualified name 要恰好一个 overlap-aware __（___ 算两处而拒绝）、两侧非空并满足 ToolId；registration 再对完整名称施加 64 ASCII regex。invalid skip/log。model_visible 默认真；若 ui.visibility 是数组，则只有含字符串 model 才 true（空数组及非字符串项均不授权），类型不是数组则回退 true。
- McpErasedTool metadata kind=Other namespace=MCP，运行时 id 用 qualified name（非法 fallback mcp_tool），description 使用未限定 raw name；成功注册前已做更严格校验。run 忽略 ToolCallContext，锁 state 查 client 后释放；本方法不显式复核 disabled/eligibility/max_access，须看 runtime 调用者的门禁。
- raw args 非object 时 params.arguments=None；ensure_initialized 在 tool timeout 包裹之外。单次 call 使用 per-tool 秒数 timeout。TransportClosed/TransportSend 可 recover，HTTP JSON-RPC 除四个 deterministic client codes 和 auth-message 外可 recover；只重试一次，恢复失败保留原 error，重试失败返回 retry error。超时不重试 side effect，HTTP 首超时 reset 供下次用；重试 timeout 不再 reset。错误路径直接 ToolError，is_timeout/reconnect 标志不会被封成 MCPOutput，且该 run 尾部成功/业务错误诊断事件不会执行。
- 业务 is_error 只聚合 Text，丢其余 content；正常 Text、Image、Resource 保留，image blob resource 转 data URI，其他 Resource JSON；audio/resource_link 等与 structuredContent 未在该分支输出。可选 raw base64 wrapper 与 data URI 同时生成，不验证 mime/base64 或做转义。auth_retry_attempted 固定 false；typed timeout 只认 McpError::Timeout，不含 ServiceError 内的 timeout。
- auth rejection 字符串 ASCII lowercase，auth required/authorizationrequired/authrequired/authentication/unauthorized 子串匹配，或五个 401 上下文且仅要求右侧非 ASCII alnum；不校验左边界。403 本身不匹配，但含 authentication 的 403 文本仍可命中，不能写成无条件排除所有 403。
- ResilientRwTransport send 串行 JSON+LF 并 flush，close take writer；receive read_until LF 后去 LF/CR，空行略过，合法 Rx JSON 返回，坏行继续。id 缺失且 method 为字符串的未知通知静默 trace；null id 不算缺失。record_decode_error 实际只有 tracing warn，没有注释所称 McpTransportDecodeError session 事件；sample .chars().take(200) 是字符上限而非 byte，上游 from_utf8_lossy 对整行处理且无输入长度 cap。
- receive 的 read_until 累积到本地新 Vec，取消安全性须结合 rmcp 调度，不从函数签名推导。大量坏行持续消耗至合法消息/EOF，不回 JSON-RPC 错误，避免回音；不证明恶意输入全局时间/内存有界。
- SafeTokioChildProcess spawn 配管道，best-effort group attach，失败降级 direct child；scope 已关闭时 register kill group，再 runtime child.kill 或无 runtime start_kill，返回关闭错误。drop 同步杀 group，再当前 runtime spawn kill/reap 或新 cleanup thread/runtime；thread/runtime 创建失败仅告警/信号，故注释 never zombie 不是无条件保证。
- graceful_shutdown 先 take child，再 close stdin，等待 exit 或 3s 后 kill group+child，最后清 group。该 async 方法取消时局部 child 已离开 self，是否仍完成 reap 需独立核对 tokio 与 owner；不能把正常路径当取消安全证明。child.wait 后才 kill group，注释 pid reuse 的强保证需 ProcessGroup 实际机制支撑。
- ClientState Empty/Pending/Initializing/Ready，Http/ACP 的 addressing 可 restorable，stdio child 不可 clone。McpClientEpisode 三私有字段与私有 new，公开 getters；authority 保存 transport revision Arc atomic。事件五种 client-origin 与 ConfigDiff/Added/Removed；50ms coalescing 仅注释指向外部 dispatcher，本文件尚未证实该行为。
- InitGuard restore=None 即 drop 无动作；HTTP/ACP 被取消时 try_lock 且仍 Initializing 才还原 Pending，并 notify。stdio restore=None 不走通知/恢复；锁冲突也不恢复，只 notify。后续 ensure_initialized wait-timeout 如何恢复待继续阅读。NEXT_CLIENT_ID 从 1 checked fetch_update，耗尽 panic。

停在 McpClient 字段定义 2770 行；本轮只审阅，尚未执行测试，不标整包完成。

## servers.rs 2771–4460：生产实现完成

- timeout 实现优先级为 meta server ms ceil/1000 > 外部 sec > startup30/tool6000；per-tool 外部 map 为底，meta per-tool ms 覆盖同 key，未命中才 fallback server timeout。0 允许为0。tool_timeout_for 注释默认60s过时，实际6000s。expose_image_base64 同样 meta > override > false。
- 构造统一 Pending，保存 Http/ACP reconnect 快照、独立 warn budget、client ID 与 transport revision0。is_http 判断 http_config Some，is_acp 看 reconnect 类型。reset 仅可 restorable，先 revision+1 再异步 replace Pending 并 notify；不会主动清 liveness handle。
- recover 先单独 state_kind 再 reset，两次锁之间存在并发窗口，注释“coalesce arbitrary concurrent recovery”不能只据此证明。非Ready直接 ensure；Ready stdio reset false返回不能恢复。ensure成功后 arm watcher；旧handle未清可能 false，并仅在有sender非ACP时warn。
- ensure 在看 state 前建立 Notified，锁内交换 Initializing；Ready立即返回即使transport已closed，Empty错误，Initializing等待 startup+1s后仅返回 ClientError，不改变卡住状态。Pending 调用者拿transport、锁外握手。InitGuard仅Http/ACP可还原；stdio取消无restore/notify。guard在结果发布前的state.lock().await之前disarm，若此等待处被取消无法恢复；仍待并发定向复现。
- 每握手 revision+1；result落Ready或Http/ACP Pending/stdio Empty并notify，再用握手前后捕获sink及固定revision发Ready/Failed。结果写入时不比较当前state/revision，外部reset竞态是否旧结果覆盖新状态须独立测试。事件admission可拒绝旧revision，但不能据此推导状态写入也被保护。
- try_handshake 三transport均由startup timeout包serve；HTTP transport构造在timeout外。ACP reverse budget=max(startup,server tool)，忽略per-tool override；外层调用仍用per-tool timeout，但大于server默认的工具可能被invoker默认backstop先截断，注释“never undercuts real outer bound”不普遍成立。
- ClientInfo name grow-shell-<server>，version::VERSION，显式protocol2025-06-18；extensions声明io.modelcontextprotocol/ui MIME text/html;profile=mcp-app。handler只override tool/resource list-changed和get_info；其他SDK默认行为非本模块实现，不从capability推导UI渲染。
- arm watcher拒绝ACP、无sink、非Ready或slot已有handle；锁住slot后spawn再存handle。公开set_liveness_handle直接替换drop旧handle，无generation配对；旧poller是否清新slot疑点仍需并发验证。poll_interval0没有本层检查，tokio interval前提由调用者满足。
- HTTP headers逐项parse，非法仅warn key并skip，重复HeaderMap insert后者覆盖。Figma名字或host匹配则缺UA时补grow-cli，不覆盖调用方UA。reqwest默认headers/client builder，没有显式request timeout/OAuth/credential refresh；Http/Sse config都同用streamable HTTP transport。
- is_healthy/state_kind/liveness_check只锁state读取，无网络探活，不证明远端HTTP健康。server_instructions仅Ready取非trim空白文本，返回原字符串不trim，不触发init。
- materialize_descriptors先ensure，再分页list_tools收集全部bytes；无本层分页数/重复cursor/额外deadline。原name/description/inputSchema直接落JSON，不施加registration过滤或schema补全，资源不落地。spawn_blocking create_dir_all，逐文件NamedTempFile写+persist原子替换，统计成功写次数；sanitize碰撞可能覆盖同名文件，旧descriptor不删除，没做整目录事务/fsync。tool文件附.json，使原 . / .. 不再是目录穿越；server_dir为外部传入，server名称目录的containment须调用方核对。
- get_tool_registrations也分页至None后全量建registration；缺description变空，schema仅在缺字段时补type=object/properties={}，已有非法值不改。名称不合法过滤，meta保留、model_visible按前述规则；timeout map未知key只info。自身不过滤disabled和access ceiling，交给调用方；总startup/tool list超时不是这段保证。
- public call_tool（调试入口）ensure后直接call，非object args=None；没有McpErasedTool的timeout/recovery/output转换。不能把运行时包装器的保证套到这个公开入口。
- stderr日志位于grow_home/logs/mcp/<sanitized96chars>.stderr.log，每spawn截断；目录和OpenOptions同步I/O，之后tokio copy。无字节上限/轮转，同名或sanitize冲突的进程可共用文件；打开失败返回并drop stderr，不持续drain。文件名附后缀故 . / .. 不形成直接路径跳出。
- header session substitution支持{{session_id}}与${session_id}两次replace；无session时任何含placeholder的整项丢弃，不含的保留。先前替入的session值若含第二种token可再次展开，尚未真实配置复现，不承诺single-pass。
- start_stdio先command.to_string_lossy，所以helper返回OsString虽免再次lossy，原非UTF8命令已丢失。Windows裸名字按env第一个大小写不敏感PATH/当前cwd用which_in，含/或反斜线不解析，nonWindows不解析；命令继承当前cwd/env并应用显式env，没有env_clear。kill_on_drop(true)、detach_command、scope enrollment后立即返回pending client，不在此握手。
- start_http/sse仅解析headers占位并创建pending；未知variant明确错误。start_mcp_servers buffer_unordered8，返回完成顺序非输入顺序，spawn同步工作仍可能占执行器。name/transport/target helpers未知返回unknown/empty；target把args以空格连接只供展示，不是shell安全编码。
- stub为公开doc(hidden)Empty，不在cfg(test)，startup10/tool60、无reconnect。GrowClientHandler每通知读取最新sink但固定自身transport revision，best-effort发事件。生产代码截至约4350已全读；测试已读至4460，包含resilient坏行继续、Windows解析helper与PATH、Figma识别，尚未执行测试，剩余至7660待逐项审阅。

本轮未Cargo构建。另一任务报告main归档fix-skill-substitution-single-pass及50项工具技能测试，这不属于当前工作树证据，未合并或据此改变本包事实。

## servers.rs 4461–7660：全文件及测试审阅完成

- 已补读末次输出截断的7140–7260。全包7个Rust文件（6 src +1 integration）和manifest阅读完成，待生成契约映射及动态验证后更新inventory。
- Figma三项测试验证缺UA补grow-cli、显式UA保留、非Figma不补，未用真实HTTP请求检查发送。Windows launcher helper在本机传is_windows=true，只能验证计划结果，不证明Windows CreateProcess运行。
- Unix真实sleep测试在drop runtime后drop transport，5秒轮询kill0确认leader不存在；scope.kill_all测试先确认live_count1，取child后清group，wait并断言SIGKILL，避免普通drop掩盖缺登记。这些不是grandchild/取消中途/cleanup thread创建失败覆盖。
- ACP registry保持与overrides测试只构造pending，无handshake；ACP liveness gate测试同时无sender且非Ready，因此单独false断言不能辨别是否命中特定ACP gate。is_acp/is_http类型断言有效。
- init_failed fresh attempt测试在NotStarted调用mark_servers_initializing，确认failure被清但没检查该方法后续因NotStarted忽略handshake集合；不能把“清错误”视为成功开始init。
- config tests明确order-sensitive、generation增加、state reset、diff新增/删除/替换；generation/revision耗尽should_panic，不能证明panic前其它已改字段回滚。retained identity与同名replacement revoke通过stub authority真实辅助函数验证；typed emit测试直接private helper，不是外部dispatcher整链。
- shared pool测试验证Arc身份、owned优先、configs碰撞过滤、map快照独立、live工具撤权、后增server受scope限制、同名client replacement及后代在中间parent rebind前拒绝。都使用stub，证明eligibility数据结构而非实际Tool::run必经这些检查。
- name tests区分ToolId结构与provider regex：123__lookup和server:scope__tool结构允许但registration拒绝，64/65字节边界明确。___ overlap拒绝；同raw name不同server真实LocalRegistry各自保留。
- error classifiers覆盖TransportClosed/Send、四种deterministic JSON-RPC、HTTP recoverable、auth message抑制、非HTTP不重试业务RPC。transport predicate即使reconnect_attempted=true仍true，整体至多一次由recover_and_retry不递归保证。
- 真实本地fake HTTP测试覆盖-32603重建成功（calls2/inits2）、两次失败返回第二错误、invalid params不重试（calls1/inits1）、首次outer timeout转Pending且仅calls1，下一次重新init成功，以及重试timeout（calls2/inits2）。直接try_call_tool测试，不经过Tool::run输出/诊断转换。
- recover失败用192.0.2.1:1 startup1秒，验证原错误且reconnect=true/timeout=false。HTTP reset幂等测试只断言true，不证明revision不变化（实际变化）；reset后两次失败不证明端口一定拒绝，只接受Timeout/HandshakeFailed。
- ACP恢复测试手工安装真实dead RunningService，再调用真实try_call_tool；echo结果证明重新握手后第二条工具请求成功。但dead_service未显式等is_transport_closed，关闭观察依赖调度；不把它当任意并发事件可靠性证明。
- auth tests覆盖401词边界、普通403/forbidden、incidental digits与typed Spawn/Timeout不归auth；没有403混auth word和左边界测试。image format tests仅字符串/前缀数量，不调用session extractor、不验证实际图像或无效base64。
- ensure并发5caller不可达HTTP测试只要求每个返回握手错误，未计数并发握手数量。TEST-NET地址不保证本机proxy/route一定挂起，不能凭注释认定确定性。parked notify测试手工写state+sleep50ms；silent timeout测试手工Initializing检查错误文字，未验证state恢复；abort测试轮询Initializing后abort，检查Pending但不再执行成功握手，未覆盖stdio/锁竞争/guard disarm后取消。
- liveness Ready+closed集成覆盖的前文注释没有在本文件找到对应实际watcher测试。此处注释反而明确Ready健康分支交由上游测试；当前仅Empty/Pending/Initializing与无网络<=1秒。不能宣称本包测试已验证Ready+closed单次发事件和清slot。
- handler测试直接emit而非真实notifications入站；old/new revision检查手工advance并验证admission，后置sender可见性有效，但不模拟真实recovery。get_info只比较name/version，没有protocol/extensions断言。最后ConfigAdded只核server_name，未验证注释中的Ready→initialized映射。
- 本包没有materialize_descriptors文件系统测试、model visibility矩阵、session header substitution、整体Tool::run内容转换、分页循环或真实stdio handshake覆盖。阅读完成不等于这些分支已动态通过。

## 最终登记与验证

7 个 Rust 文件和 Cargo.toml 完成逐项审阅，43 项契约映射到 mcp-integration。此前“进行中/未运行”是历史阅读阶段状态；现在整包已完成当前功能提取，运行时疑点另列backlog，不把未实现保证写入当前契约。

本工作树执行 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p mcp --target-dir /Users/lordcasser/workspace/projects/grow-openspec-sdd/target -- --test-threads=1，退出0：155单测、3个真实本地fake SSE集成、1个compile_fail doctest通过，0失败/忽略。日志 /tmp/grow-mcp-inventory-tests.log。仅本机平台，Windows launcher为helper模拟，不代表Windows运行验证；现有测试覆盖限制仍如前述。

测试完成立即 cargo clean --profile dev --target-dir 本任务target，删除5041文件约1.7GiB，未动其它工作树。严格规范校验和来源核对另外记入verification。
