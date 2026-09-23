# sampler 逐包核查（进行中）

已读取 Cargo.toml，列出 src 下 23 个 Rust 文件；尚未读取实现，不标记 reviewed。

包描述是 provider-neutral HTTP streaming/retry 层，直接依赖 sampling-types/version 和 async-openai/reqwest/eventsource-stream/tokio 等。manifest 无自身 Cargo feature。测试使用 axum/test-support，ctor+diagnostics 安装 TLS provider，tokio test-util/net/multi-thread 支持超时和本地 mock；这些依赖配置不能证明运行时全部请求或重试路径正确。下一步从 lib/config/client/shared_http 开始，继而 actor、三个 stream 消费者与重试/doom-loop 边界逐文件审阅。


## lib/config/shared_http 完整读取；client.rs 1–250

- lib 暴露三层：原始 HTTP stream、SamplingEvent 转换、actor handle 并发/取消/重试。此处仅入口，后续逐实现验证，不以模块描述证明运行时行为。
- SamplerConfig 无统一 serde default；默认构造空 URL/model、context_window=0，AuthScheme 默认 Bearer，另有 XApiKey。backend 与 auth 独立。extra_headers 不由本层按 URL 推导。query/env header 有 serde default；配置可序列化 api_key，Debug 也派生，不能声称该容器不含敏感值。
- attribution_callback/bearer_resolver serde skip，往返丢失，调用方必须重新挂接；BearerResolver 是同步 current_bearer。context_window 仅信息，本结构不强制上下文容量。RetryPolicy 默认引用 retry 常量，retry_only_before_output 默认 false。config 两项测试已读未执行。
- shared_http 两个进程 OnceLock 缓存客户端；共享开关 GROW_SAMPLER_SHARED_CLIENT 仅精确 0 或大小写无关 false 禁用，不 trim，首次读取锁定。禁用时每次 build，绕过 cell。失败不缓存，并发构建失败/竞争不保证只构建一次，胜者缓存。
- 默认客户端 pool 每 host 2、idle 90s、connect 10s，可由对应环境整数覆盖，解析失败默认，不限制零或极值；tcp_nodelay，HTTP2 ping 15s/timeout5s/idle ping。无 total request timeout 配置。HTTP1 fallback pool=0、idle=0、http1_only，共享对象但不复用池连接。禁用共享会每次重读构建环境 knob，不能照注释泛化为都只读一次。
- shared_http 两个单测覆盖构建失败不缓存及 disabled 不写 cell，ctor 安装 TLS provider。注释引用 shared_http_wire/kill_switch 集成二进制，实际文件归属仍待核对，不能视为本轮运行证据。
- client 开头：Responses/Messages generic tagged decode 只容忍未知非空顶层 type；用仅 type 的 serde probe 区分顶层 unknown_variant 与已知类型字段损坏，后者仍错误。未知事件不进入 L2，不能刷新 L2 idle timer（最终流循环待读）。
- Chat decode 要求 object=chat.completion.chunk；失败时仅没有 choices/error 键、type 非 error、object 非标准chunk且 type.or(object) 非空时当扩展忽略。空 type 存在会遮住有效 object，已知坏 payload 不允许伪装扩展。
- terminal Responses override 仅 completed/incomplete：正 i64 cost 存 grow.cost_usd_ticks metadata；context_details input/output 都可转 u32 时饱和相加覆盖 total_tokens，不改累计 input/output/cache/reasoning。缺任一或越界不覆盖。该函数先处理 cost 后检查 typed usage。
- request failure span 记录 success=false/error；Retry-After 仅解析 u64 秒、最多120，不支持 HTTP-date；x-should-retry 仅大小写无关 true/false、不 trim。client.rs 从251继续，extract_model_metadata 未读完。

读取哈希：
- `crates/codegen/sampler/Cargo.toml` SHA256 `641e4abc28e2632600d094a8eb13d2d2ab02ca73223b7ef8d40f8b3dee081a5b`
- `crates/codegen/sampler/src/lib.rs` SHA256 `418daac2812f5e67d94cd635cf39ff663cc0476746c926cf3f313eb1c54194f6`
- `crates/codegen/sampler/src/config.rs` SHA256 `7b008072b3ff14fa203a45c61990d56babb2c1307054af23f75a30f7f77e3591`
- `crates/codegen/sampler/src/shared_http.rs` SHA256 `b45441558e80cee9ba09fd27a80175a3e3d4a4bed15ef91d3f493216382a2828`

## client.rs 251–820 连续读取

- metadata 三头 context_window(u64)/output_limit(u32)/models_etag 任一解析成功则 Some，数值零不拦截，ETag 空字符串也可保留。
- StreamingChatRequest flatten inner 并加 stream/include_usage，实际派发待后续。env_http_headers 在构建时取环境，trim value、跳过 unset/blank/非法 header，warn 只记录 header/env 名；后应用覆盖已有头。
- EndpointTemplate 先 trim 尾斜杠，无 configured query 且原串无 ? 就 Plain 拼接，不解析 URL；否则解析 base，失败 warning 含原 URL 并回 Plain，配置 query 丢失。解析成功保留未覆盖 base query 对（原重复 key 可保留），配置 key 覆盖所有原同 key，query_pairs percent encode。prefix 去 query 但没去 fragment，因此带 fragment URL 的字符串追加不能当作可靠路径拼接。
- UA grow-shell/version 加平台，origin 同 product/version 时不重复；origin version 缺失仅产品名，arm64 归 aarch64。未清洗 origin 内容，最终 HeaderValue 失败时不替换已有 UA。
- body size 只检查 reqwest::Body::as_bytes 可见长度，超过50MiB 本地 API413 + should_retry=false；流 body/as_bytes None 按0，不能推广为任意流 body 都限制。
- new header 顺序 Content-Type→config api_key 按 auth_scheme→extra_headers→环境头→可构造UA。非法静态 api_key 返回 AuthUnknown，debug 含完整 key；非法 extra header 返回 InvalidConfiguration；环境非法则跳过。配置有 key 即先校验，不能因稍后 resolver 覆盖就忽略坏静态值。
- default 保存 model/output/temperature/top_p/backend/auth/stream_tool_calls/doom policy，不保存 config.reasoning_effort 作为 ClientDefaults（该值仅日志出现）。idle timeout 默认300秒，显式0保留。force_http1 选择共享的无池客户端。
- check_and_record_request 先 size 再等当前 AttemptEvidence.request barrier；audit route 去用户名密码/query/fragment，但 body 原字节提交 audit，记录失败变 Persistence；成功 mark_dispatched，是否六路径都正确调用待后续阅读。
- post 有 resolver 时先删除 Authorization/x-api-key，只以当前值重建指定scheme；None 或非法 fresh 值导致无凭据发送，不回退静态头。日志 info 输出 Authorization 前20字符与 x-api-key 前12字符，不是单纯布尔诊断。
- sent_fragment 根据配置 scheme 读取，Bearer 仅精确 Bearer 前缀接受；因此任意 extra Authorization 非该格式可能实际有头而片段None，不把注释“无凭据头”泛化。Auth401 用 post 捕获片段确定 Sent/Missing；callback 收捕获片段。current_sent_bearer_prefix 为诊断另读 resolver，不校验其能否构成头，可能与后续 wire 不同。
- Debug(SamplingClient) 包含 base_url/defaults 和 hook存在性，不含 header；base_url 可本身含 query。auth_info 从821继续，尚未读完。

## client.rs 821–1580 连续读取

- auth_info 为诊断，按 scheme 与当前片段选 bearer/x-api-key/none。debug 请求头按名称含 authorization/api-key/apikey/token/secret 脱敏，不覆盖任意敏感自定义名称；该机制不消除先前 client_post 前缀日志。错误 body_preview 是 lossy UTF8 前500字符；成功 HTTP 却 JSON 解码失败会 error 记录完整 raw_body。
- Chat 默认仅补 model/max_tokens/temperature/top_p 的 None，显式空模型不补；不会补 config reasoning_effort。非流式 build→size/audit barrier→execute→status/响应字节→解析；body 读取失败可先于401归因返回。非401错误带 metadata/retry hints，401变Auth。
- Chat stream 强制 stream=true/include_usage=true、Accept SSE，发送前同样执行 barrier。状态码一到即记录 success=HTTP success，不代表完整流成功。401先归因再读body，与非流式顺序不同。全部 body 原bytes先 audit，再只对第一个bytes chunk剥完整UTF8 BOM；跨chunk BOM或首空chunk不由此修复。
- Chat SSE 精确 [DONE] 结束；每条其他data info记录原内容。try_parse_stream_error 优先，再decode_chat_chunk；未知扩展 filter掉。仅 eventsource/transport error 设置终止flag，先emit一个EventStreamError，后续scan结束；语义/serde error本层并不设置flag，消费者如何停止待stream模块。audit.response错误在byte stream转字符串，也走EventStreamError路径，不能说始终Persistence。
- Responses defaults 补缺模型/温度/top_p/output，store仅None改false，显式true保留；include无论原先有无都确保加ReasoningEncryptedContent。非流式serialize Value→patch reasoning type→json→barrier→execute；成功直接typed Response，不运行terminal事件context/cost override。
- Responses stream 强制stream，stream_tool_calls仅配置true时写true；每次创建新doom collector，存在则强制opt-in头true（config None不主动删除extra_headers同名头）。所有发送先barrier；401先归因再读body。byte audit/BOM/scan与Chat相同。
- Responses SSE [DONE]结束，非DONE记录原data；先collector.absorb或禁用时is_check_event，check即吞，再stream error，再typed decode。关闭策略仍吞服务端check事件，不能说禁用后完全无解析保护。语义错误不置transport终止标志，collector接收的terminal信号逻辑待doom_loop模块。
- Messages defaults 对空model补值，max_tokens=0时用output_limit或128000；output_limit=Some(0)仍得到0，非统一合法性检查。只补缺temperature/top_p。非流式发送同barrier顺序、状态/metadata/hints，读body后401归因。create_message剩余及流式路径从1581继续。

本轮只静态审阅，无构建产物新增；未运行 sampler 测试，pending 不变。

## client.rs 1581–2530 连续读取

- Messages 非流式成功反序列化失败记录完整 raw_body。stream 强制 stream=true/Accept SSE；build→barrier→execute 与前两协议一致，因此六个发送入口均已核对有 size/audit 前置检查。401先callback再读body，其他错误带响应头提示。
- Messages byte audit 在 BOM 剥离前；SSE 精确DONE结束，stream error优先，generic tagged decode忽略未知顶层type；transport/eventsource错误仅发一次，语义错误不在此终止。此路径没有底层独立idle timer。
- 六个 conversation_* 便捷方法先补缺 model/temperature/top_p/max_output，再调用各明确协议入口；不因 config backend 不符拒绝显式指定协议的方法。conversation_collect 才按 backend 分派，生成随机RequestId，将client idle_timeout传给L2 stream，collect后丢metrics并把SamplingErrorInfo转回SamplingError；此入口没有actor重试循环。
- 已读测试：body恰好50MiB允许/+1拒绝且veto，idle配置字段，stream wrapper flatten，三协议output字段，Retry-After秒/120/零/date忽略，should-retry布尔，环境header trim/覆盖/跳过，base query路径，auth scheme与backend独立、UA形态。
- post捕获尾片段、无headerNone、resolver构造后轮换不影响已捕获片段、Bearer与XApiKey单头替换、callback一次且收到片段均有静态断言；尚未执行这些测试，不能以fixture构造代表网络到达服务端。client.rs从2531继续。


## client.rs 2531–2942 完成；retry.rs 1–245

- 客户端剩余测试覆盖 resolver=None 删除静态凭据、替换单头、callback缺失无操作；三协议未知扩展可跳过而已知坏payload/嵌套未知variant失败；Chat带choices不能伪装扩展。终止context_details两值覆盖total，缺失/部分不覆盖；正cost进metadata、零不存。均静态读取未运行。
- read_response_bytes 无audit时直接response.bytes全量缓冲；有audit逐chunk记录后追加Vec，audit失败映射EventStreamError。该函数本身无response大小上限/idle timeout，具体audit限额待audit模块核对，不能以注释“bounds”推断无条件限制。
- client.rs全部2942行读完，含文件尾生产函数，累计sampler 4/23个Rust文件完成。
- retry.rs 开头旧注释称413剥图、Auth fatal，与classify_error实际不符；实际先Auth/Api401 EmitToSession，即使max=0或有veto也先返回；再max=0 fatal，再veto fatal（413/context/false header）。
- doom-loop在上述检查之后直接Retry近即时，不检查普通retry_count预算；因此注释“never Fatal”仍受max=0影响。retry_count+1普通加法，极值不保证安全。
- 429 next=retry_count+1，cap=min(max,threshold)，next>=cap即fatal；默认threshold2允许一次重试，并非两次。其他retryable同next>=max规则，首次无论API5xx还是EmptyResponse/EventStream等均返回HTTP1重建决策，不只HTTP transport；后续Retry。Retry-After原u64秒直接使用，120秒上限在HTTP解析端，不是分类器统一约束。
- resolve_max_retries 每次读GROW_MAX_RETRIES，合法u32优先model然后15，非法值回退、不trim或clamp。模块声称纯无I/O，但公开resolver实际读env，backoff也有atomic全局序列。
- doom jitter hash序列+retrycount mod251为0..250ms。普通backoff base=2000 checked_shl(saturating(retrycount-1))后min30000，jitter ±20%，所以最终最大36000ms，不能写30s硬上限；checked_shl只检查shift宽度，不防移位丢高位，极大合法次数存在非单调边界。retry.rs format_sampling_error从246继续。

读取证据：`crates/codegen/sampler/src/client.rs` SHA256 `4ec5c0a414f185202693a6286ceda26365d3183bb8e2bc94a7b8f0a884f41f41`。


## retry.rs 246–795、types.rs 完整读取

- format_sampling_error按variant生成文案，retry_count Some一般加前缀，但Persistence分支不使用前缀。Http文案含错误URL（无query脱敏）；API403文案提示authentication与is_auth_error不接受403不是同一分类语义。Serialization输出line/column；Empty缺completion_tokens展示0；doom文案声称resampling仅为描述，不执行采样。
- clone_error 保留Auth credential、Api metadata/retry hints、Empty上下文、doom triggers/chunk等；Serialization仍为Serialization并保留原位置的文本，但重建错误的结构line/column不保证原值。Http无法Clone，转EventStreamError，会失去reqwest结构且后者总可重试，不能宣称任意Http复制后分类等价。
- 全部retry测试读取：env优先级、backoff0/1/2/10范围、Auth/401交session、413fatal、429 threshold、zero budget、首次5xx重建/后续retry/耗尽、EventStream/streamApi、Idle/Config/400/Serialization fatal、error文案、veto false/context500、true和缺省hint、doom99仍可重试。未覆盖极大计数溢出、doom max0或非retryable Http clone后语义；尚未动态执行。
- RequestId是String newtype，From接受任意字符串（包括空/非UUID），random产生UUIDv4，Display/as_str原样，serde字符串。4项测试检查From/Display与两次随机值不同及长度；不能据此声称全局无碰撞保证。

累计sampler 6/23个Rust文件完整读取；尚余actor、events、handle、audit、metrics、attribution、logging、doom-loop和stream等。

- `crates/codegen/sampler/src/retry.rs` SHA256 `d2ca4bd4cd877309261623c0221b1ac0781eb7377ea0b66090bfc7d9c77bc59e`

- `crates/codegen/sampler/src/types.rs` SHA256 `6e9e8c65a45a9e66e4b1233f911871b6170c859b09d32265a94ce37847226e56`


## attribution.rs、commands.rs 完整读取；events.rs 1–245

- SamplingConsumer 仅六个采样端点（两模式×三协议），as_endpoint固定snake_case字符串，不含embedding。Auth401AttributionCallback同步Send/Sync/Debug trait，非阻塞只在文档要求，没有执行限时或panic隔离。
- SENT_BEARER_PREFIX_LEN=12实际为尾12个Unicode字符，不是字节或前缀。短于等于12字符返回完整原串、空串原样，因此“full bearer never leaves”注释不适用于短key；单测明确abc完整返回。调用方可直接调trait传任意文本，截断保证来自client正常路径。
- SamplerCommand 为pub(crate)，Shutdown/Submit/Cancel/UpdateConfig/IsActive/ActiveCount；Submit boxed request+optional config，optional completion oneshot/scope/usage/evidence sink，结构本身不验证ID唯一或提供执行确认；关闭/取消语义需actor实现证明，非仅枚举注释。
- SamplingEvent包括StreamStarted/FirstToken/ChannelToken/ToolCallDelta/ResponseStarted/ReasoningCompleted/Completed/Retrying/Failed/ModelMetadata。响应开始含input与cache分桶，reasoning完成带signature；这些Messages专用发出约束是注释，后续stream核对。Event仅Clone/Debug，无serde。SamplingChannel serde默认变体名Text/Reasoning，未snake_case。
- SamplingErrorInfo保存kind/status/message/is_retryable/retry_after/modelmetadata及可选empty/doom/credential/usage；公开字段不校验kind对应payload。credential默认Unknown且序列化省略Unknown。SamplingErrorKind serde默认变体名，与as_str小写标签不是同一wire形式。
- From错误映射使用err.is_retryable（未应用veto）；InvalidConfiguration降Api且无status，Http和EventStream归Http，Api429归RateLimited、Api401仍Api（Auth variant才Auth）；将ShouldRetry hint丢弃的风险待剩余映射核对。events.rs从246继续。

累计sampler 8/23个Rust文件完整读取；本轮无构建，无动态测试。

- `crates/codegen/sampler/src/attribution.rs` SHA256 `09154dfc136e7eb421ecb23e23e4ebaaf7cea83a99eb624fddd620eb27853b19`

- `crates/codegen/sampler/src/commands.rs` SHA256 `d9b632d0999c20c5e60bcf92760de4ecc7587dbfa8a680d97f21a10143bfa9ab`


## events.rs 246–552、metrics.rs 完整读取

- SamplingErrorInfo::from保留Empty上下文/doom标签和chunk/Auth credential，usage始终None。未存独立should_retry，is_retryable只来自err.is_retryable。
- sampling_error_from_info：Persistence去一次固定前缀；Idle从message首个能解析且以s结尾的空白token恢复秒数，失败为0。Auth原文可累积显示前缀；Http全部变EventStreamError。Api/RateLimited用status_code有效值或500，should_retry=Some(info.is_retryable)，不保留原veto；InvalidConfiguration往返降Api500但should_retry=false。Empty无context退EventStreamError；doom缺triggers空Vec。usage未进入重建错误，不能宣称往返无损。
- 测试覆盖基础分类、旧payload缺credential、stream标签/数字code往返、Idle文字、Persistence非retryable保真；未覆盖API500+should_retry=false或Http非retryable往返，后续actor/L2影响需追踪。
- compute_percentiles要求非空且调用方已排序（不主动验证排序）；p50取len/2上中位，p99=ceil(len*0.99)-1夹上界，max取末项，sum普通u64求和、mean整数截断。任意极值数组不保证sum不溢出。
- from_timestamps TTLB取stream_end-start，TTFB取首内容chunk；空chunks仅TTLB，单chunk无ITL；相邻时间差转毫秒u64，interval保留输入顺序，仅副本排序算统计。chunk_count转u32饱和，attempts始终0等retry loop填写；公开时间输入不强制顺序或end晚于内容。
- record_on_span只写TTFB/TTLB/chunk_count/p50/p99，不写max/mean/attempts。六项测试读取空/单/双/多chunk、p99、尾metadata影响TTLB；未执行。

累计10/23个Rust文件完整读取，仍未完成crate。

- `crates/codegen/sampler/src/events.rs` SHA256 `b74e19292631afa20c9bc2f0466bc9e9a849eb48d61974b8067c4b2d73fa6053`

- `crates/codegen/sampler/src/metrics.rs` SHA256 `1f25ecc226acd0de02d0210135d2ba39748b6aa05e0ebc28c9169c9d5b7c3386`


## doom_loop.rs、sampling_log.rs、stream/mod.rs、stream/collect.rs 完整读取

- DoomLoopSignalCollector clone共享Arc Mutex，Default默认策略且armed，new写策略。disarm仅停止abort_triggers，继续记录；abort_triggers非drain confident raw列表。锁poison各入口降None/空/不写，不panic恢复。
- absorb依peek分类，命名check即使坏JSON仍吞；ResponseField记录但不吞（除非event名为check）。空signals与malformed每collector只debug一次。record按raw线性去重保序，没有容量上限；take清signals但不重置malformed_logged/disarm，take后同raw可再次记录。每attempt新collector来自client而非类型阻止跨attempt复用。
- 8项doom测试全部读取：check有/无event名、累计去重、ordinary/terminal转发、坏payload吞、take清空及disarm不停止记录。未运行。
- sampling_log只定义target和request_span，直接记录request/model/backend/base_url/AuthInfo片段，后续reasoning/output统计字段预留；不在本模块控制磁盘sink、启用开关或脱敏，不能从模块注释推断只有--log-sampling才可能观察所有事件。
- collect_response返回首Completed或Failed，忽略其他事件，不检查request_id一致、不读取首terminal之后尾部、不自行timeout。EOF无terminal生成Api kind、无status、is_retryable=false、无usage。四测试读取成功/错误/EOF/中间事件丢弃。
- stream::protocol_failure构造Serialization→ErrorInfo，显式附传入terminal usage，Failed携request_id；不自行验证usage确属terminal，调用方负责。该路径保留失败用量到ErrorInfo，反向SamplingError重建不包含usage这一点仍需actor如何消费核对。

累计14/23个Rust文件完整读取；尚余audit、handle、actor三文件、stream三个实现及messages_tests。没有构建产物新增。

- `crates/codegen/sampler/src/doom_loop.rs` SHA256 `a6dc667e40e3f28cc98147b35f26b7edc57d1afb37eb38f699070c4e043e455c`

- `crates/codegen/sampler/src/sampling_log.rs` SHA256 `478c5c1d61032782ac512c3f8146a8fd8a4819ce15051372a589f34e920b3a69`

- `crates/codegen/sampler/src/stream/mod.rs` SHA256 `c1ad263a968032629e517ea4cf91ba74c858d0ebc0724728e928b4ab1680fd46`

- `crates/codegen/sampler/src/stream/collect.rs` SHA256 `e9e6e0c182d81a5a305a8a83101b51fe384562522057c400011684678a8328c2`


## audit.rs、handle.rs 完整读取

- AttemptEvidence task-local scope共享Capture，sink是异步closure，模块不拥有文件系统持久化保证。request复制body并等待sink ACK，失败记录sticky failure；client随后mark_dispatched只代表execute获准，不证明服务端接收。
- response证据最多64MiB，追加可容纳前缀后置overflow并返回错误；恰好上限允许，溢出状态sticky。锁poison用expect panic（区别doom collector静默降级）。scope不自动传播到另spawn任务；实际调度待actor。
- finish先取snapshot，有request failure直接返回不写response；否则sink收body/status/truncated/outcome，ACK失败不清body，成功才清body，overflow仍返回错误且不重置flag/status/dispatched。重复finish不自动幂等，成功后可再次发空body；capture与finish并发使用可能清后续新增，正常每attempt序列责任需调用方核对。
- record无sink为Ok，有sink发送空body任意kind/metadata并等待；没有超时。audit自身无测试，不能据路径存在证明sink真正落盘。
- SamplerHandle使用unbounded mpsc与共享AtomicBool accepting；noop=false且receiver丢弃。close swap false仅首次send Shutdown，无等待join；submit检查accepting后send存在竞争窗口，最终拒绝依赖actor。fire-and-forget send错误忽略，无自动Failed确认。
- cancel/update_config不查accepting；queries走oneshot，发送失败或回包丢弃返回false/0，不代表可靠区分关闭与空闲，没有超时。
- submit_and_collect及accounted关闭时返回AuthUnknown关停错误；发送成功即装CancelOnDrop（只保证入队，不证明actor执行），future正常返回/取消/panic析构均send Cancel。completion丢弃也返回AuthUnknown。accounted透传scope/usage/evidence，config仍None；此处不执行结算，待request_task。handle无内联测试。

累计16/23个Rust文件完整读取；剩余actor/mod,state,request_task和stream/chat_completions,responses,messages,messages_tests。

- `crates/codegen/sampler/src/audit.rs` SHA256 `52a55cf16f4e3fd006f733fd36cc05807835564213c63aae830cc5cef16b6fd9`

- `crates/codegen/sampler/src/handle.rs` SHA256 `3e56a114d6bf2df12b0d5259948988bed5235c34a0ec45fce1d6fcd8af1f968e`


## actor/state.rs、actor/mod.rs 完整读取

- ActorState 以 RequestId 为 HashMap 键；register 返回旧值但自身不取消，cancel 先移除再取消，未知 ID 返回 false；更新 config 仅影响后续无 override 的请求，retry_policy 不变。3 项 state 测试完整读取，未执行。
- spawn_owned 创建 unbounded command channel 和 JoinSet；spawn 主动丢弃 JoinHandle，返回 handle；SamplerOwner 没有 Drop 关停逻辑。shutdown 先 close 再 take/await，重复调用无 task 返回成功；abort_and_join 直接 abort，不主动关闭 accepting，取消 JoinError 视为成功。bounded shutdown 先关 admission，超时 abort 且 await，仍返回超时错误。2 项 owner 测试完整读取，未执行。
- run biased 优先回收完成任务；正常退出只凭 RequestId 删除 active entry，没有 generation 校验。因此同 ID 替换后旧任务返回可删新 entry；JoinError 只告警，不清对应 entry。active_count 是登记数量，不等价 JoinSet 实际运行数量。
- Submit 注册新 token 后取消旧 token，每个请求复制 effective config/retry policy 并 spawn；没有 actor 侧 accepting 二次检查，close 与 submit 竞争的处理取决于 Shutdown 入队顺序。Cancel 立即移除登记，任务退出异步；queries 回复当前 map，发送失败忽略。
- 收到 Shutdown 或 command EOF 后先取消已登记 token，再 JoinSet.shutdown abort 并等待所有任务；不能将此描述为每项请求保证完成持久化结算，需继续核对 request_task。

累计18/23个Rust文件完整读取；剩余request_task和stream/chat_completions,responses,messages,messages_tests，crate仍pending。

- `crates/codegen/sampler/src/actor/state.rs` SHA256 `1b90b833498a8795b8e50f47b77c3912cd4b453d0e0c2e777f76978a8ea5d40c`

- `crates/codegen/sampler/src/actor/mod.rs` SHA256 `2b58a755f1776b4e590303e79e19e8e777e104f4d703b1c6a2a0bd56bd58cdbb`


## actor/request_task.rs 完整读取（2085 行，含测试）

- 初始 client 构建失败直接 Failed+completion，不进 attempt、不做 scope/usage/evidence。idle默认300秒；config max优先policy，显式0绕过env，其余通过env解析覆盖。doom仅max>0启用，独立budget/counter；output_observed跨attempt共享。
- 每attempt包装 evidence metadata attempt_number，task-local scope覆盖run_one_attempt；capture准入失败直接 EventStreamError终态，不走重试/finish。provider_started表示capture成功后进入provider future，不代表请求已发出。slot区分未准入与准入但无Goal；取消开流时仍携带准确scope。三backend均先capture再开流，开流select取消优先，capture未完成取消不登记provider_started。
- outcome usage来自成功/截断/失败终态；无usage时，仅有evidence且尚未dispatched才补精确零；否则严格无条件图片能力拒绝可补零。没有evidence时不能仅凭本地init failure推断零。Cancelled/InitFailed默认无usage。
- evidence.finish先await，随后provider_started且有sink才结算Known或Incomplete；usage失败优先报Persistence，之后再检查evidence_result，再观察cancel；这些finish/usage await没有取消select或超时。失败均先Failed再oneshot；不保证actor强制abort仍执行结算。
- transport重试在retry_only_before_output且任意输出已观察时effective budget归0；doom失败走独立分支，不咨询transport classifier或该输出门。doom预算花完下轮disarm，仍收集signals并可接受带信号响应。每轮request clone，最终metrics.attempts=两counter+1，metrics其他字段仅成功轮。
- 重试前必须等待retry evidence ACK，再发Retrying并sleep；audit与sleep均取消优先。rate threshold0替换常量默认；first rebuild等待backoff后强制HTTP1重建，失败告警继续复用旧client。Fatal exhausted span排除server veto，但max0/输出后budget0仍可能记录exhausted；日志不等价真正重试过。
- drive_l2优先poll stream，Completed先标observed output，再doom判断，再Length/ContextWindowExceeded/PauseTurn分类，最后空响应检测；ContentFilter豁免空响应重试。三截断类直接Completed部分响应，不本层自动continue/compact。
- output判据含首token/channel/tool delta（即使内容空）、terminal assistant非空content/toolcalls、reasoning非空、backend tool或billed output/reasoning>0。文本长度为bytes；emptycontext first_choice_seen只是model_id是否Some代理；raw_stop_reason优先保留未知wire值。
- L2 Failed优先取tee捕获的首个clone_error，fallback Info重建，usage仍来自info；因此真实raw Api的should_retry=false经clone保留，不能直接把Info丢否决的债务推广为所有actor失败。Http clone仍降EventStreamError。poison的errorcell静默跳过；scope slot poison panic。
- 非terminal转发不查send成功，retag实际上原样返回，不修改ID；stream EOF合成retryable EventStreamError。terminal与cancel同时ready时terminal优先保usage；非terminal积压最多转发一个即检查cancel。外层结算后再查cancel，所以terminal usage获结算并不保证最终Completed。
- cancellation事件Api/no status/nonretryable，而oneshot为AuthUnknown；send_completion take一次，所有channel send失败忽略。旧任务正常包括取消均返回ID，证实actor同ID替换后的旧任务回收风险。
- 全部内联测试静态读取：三backend admission/开流取消/请求ACK阻止wire；response及retry ACK/failure/lost/cancel门；raw非UTF8响应证据保留；persistence terminal；错误重建、backoff取消、first-error tee、未知finish、输出与截断/doom优先级、terminal取消用量、协议失败usage。尚未执行。response ACK取消用例同时释放ACK，不能证明永不ACK时取消能打断finish。

累计19/23个Rust文件完整读取，剩余四个stream文件，crate仍pending。

- `crates/codegen/sampler/src/actor/request_task.rs` SHA256 `469ce09789e65a93920c0797260d0545626db4016ce5867ce9365b3719004c03`


## stream/messages.rs 与 messages_tests.rs 完整读取

- 首次poll依次yield StreamStarted和可选ModelMetadata，后才读取wire；MessageStart必须id/model trim非空、type=message、role=assistant、content空、stop_reason=None且不可重复；ResponseStarted携原始uncached/cache计数，不将其合为prompt。Ping/Error可在start前出现，其余事件不允许。
- block index只要求未用过，不要求连续或递增，可多block并行打开；delta类型匹配已开block，stop只能一次；首MessageDelta后禁一切block事件。Image/ToolResult拒绝。message_delta出现未闭合block置sticky错误但继续读到终态usage；一般生命周期错误立即Failed无usage。
- Text/Thinking start即使初始文本为空也发一次FirstToken；初始非空文本只入累积，不发ChannelToken、不计message chunks/latency；tool start发id/name delta，无FirstToken。非空text delta计Text通道、共享chunk_index、messagecount和时间戳；thinking仅共享chunk_index及Reasoning，不计latency；tool参数非空才发delta，但空片段也将args_started置true。
- signature delta直接替换此前signature，不拼接；Thinking stop有签名发ReasoningCompleted，非空思考转synthesized item；native保留signature。RedactedThinking不发实时事件/visible item，但closed native保留opaque data。Text与ToolUse native的cache_control重建为None，不能称所有wire字段逐字保留。
- closed text按stop到达顺序聚合，中间插换行；toolcalls/reasoning也按关闭顺序，native以BTreeMap index升序，与durable items可异序。没有工具ID/name非空或跨block重复ID检查；输出项大小与block数无本层上限。
- tool initial input必须object；若出现任何delta则初始object必须空，用delta原文；无delta则initial JSON重序列化。验证trim_start以{起且完整IgnoredAny反序列化，故空delta/数组/null/串接对象/未完JSON失败。坏tool仅置sticky错误，健康兄弟也不能产生Completed；已发预览delta不撤回。
- message_delta允许多次，stop_reason不得为空或与已有raw不同，stop_sequence不得与已有不同；不强制stop_sequence只出现在对应reason。输出usage每次覆盖，可下降；optional input/cache缺省保留，Some0覆盖。stop_details出现时explanation直接替换，None可清已有message。
- EndTurn/StopSequence→Stop、MaxTokens→Length、ToolUse→ToolCalls、Refusal→ContentFilter、PauseTurn/context超限各保留；Unknown非空warn→Stop且raw原值保留。无reason的usage delta保留前reason。任何已闭合有效tool使最终内部stop_reason=ToolCalls，覆盖包括Length/Refusal/context/PauseTurn；raw_stop_reason仍provider原值。
- MessageStop立即break不poll尾流；必须有MessageDelta、reason和所有block关闭，否则协议失败。usage仅stop_seen+reason存在时可信，即使invalid_response也附终态usage；prompt=u32饱和uncached+read+write，total饱和加output，reasoning0，cache read/write分别保留，合法全零也Some。
- EOF无完整终态协议失败；raw/provider Error立即Failed不保留非终态usage。每next有idle timeout，另std::Instant内容时钟使Ping/空delta/重复相同usage不能无限续命；结构事件及非空signature算进展，message_delta仅首个/新reason/usage变化续命，stop details或sequence单独变化不续命。空事件检查elapsed严格>，timeout诊断as_secs对子秒截断0。
- 成功固定reasoning items后一个Assistant；native=Some(Messages(sorted blocks))可空；无cost/doom，model_id来自start，effort/fingerprint None。metrics仅text deltas，不能将FirstToken事件时点等同TTFB。
- 测试文件1124行全部静态读取：畸形生命周期、JSON原子拒绝含健康兄弟、terminal usage保留、不poll尾、空delta超时、text/thinking/native签名、工具delta、各stop映射及工具覆盖、未闭合thinking/tool失败、529/transport/idle、metadata顺序、cache buckets。多thinking签名测试没发message_delta只检查中间签名，metadata测试也用非法流只检查前两事件，均不能证明成功终态。尚未执行。

累计21/23个Rust文件完整读取；剩余chat_completions.rs和responses.rs，sampler仍pending。前述request_task注释声称截断丢弃未闭合block不符合当前Messages代码及失败测试，正式契约采用此处事实。

- `crates/codegen/sampler/src/stream/messages.rs` SHA256 `c5cc9a0bac11782786a8603d785cbfa9e1b71e0f762f09d2bff29e36aea1ee8a`

- `crates/codegen/sampler/src/stream/messages_tests.rs` SHA256 `76ed03f4734246b8e696e9a7302c55cce56dd76f5d68ff59fa9f1265b6d7cf6f`


## stream/chat_completions.rs 部分读取（1–270 行）

- merge_tool_identity忽略缺省/trim空输入，首次保留原值，重复完全相同不发更新，冲突Serialization。
- 首chunk取id/model/fingerprint（只过滤空字符串）；以后只校验id不变，不验证model一致或id非空。每chunk至多1choice且index必须0；usage/cost每次覆盖，cost缺报不抹已知值。
- finish_reason首现启动固定2秒tokio尾deadline，每next用剩余与idle较小值；finish后timeout作为正常尾结束，尾raw错误仍Failed。重复finish须原wire完全相同且非trim空；同frame首finish仍可含output，后续frame非空text/reasoning或任何tool_calls拒绝。
- 此段text非空计FirstToken/text chunk/latency，reasoning非空也计FirstToken/shared index但不计text latency。待从271行继续，未将文件记为完整读取。

另一任务报告main正在重新链接CLI；其target归该任务所有，本次未触碰，也未把main新归档变更计入当前分支证据。


## stream/chat_completions.rs 完整读取（续271–1404，含全部测试）

- tool kind Some必须精确function，含空kind也拒绝；缺失允许。id/name trim空且args None/空的delta直接忽略不建entry；args空白非空仍会累积。按index BTreeMap聚合，不要求连续；允许参数先于identity，重复相同identity不发事件、不续idle；新identity或非空args才发ToolCallDelta，无FirstToken或text latency。
- EOF必须有choice且finish，纯文本EOF也Serialization失败。收尾仅验证arguments为完整JSON（IgnoredAny），没有Messages的object约束，也未检查最终id/name非空或跨index ID唯一。有效null/数组/数字按此代码可通过L2；正式事实不能写成已完成工具语义校验。任一坏args整轮Failed且附最后usage，不输出Completed。
- 最终工具按index升序，任意工具覆盖typed finish为ToolCalls但raw保持；Assistant含首chunk model/fingerprint，无effort。reasoning synthesized sibling置前，同时native Chat消息保留reasoning_content；response.message_id=None，即使wire id已校验。无stop_message/sequence/doom。first_choice_seen=false fallback分支被前置失败阻止，不是空流成功语义。
- meaningful只有非空text/reasoning、新工具identity/args、首次finish；usage-only/role-only/重复finish不续内容时钟。finish后content idle超时作为正常尾结束；尾deadline固定2秒不随帧更新，但消费者暂停poll/yield也消耗wall-clock尾预算。transport错误即使已有finish仍失败；多数协议错误不附usage，conflicting finish与收尾JSON失败显式附usage。
- 完整测试静态读取：空/文本EOF、id冲突、unsupported tool、终态冲突及晚output；短idle限制尾且无虚构usage；所有known/unknown reason及tool override；reasoning-only仍empty；文本/思考首token；工具identity补齐/重复/冲突、参数先于identity、UTF8合法边界切片、nonzero choice、坏JSON、noop不续idle、metadata、usage/cost保留。测试名multiple_choices的用例实测非零单choice，未直接覆盖同chunk两个choice；2秒固定尾上限也未由20ms idle测试独立证明。未执行。

累计22/23个Rust文件完整读取，仅剩responses.rs；sampler仍pending。

- `crates/codegen/sampler/src/stream/chat_completions.rs` SHA256 `680b77543fcbe42e95327052c308ea8015e27251c854db63196be7683a23e885`


## stream/responses.rs 完整读取（1564 行，含全部测试）

- response_usage拷贝input/output/total/reasoning/cache-read，cache-write0，不本层重算context；context改写来自L1。raw stop为status[:incomplete reason]，供应商未知detail不驱动Length。
- meaningful排除created/inprogress/queued、空文本/参数等；hosted tool状态、item/part结构和未知兜底true，即使不转发也置output_observed；ResponseError专门不标输出。此标记在协议校验和doom前，坏事件也可留下observed事实。
- 每次next先timeout/raw error，然后检查collector abort，再处理event；armed confident即使终态frame也先Failed Doom且不附该frame usage。collector仅在raw返回Ok事件时检查，吞掉check后无后续事件可能等到idle或EOF；不支持单靠collector独立唤醒。
- created/inprogress/queued只校验response id一致，可缺全部前置事件，可空id；终态Completed/Incomplete检查相应status，Failed检查failed status；Failed直接映射provider error附usage，不校验前置response id。普通ResponseError无usage。
- text delta实时发Text，不累积最终text；summary delta发Reasoning但不累积fallback；reasoning text delta额外累积fallback。非空text才记latency/messagecount，text与reasoning共享chunkindex。refusal、text done、hosted tools/annotation等除进展判据外不发事件。
- OutputItemAdded index不可重复或已done；function按到达顺序分配tool-only index，发call_id/name，不发initial arguments。arguments delta必须对应function且未argsdone/itemdone，call.id Some才比item_id；argsdone要求前缀与可选name一致，允许补全后缀，不发补全delta。
- itemdone可无added，但不可重复；拒绝已见function改类型及非function改function；非function生命周期不完整交叉校验。streamed function终态按output vector索引比id/call_id/name/namespace和args前缀，argsdone后要求相等；done function要求identity/args完全相同且done status无或Completed。
- 终态snapshot权威，允许省略中间events但不能丢已见tool；未给status的terminal call若匹配done补Completed，显式Incomplete/InProgress不覆盖，其他完整性由sampling-types转换器校验。无created也可terminal-only。sequence_number不校验；普通text预览不与terminal核对，最终正文取snapshot可与已发内容不同。
- terminal provider error优先于id/tool交叉校验，映射Failed附usage；conversion失败同样附usage。metadata cost键移除后parse i64，本层不限制正数（L1注入已有筛选，直接L2调用可传负）。native先于reasoning fallback生成，fallback只改durable items。
- 成功Completed→Stop，Incomplete仅max_output_tokens→Length、content_filter→ContentFilter，其他Stop；有效assistant toolcalls覆盖ToolCalls。raw保status及detail，无message_id/stop_message/sequence。成功才take collector并warn rawlabels，失败不drain。
- 完成/不完整事件先经过content idle检查再break；空terminal不算进展，若最后内容已超时可返回IdleTimeout丢terminal usage，不能无条件宣称terminal优先。正常break不poll尾，EOF缺终态Serialization。
- 全部测试静态读取：terminal缺失/stop映射、不poll尾errors/pending、失败500/transport/idle、metadata、nonforwarded refusal输出门、empty completion、tool冲突矩阵/完整性状态/孤儿/工具索引、doom armed/disarmed及empty signals。text delta测试最终snapshot为空，仅验证预览及stop，不证明正文保留。尚未执行。

sampler 全部23个Rust文件及manifest静态读取完成；尚待包测试、正式contract映射与hash登记，因此inventory保持pending，不提前增加完成数。

- `crates/codegen/sampler/src/stream/responses.rs` SHA256 `d310a4bce7060d55514927bc7161f5800cf64f00b81136db7baec6a764d9974e`


## 包测试完成与源码范围纠正

`cargo test --locked --offline -p sampler --all-features` 退出0：218 unit + 33 integration（6+1+1+3+22）全部通过，0失败/忽略；doc-tests0。日志 `/tmp/grow-sampler-tests.log`。独立target关闭incremental/dev-test debug，结束立即cargo clean删除5425文件1.9GiB，可用磁盘69GiB。测试仅本机fixture，不代表真实provider兼容性全面验证。

重新列包文件发现此前23仅统计src，另有tests/cf_edge_error_message.rs、request_query_and_headers.rs、shared_http_kill_switch.rs、shared_http_wire.rs、support/mod.rs、test_actor.rs共6份Rust文件未静态阅读。故“全部23”应明确为src全部；真正包总数29份，需补读这6份后才登记reviewed。shared_http注释引用的集成二进制现在已定位并执行，静态覆盖分析仍待进行。

34项初步contract已写入sampler-features.draft.json；尚缺actor/audit/协议细项，未并入正式feature-map或增加完成计数。


## tests/ 六份源码完整读取

- cf_edge_error_message 128行：3项Chat stream mock HTTP检查524/503 HTML不进用户Display及429结构message保留，另3项纯sampling-types状态文案/空body/JSON envelope；不证明日志或审计body被抹除，也不覆盖全部六端点。
- request_query_and_headers 72行：本地HTTP实际捕获query与headers，确认覆盖旧api-version、保留keep、编码空格、env值trim。临时env唯一名但非RAII恢复；support::send_one忽略conversation结果，返回{}不是合法completion，验证仅wire请求。
- shared_http_kill_switch 37行：独立binary首次client前设0，两client顺序请求间隔50ms，TCP accepts=2。shared_http_wire90行：Once pin env，默认两client顺序共享一次accept；不同key/extra头隔离；forceHTTP1两请求2accept。仅本地明文HTTP，不验证真实TLS/HTTP2重连故障。
- support/mod.rs32行仅config/user request/send_one，泛用counting server来自test-support。
- test_actor1338行：22项mockHTTP集成涵盖初始空闲/未知取消、开始到文本终态、collect、首token后取消、owner shutdown join与channel关闭、不同ID并发、500重试、输出前空回复重试、输出后transport/reasoning-empty不重试、429、401、Messages refusal含空refusal、损坏事件不重试、更新config后model变化、三backend扩展帧四模式、doom信号透传及一次resample。
- 限流断言允许1..=3请求，不能当作精确2次实测；并发测试使用不同ID，不覆盖重复ID回收债务；owner关停仅join/channel，无持久化结算断言。500测试hits>=2；first output后的Failed仍is_retryable，只表示actor当前策略不重放，不改变错误本身类别。
- 三backend扩展矩阵验证heartbeat-only idle、SSE event名ping不遮真实payload error、扩展交错正文成功、缺terminal协议失败，均无Retrying且active_count0。Responses后置坏JSON仅证明terminal后不再消费；不证明Chat也忽略finish后的尾错误。测试harness sleep20ms启动、shutdown只发signal不joinserver；drain首terminal返回，不自动证明后续没有第二terminal。

本包29份Rust源码及Cargo.toml已完整静态审阅；251测试通过记录有效。待补齐契约草稿再正式登记，不提前标reviewed。

- `crates/codegen/sampler/tests/cf_edge_error_message.rs` SHA256 `cf3dc0e0b5a800b5a1eda2ab68a4ab072550fb8fe0b60ee922d8540f1a0f5d55`

- `crates/codegen/sampler/tests/request_query_and_headers.rs` SHA256 `6263fefaf48e704d671af757dcea2eeef0aa68d9fe9caac37c6e56b8c6e0dfb9`

- `crates/codegen/sampler/tests/shared_http_kill_switch.rs` SHA256 `e9322981ada0ae354baaf1cd3c3e3ed889aef6f9067164e387438a39b77b340a`

- `crates/codegen/sampler/tests/shared_http_wire.rs` SHA256 `7abf98b847bbee34c772d0116aad700cffba1842de956acf152dcdd0f2ccb8b0`

- `crates/codegen/sampler/tests/support/mod.rs` SHA256 `17db6ebcda0280c904a3472841308eda65251d15898e0321ef70bed825003558`

- `crates/codegen/sampler/tests/test_actor.rs` SHA256 `fc009d8618c375f8fc80361a17562e860e9da64218295cb5f15bdbb06550c749`


## 正式契约登记

29份Rust源码及manifest全部审阅，96项要求映射至model-sampling delta；251项包测试通过，动态边界见上方测试分析，构建产物已清理。历史进行中计数由本节最终状态取代。

- Sampler configuration and auth independence
- Sampler configuration hook serialization
- Sampler shared HTTP client lifetime
- Sampler HTTP transport defaults
- Sampler endpoint query merging
- Sampler header construction precedence
- Sampler fresh resolver authority
- Sampler credential attribution capture
- Sampler credential fragment boundary
- Sampler request size barrier
- Sampler request evidence admission
- Sampler response header hints
- Sampler Chat request defaults
- Sampler Responses request defaults
- Sampler Messages request defaults
- Sampler stream byte and extension handling
- Sampler Chat extension discrimination
- Sampler Responses terminal context and cost
- Sampler SSE error termination boundary
- Sampler direct conversation collection
- Sampler response buffering limit boundary
- Sampler retry budget resolution
- Sampler retry classifier precedence
- Sampler retry attempt limits
- Sampler initial retry client rebuild
- Sampler retry delay arithmetic
- Sampler error clone fidelity
- Sampler request identifier representation
- Sampler event error representation
- Sampler event transport shape
- Sampler collection terminal contract
- Sampler latency statistics
- Sampler doom signal collector
- Sampler doom collector poison and checks
- Sampler attempt evidence response cap
- Sampler evidence acknowledgement lifecycle
- Sampler task scoped audit boundary
- Sampler handle admission and close
- Sampler handle query failure defaults
- Sampler collect future cancellation guard
- Sampler actor request isolation
- Sampler active registry semantics
- Sampler actor shutdown ownership
- Sampler bounded owner shutdown
- Sampler provider scope admission
- Sampler attempt usage settlement
- Sampler persistence before terminal and retry
- Sampler retry evidence gate
- Sampler output observed replay policy
- Sampler independent doom resampling budget
- Sampler L2 outcome precedence
- Sampler terminal cancellation usage priority
- Sampler raw error tee and event forwarding
- Sampler cancel terminal representations
- Sampler wire shared connection verification
- Sampler user facing edge errors
- Sampler Chat candidate and response identity
- Sampler Chat finish evidence and bounded tail
- Sampler Chat terminal conflict checks
- Sampler Chat content and latency
- Sampler Chat tool identity assembly
- Sampler Chat tool JSON completion boundary
- Sampler Chat tool ordering and stop override
- Sampler Chat native and reasoning response
- Sampler Chat usage and cost updates
- Sampler Chat content idle completion boundary
- Sampler Messages start and block lifecycle
- Sampler Messages message delta phase
- Sampler Messages initial content preview
- Sampler Messages delta channels and signatures
- Sampler Messages tool JSON object admission
- Sampler Messages visible and native ordering
- Sampler Messages opaque reasoning separation
- Sampler Messages stop reason preservation
- Sampler Messages repeated terminal delta updates
- Sampler Messages terminal usage accounting
- Sampler Messages terminal boundary and EOF
- Sampler Messages content aware idle
- Sampler Responses progress and output tracking
- Sampler Responses doom abort timing
- Sampler Responses response lifecycle identity
- Sampler Responses terminal status correspondence
- Sampler Responses preview and final content
- Sampler Responses function output registration
- Sampler Responses function argument lifecycle
- Sampler Responses item done evidence
- Sampler Responses terminal tool consistency
- Sampler Responses conversion errors and usage
- Sampler Responses incomplete control semantics
- Sampler Responses native fragment and fallback
- Sampler Responses terminal consumption and idle
- Sampler Responses terminal cost and signals
- Sampler diagnostic disclosure boundary
- Sampler sampling span fields
- Sampler user agent composition
- Sampler error display reconstruction
