# acp-transport 逐包核查

包路径：`crates/codegen/acp-transport`。全部 Rust 模块、Cargo.toml 和可用 build.rs 已阅读；测试作为证据阅读；cargo test --locked -p acp-transport在本独立工作树通过16项测试，0失败、0忽略、0 doctest，日志/tmp/grow-acp-transport-tests.log。

## 模块与开关

- `crates/codegen/acp-transport/Cargo.toml`
- `crates/codegen/acp-transport/src/channel.rs`
- `crates/codegen/acp-transport/src/common.rs`
- `crates/codegen/acp-transport/src/connection.rs`
- `crates/codegen/acp-transport/src/gateway.rs`
- `crates/codegen/acp-transport/src/handler.rs`
- `crates/codegen/acp-transport/src/lib.rs`
- `crates/codegen/acp-transport/src/line_reader.rs`
- `crates/codegen/acp-transport/src/message.rs`
- `crates/codegen/acp-transport/src/protocol.rs`
- `crates/codegen/acp-transport/src/stdin_reader.rs`

Cargo feature：`{}`。

## 功能与规范映射

- [ACP v1 local handler boundary](../specs/acp-transport/spec.md#requirement-acp-v1-local-handler-boundary)：ACP适配层 SHALL 重导出SDK v1 schema及连接原语，以Grow的AgentSide/ClientSide和?Send handler将本地会话实现与SDK连接隔离。
- [ACP typed bidirectional messages](../specs/acp-transport/spec.md#requirement-acp-typed-bidirectional-messages)：ACP内部消息 SHALL 为每侧定义11个变体，将request与对应response oneshot绑定，支持boxed/unboxed存储和method_name查询。
- [ACP internal message serialization](../specs/acp-transport/spec.md#requirement-acp-internal-message-serialization)：Agent消息序列化 SHALL 只输出method_name与request，扩展消息使用内部ext_method/ext_notification标签；反序列化按已知方法选择类型。
- [ACP channel round trip failures](../specs/acp-transport/spec.md#requirement-acp-channel-round-trip-failures)：ACP双向通道 SHALL 使用unbounded mpsc；acp_send先enqueue再等待typed oneshot，无内置deadline或重试。
- [ACP gateway forwarding modes](../specs/acp-transport/spec.md#requirement-acp-gateway-forwarding-modes)：Gateway sender SHALL 支持等待响应、返回completion receiver和只报告enqueue成功的fire-and-forget三种入口，clone共享队列。
- [ACP gateway dispatch and trace context](../specs/acp-transport/spec.md#requirement-acp-gateway-dispatch-and-trace-context)：Gateway receiver SHALL 按队列接收后为各消息独立spawn任务，默认spawn_local，可覆盖spawner；结束接收循环不等待已spawn任务。
- [ACP SDK byte stream connection lifecycle](../specs/acp-transport/spec.md#requirement-acp-sdk-byte-stream-connection-lifecycle)：connect_agent_v1和connect_client_v1 SHALL 接受Send字节流，以unbounded队列桥接SDK回调和本地handler，返回sender与需驱动的连接future。
- [ACP request cancellation dispatch](../specs/acp-transport/spec.md#requirement-acp-request-cancellation-dispatch)：入站request SHALL 由SDK RequestCancellation::run_until_cancelled包裹handler；Agent prompt取消后另调用session cancel，忽略cancel返回错误。
- [ACP extension wire names and peer requests](../specs/acp-transport/spec.md#requirement-acp-extension-wire-names-and-peer-requests)：两侧Peer SHALL 对普通请求调用SDK send_request并等待block_task，对通知调用send_notification；扩展请求和通知的wire method无条件前置下划线。
- [ACP complete line buffering](../specs/acp-transport/spec.md#requirement-acp-complete-line-buffering)：LineBufferedRead SHALL 在后台读取完整换行分隔byte行，经容量参数64的channel供AsyncRead消费；缓冲行内连续返回Ready，Pending发生在无行可用时。
- [ACP dedicated stdin line reader](../specs/acp-transport/spec.md#requirement-acp-dedicated-stdin-line-reader)：spawn_stdin_line_reader SHALL 启动名为acp-stdin的OS线程，以blocking read_until读取原始byte行并经容量64通道blocking_send，保留末尾无换行行。

## 边界

- 必须实现initialize/authenticate/new_session/prompt/cancel；load、mode、config option、list默认method_not_found。
- 必须实现request_permission和session_notification；文件读写与五种terminal方法默认method_not_found；两侧ext method默认JSON null，ext notification默认成功，Rc/Arc委托全部方法。
- 包含initialize/authenticate/session new/load/list/set mode/set config option/prompt/cancel及ext method/notification。
- 包含permission、read/write text、session update、terminal create/output/release/wait/kill及ext method/notification；route_to方法按变体spawn本地handler并尽力送回结果。
- 返回错误，不自动归为扩展。
- 反序列化创建oneshot但立即丢弃receiver，不提供可等待响应；boxed转换保留原response sender。
- 返回InternalError，data.growAcpChannelFailure=send_failed。
- 返回InternalError且tag为recv_failed；classifier对未知或无tag返回None，不解析错误文本。
- fire-and-forget后返回Ok，即使队列拒绝；需要判断接受与否的调用方使用bool入口。
- 等待本地forward响应，与AgentSide通知入口不同；completion只代表所接handler完成，不代表远端已确认业务处理。
- 构造span并instrument任务，缺meta使用空span，ExtRequest/ExtNotification不调用on_meta。
- 以compact JSON记录完整request和成功response，无ANSI且无字段脱敏；序列化失败返回空字符串。独立任务不保证任意异步handler完成顺序，现有ordering测试handler在首个await前记录。
- request保存responder与cancellation并独立spawn，handler结果转JSON后响应；notification错误被忽略，未知request返回method_not_found，未知notification忽略。
- 连接回调返回成功，不在本层join已spawn本地handler；LineBufferedRead由调用方选择包装，connect本身不自动加行缓存。
- 现有字节流测试观察prompt future被drop、cancel一次及-32800 Request cancelled响应。
- 取消其handler future，不额外构造session cancel；此层不保证handler派生后台工作被取消。
- wire为_grow/coordination/list，现有测试验证入站handler收到无前缀逻辑名称；扩展响应由JSON反序列化，失败为InternalError。
- 现有SDK适配测试只对list生成batch response，cancel送至handler；碎片化initialize按V1返回。
- 保留最终部分行并跨多次read交付，结束返回0，不做UTF8验证。
- 在扩展当前fill buffer并consume后返回InvalidData且停止生产；限制不是全队列总内存上限，内置同名oversize测试实际只测普通行。
- 退出线程并关闭发送端；接收端drop不能主动中断正在阻塞的stdin read，无join/cancel handle，单行无大小上限。
- 线程使用私有副本并尝试将进程stdin置NUL；duplicate失败回退全局stdin，NUL失败仍使用副本，SetStdHandle返回值未检查，不保证隔离总成功。

本包已枚举入口的功能均已映射至上述 delta；调用方的更大流程仍在其所属 crate 核查，不由本包推导额外保证。

## 阅读与验证范围

全部10个Rust文件与manifest已阅读。Cargo无本地feature，主要依赖SDK、Tokio、futures、serde/serde_json、async-trait和tracing；tokio-util compat用于测试。16项测试由channel关闭2项、common分类2项、line reader 7项、connection wire 3项、gateway completion 2项组成。

- connection测试使用内存DuplexStream、LocalSet和SDK字节协议，验证3字节碎片initialize、batch含cancel/list、未知通知、扩展前缀与在线prompt取消。没有真实IDE、网络断线或Windows标准输入测试。
- line_reader的read_line_capped_rejects_oversized名称与实现不符：只输入normal line并检查EOF，没有实际构造64MiB超限。64MiB行为来自源码，不计作动态验收。行上限是扩展fill buffer后检查，队列容量不等于64MiB总内存上限。
- stdin forward_lines没有内置测试；read_until每行无上限，读取错误直接退出并丢弃本轮部分line，receiver drop只在下一次send时被观察。spawn失败panic，没有线程join或主动取消入口。Windows NUL句柄被保留为进程标准输入，SetStdHandle失败返回未检查；不能声称所有stray read必定隔离。
- gateway completion测试的handler在首个await前同步记log，因此证明此测试调度下的排序及全部completion先于response标记，不能证明会挂起的handler仍保持执行顺序。sender入队、handler完成和wire通知真正业务处理是三个不同观察点。
- connect的入站pump及各handler通过外部spawner启动，outgoing gateway默认用spawn_local；仅传入custom spawner不改变gateway默认执行器要求。连接future结束没有集中取消或join这些任务。
- Agent消息的JSON是内部method_name/request封装，不是wire JSON-RPC。Client消息没有对应serde实现。Debug仅委托request，不输出oneshot。扩展wire函数无条件加一个下划线，调用方应传逻辑名。
- compact_json失败返回空串；启用gateway tracing时没有敏感字段过滤。默认关闭tracing，不将此行为解读为日志脱敏保证。
