# Design

## 依据与所有者

事实与反例入口见[有界调查](../2026-11-07-complete-nondeferred-backlog/sdk-reverse-preparation.md)。父亲核了真实producer `session/acp_mcp.rs`（`SdkCallParams`只有serverId/message）、`session/actor/spawn.rs`（整批SDK声明共用一个gateway invoker）、leader有效ingress与`extract_target_client_id`优先单播、目标断开不fallback。现sharedpool会捕获SDK进入owned_clients后的Arc，因此child可以按原ceiling继承同一invoker；不能把child传空SDK声明误作“不继承SDK”。

模块职责分为两项：leader拥有accepted connection的权威ClientId以及raw new/load入口覆盖；session SDK owner拥有本批注册的session/connection绑定与反向params。leader已有target单播就是消费方，不新增路由表、serverId索引或driver例外。MCP库继续只处理transport和资格，不负责猜ACP所属连接。

## 冻结的接口与状态归属

### leader ingress

复用`params._meta["grow/leaderClientId"]`，new/load无条件写实际`ClientId.0`，覆盖caller值（含错误类型），替换现仅在缺失时补值的行为。`_meta`缺失或null时为有效可选元数据，应创建object后正常注入；非object非null由leader在改变session subscriber/driver/backfill/pending-request状态之前向原连接返回JSON-RPC `-32602`（有id时保原id；无id丢弃），不转发至agent，也不能悄悄规范化成功。其它capabilities/default-model规则保留。server实际接受的连接才是权威，不从clientIdentifier、SDK serverId、session driver或模型输入推断。

实施核对推翻了原“交ACP自然拒绝”的前提：锁定`agent-client-protocol-schema 1.5.0`的v1 NewSessionRequest.meta（1032）和LoadSessionRequest.meta（1195）使用`DefaultOnError`，会把畸形metadata吞成None。父亲读锁定源码，不能再以此当下游保证。该入口必须兑现有可信owner的请求才可进入session操作；拒绝是本次leader authority的必要边界，不修改vendor或通用ACP解析策略。判别测试需带齐合法必需字段，尤其`mcpServers: []`；缺该字段的typed失败不能证明meta被拒。原inject helper保畸形JSON不物化即可，实际早拒绝由leader ingress承担。

此字段本来也服务load replay target；修正同一信任入口时，replay会回到实际loading连接，不能保caller伪造target的旧测试预期。`extract_target_client_id`及既有单播/断线丢弃路径不需新算法。不要借此变更非SDK普通reverse的driver政策。

### Shell session SDK registration

`GatewayAcpInvoker`绑定三个值：现gateway、注册时的`acp::SessionId`、可选leader `u64` owner。构造契约：

```rust
pub fn new(
    gateway: AcpAgentGatewaySender,
    session_id: acp::SessionId,
    leader_client_id: Option<u64>,
) -> Self
```

`SdkCallParams`保serverId/message，新增`sessionId`；owner存在时新增`_meta: { "grow/leaderClientId": owner }`，不存在则省略这个meta。使用typed serde字段，不将MCP JSON-RPC message内的id/参数当host路由字段。`invoke`仍通过同一gateway一次send、同一timeout/错误处理，不新增callback或重试。

注册值来源是本次new/cold-load的session meta。主bootstrap解析该owner并沿现两个spawn函数传入；具体增加紧邻`acp_mcp_servers`的`acp_mcp_leader_client_id: Option<u64>`参数即可，现只有primary、child和spawn转发这几个生产调用方。child新注册Vec为空且传None；已继承的client自带原invoker不修改。actor在原`set_acp_servers`处以真实`session_info.id`构造invoker。不要从initialize级全局meta取owner，后续另一连接initialize会污染归属。

每个现SDK registry整批来自同一次请求，并共用同一invoker；把owner绑到invoker足够。不给`mcp::AcpServerEntry`新增owner字段，不把它序列化/持久化，也不预建多owner条目。若未来真实入口接纳不同来源的混合批次，再独立设计；当前无此要求。

## 生命周期决定

- new和cold load：仅对本次有效声明创建新绑定；cold不能从旧文件恢复进程ClientId。旧SDK client不跨进程复用。
- resident load：现入口不更新SDK registry，保原owner/invoker。B成为driver或带新的有效SDK metadata加载现resident actor，也不夺取A注册。重复声明F3的早期拒绝仍保持；本次不增加SDK热更新端点。
- inherited client：child保已授权shared Arc及其原session/owner，不改为child当前driver；原delegated ceiling/eligibility独立约束可调用资格。
- owner断开：现target路径不投B，gateway/原timeout产生失败；不宣称远端已执行，也不为避免timeout改绑另一客户端。
- standalone ACP：没有leaderowner则用本身gateway，新增sessionId只表示注册上下文，不建立另一条连接；现bounded round trip保持。

## Coder Task Specs

1. **leader提供真实注册连接身份**。只改现new/load metadata注入、畸形metadata早拒绝及对应tests，交付forged owner被覆盖、absent/null元数据正常注入、畸形元数据在session路由状态变更前向原连接返回invalid_params且不转发，以及既有replay/target/driver回归。该增量不负责SDKproducer。利用真实leader ingress检查actual forward payload，不只断言私有helper；旧保caller值测试应按新契约更新。锁定schema的DefaultOnError是早拒绝必要性的依据，typed控制完整且区分解析失败来源。
2. **session SDK invoker保注册归属**。在增量1满足可信字段后改invoker/params、primary/spawn参数及child机械适配，保原MCP库接口和SDKduplicate校验。真实GatewayAcpInvoker的输出接leaderfixture，验证A注册/Bdriver、A/B同serverId、A断开无Bfallback；standalone同producer收到正确response。模块事实需要模拟输入时明确范围，不把手写_meta包当producer覆盖。

两个任务按依赖交付，源码/Cargo窗口串行。父核信任入口和真实producer/consumer决定性代码；不整支合并旧`codex/mcp-policy-sdk-identity`，不混入SDK目录合并、rmcp升级、F1 call gate或name bounds。

## Review Plan

增量1独立验收实际ingress覆盖与已有路由回归；增量2独立验收绑定字段、构造来源/生命周期、单次gateway行为及producer到leader边界。集成里程碑要求实际leader接受A/B并分配ClientId，经new/load声明所得值创建invoker，真实initialize/list/call消息只到原owner；A断线Bdriver仍不能收该SDK请求，generic reverse继续按driver。保request id/response相关与timeout真实退出，不靠短sleep算“未收到”。

父亲核leader的driver前提：session请求入口与new响应都用entry.or_insert，后来的load是join-not-steal，不会自动把driver从A改为B。路由层可先建立B为已知session的driver，再用A实际new的forward meta取得registration owner；该fixture仅构造target与driver不同的真实leader路由状态，不冒称真实Mvp会按同一顺序创建session。必须用generic reverse probe实际投B确认前提。真实Mvp生命周期另外按A new→B resident load验证原SDK注册保留，并保持实际driver政策。两个层次的结果分别报告，禁止为测试方便改driver生产语义。

真实producer路由证据必须以实际客户端经socket回复、leader转回、invoker取得同一outer请求的response闭环；testbridge只做忠实线上格式/请求id分发，不得立即生成canned成功或用人为hold制造owner断线timeout。断线情况下由原invoker等待实际无回复走到原deadline，B的causal barrier证明没有fallback。

resident/cold入口用真实MvpAgent或现可控spawn fixture核注册处理；不能用两个随意构造invoker冒称冷恢复。继承可用SharedMcpPool/当前SDKclient的真实共享及producer验证原owner，无须为了此场景引入新注册API。若现fixture不足，先报告可定位缺口，不用static或synthetic证据替代已声明的集成范围。测试仅隔离HOME/project和本地loopback/duplex，进程归本fixture回收；不声称Windows/外部SDK部署现场。

父已核可复用完整测试入口：`leader/in_process.rs::spawn_agent(acp_rx,response_tx)`确实创建MvpAgent＋真实ACP connection/gateway；`tests/test_leader_soak.rs`示范leader setup、隔离配置与initialize/auth，不需要为SDK功能启动soak循环。SDK fixture的客户端接收循环须应答实际`_grow/mcp/sdk_call`，不能沿soak原rpc helper直接跳过所有非response包，否则会把真实reverse挂住。所有phase共用隔离GROW_HOME，避免进程OnceLock切换沙箱失效。实际cold load可参考`mvp_agent/tests.rs`的`effort.pre_reload_close`/`effort.post_close_load`链：同一session先真实close并等待writer lease释放，再调用load；跨进程重绑如另行验证须保各自连接id证据，不把更换一个构造参数称为重启恢复。

生命周期测试不需为访问状态添加public hook：现`SessionHandle::call_mcp_tool`（handle.rs575）发原CallMcpTool命令，run_loop原session activity异步调用真实McpState；ACP `grow/mcp/call`也走这条链。`snapshot_initialized_mcp_pool`可等待现初始化并供继承Arc控制。SessionHandle本身没有公开mcp_state字段，不把假定直接访问当必要接口。new/cold的owner来源仍是agent_ops中同一session_meta；父核acp_agent.rs1259 cold明确传request_meta，resident1284只UpdateMcpServers，不能为测试方便新建SDK热更新或重绑端点。producer→leader路由层可以复用实际accepted连接和forwardmeta，不要求该层fixture每次都同时启动MvpAgent；真实Mvp生命周期场景另有自己的入口证据，两者在最终里程碑汇合。

父接受各任务后同步开发说明和backlog，strict归档；本设计不表示已实施或产品验证完成。
